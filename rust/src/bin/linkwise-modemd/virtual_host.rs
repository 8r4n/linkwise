//! A radio-free host service. Each virtual cable message carries one 48 kHz
//! mono waveform; the service has no audio-device or PTT access.
use std::{io, net::SocketAddr, time::Duration};

use async_trait::async_trait;
use linkwise::{
    api::Command,
    host::{Channel, ModemPort, Reply, tcp::Listener},
    modem::{
        audio_handshake::{Action, AudioHandshake},
        bw500,
        bw500_waveform::Waveform,
        control::{self, Bandwidth, Bandwidth::Hz500, Bw500IntermediateReply, Message},
        data,
        handshake::{Established, Event},
        setup::{LinkSetup, Profile},
        waveform::BurstDetector,
        wide_level4,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    time::{Instant, timeout},
};

const PHASE_TIMEOUT: Duration = Duration::from_secs(12);
const FINAL_ACK_WAIT: Duration = Duration::from_secs(3);
const FINAL_QUERY_DELAY: Duration = Duration::from_millis(3200);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_AUDIO: usize = 48_000 * 6;
const MAX_PAYLOAD: usize = 4096;
const MAX_RECEIVE: usize = 4096;
const HOST_BATCH_WINDOW: Duration = Duration::from_millis(100);

enum Request {
    Command(Command, oneshot::Sender<Reply>),
    Data(Vec<u8>, oneshot::Sender<io::Result<()>>),
}

struct Port(mpsc::Sender<Request>);

#[async_trait]
impl ModemPort for Port {
    async fn command(&self, command: Command) -> Reply {
        let (send, receive) = oneshot::channel();
        if self.0.send(Request::Command(command, send)).await.is_err() {
            return Reply::Wrong;
        }
        receive.await.unwrap_or(Reply::Wrong)
    }

    async fn data(&self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > MAX_PAYLOAD {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "virtual modem delivery exceeds the transmit limit",
            ));
        }
        let (send, receive) = oneshot::channel();
        self.0
            .send(Request::Data(bytes.to_vec(), send))
            .await
            .map_err(|_| io::Error::other("virtual cable closed"))?;
        receive
            .await
            .map_err(|_| io::Error::other("virtual cable closed"))?
    }

    async fn closed(&self, _: Channel) {
        // The owning select drops the actor and its cable on either host EOF.
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Handshake,
    ConfirmAck,
    ResponderConfirm,
    CallerReady,
    Collecting,
    PollAck,
    TurnGrant,
    DataAck,
    IntermediateQueryShort,
    IntermediateQueryLong,
    FinalQueryShort,
    FinalQueryLong,
    Responder,
    Receiving,
    Sent,
    TurnReleased,
    Received,
}

enum Mode {
    Bw500 {
        profile: String,
        waveform: Box<Waveform>,
    },
    Wide {
        bandwidth: Bandwidth,
        waveform: Box<wide_level4::Waveform>,
    },
}

impl Mode {
    fn bandwidth(&self) -> Bandwidth {
        match self {
            Self::Bw500 { .. } => Hz500,
            Self::Wide { bandwidth, .. } => *bandwidth,
        }
    }

    fn width(&self) -> u16 {
        match self.bandwidth() {
            Hz500 => 500,
            Bandwidth::Hz2300 => 2300,
            Bandwidth::Hz2750 => 2750,
        }
    }

    fn handshake(&self, local: &str, listen: bool) -> io::Result<AudioHandshake> {
        let setup = LinkSetup {
            caller: local.into(),
            high_speed: false,
            requested_level: 4,
        };
        match self {
            Self::Bw500 { profile, .. } => AudioHandshake::new(
                setup,
                listen,
                PHASE_TIMEOUT,
                Waveform::from_json(profile).map_err(io::Error::other)?,
            ),
            Self::Wide { bandwidth, .. } => {
                AudioHandshake::new_wide(setup, listen, PHASE_TIMEOUT, *bandwidth)
            }
        }
        .ok_or_else(|| io::Error::other("invalid virtual station callsign"))
    }
}

struct Engine<'a> {
    local: &'a str,
    mode: &'a Mode,
    wide_lead: Option<wide_level4::Lead>,
    wide_level3: Option<wide_level4::LowerWaveform>,
    wide_level2: Option<wide_level4::RobustWaveform>,
    wide_rate: u8,
    wide_outstanding: Vec<f32>,
    handshake: AudioHandshake,
    session: Option<Established>,
    phase: Phase,
    listen: bool,
    disconnect_pending: bool,
    pending: Vec<u8>,
    plan: Option<data::TransmitPlan>,
    delivery_len: usize,
    send_index: usize,
    intermediate_transmissions: usize,
    retried_data: bool,
    retry_on_confirm: bool,
    returned_turn: bool,
    caller_requested_turn: bool,
    initial_poll: bool,
    closed_at_level3: bool,
    rejected_over: bool,
    receive_bytes: Vec<u8>,
    last_received: Option<(u8, bw500::Frame)>,
    deadline: Option<Instant>,
    start: Instant,
    notices: mpsc::Sender<Vec<u8>>,
    received: mpsc::Sender<Vec<u8>>,
}

impl Engine<'_> {
    fn reset(&mut self) -> io::Result<()> {
        self.handshake = self.mode.handshake(self.local, self.listen)?;
        self.wide_lead = None;
        self.wide_rate = 4;
        self.wide_outstanding.clear();
        self.session = None;
        self.phase = Phase::Handshake;
        self.deadline = None;
        self.disconnect_pending = false;
        self.pending.clear();
        self.plan = None;
        self.delivery_len = 0;
        self.send_index = 0;
        self.intermediate_transmissions = 0;
        self.retried_data = false;
        self.retry_on_confirm = false;
        self.returned_turn = false;
        self.caller_requested_turn = false;
        self.initial_poll = false;
        self.closed_at_level3 = false;
        self.rejected_over = false;
        self.receive_bytes.clear();
        self.last_received = None;
        Ok(())
    }

    fn notify(&self, message: String) -> io::Result<()> {
        self.notices
            .try_send(message.into_bytes())
            .map_err(io::Error::other)
    }

    fn connected(&self) -> io::Result<()> {
        let link = self
            .session
            .as_ref()
            .ok_or_else(|| io::Error::other("missing session"))?;
        self.notify(format!(
            "CONNECTED {} {} {}\r",
            link.caller,
            link.called,
            self.mode.width()
        ))
    }

    async fn disconnect(&mut self, cable: &mut Cable) -> io::Result<()> {
        if let Some(link) = &self.session {
            cable
                .send(
                    Message::SessionDisconnect {
                        responder: link.called == self.local,
                    }
                    .synthesize_on(&link.called, self.mode.bandwidth()),
                )
                .await?;
        }
        self.reset()?;
        self.notify("DISCONNECTED\r".into())
    }

    fn transition(&mut self, phase: Phase) {
        self.phase = phase;
        self.deadline = Some(Instant::now() + PHASE_TIMEOUT);
    }

    async fn establish(&mut self, mut action: Option<Action>, cable: &mut Cable) -> io::Result<()> {
        while let Some(next) = action {
            match next {
                Action::Transmit { token, audio } => {
                    cable.write(&audio).await?;
                    action = self.handshake.handle(
                        self.start.elapsed(),
                        Event::Transmitted {
                            token,
                            success: true,
                        },
                    );
                    self.deadline = Some(Instant::now() + PHASE_TIMEOUT);
                }
                Action::Established(session) => {
                    let caller = session.caller == self.local;
                    if let Mode::Wide { waveform, .. } = self.mode {
                        self.wide_lead = if caller {
                            waveform.caller_data_lead(&session.caller)
                        } else {
                            waveform.responder_lead(&session.caller)
                        };
                    }
                    self.session = Some(session);
                    if caller {
                        self.transition(Phase::ConfirmAck);
                    } else {
                        if self.mode.bandwidth() == Hz500 {
                            self.phase = Phase::ResponderConfirm;
                            self.deadline = Some(Instant::now() + HOST_BATCH_WINDOW);
                        } else {
                            cable
                                .send(control::synthesize_connected_ack(
                                    self.mode.bandwidth(),
                                    self.session.as_ref().unwrap().caller.as_str(),
                                ))
                                .await?;
                            self.phase = Phase::Responder;
                            self.deadline = None;
                        }
                        self.connected()?;
                    }
                    break;
                }
                Action::Failed(_) => {
                    self.reset()?;
                    self.notify("DISCONNECTED\r".into())?;
                    break;
                }
            }
        }
        Ok(())
    }

    async fn host_command(&mut self, command: Command, cable: &mut Cable) -> io::Result<Reply> {
        let idle = self.phase == Phase::Handshake && self.deadline.is_none();
        match command {
            Command::Version => Ok(Reply::Version(format!(
                "LINKWISE VIRTUAL BW{}",
                self.mode.width()
            ))),
            Command::MyCall(calls) if calls.as_slice() == [self.local] => Ok(Reply::Ok),
            Command::Bw500 if self.mode.bandwidth() == Hz500 => Ok(Reply::Ok),
            Command::Bw2300 if self.mode.bandwidth() == Bandwidth::Hz2300 => Ok(Reply::Ok),
            Command::Bw2750 if self.mode.bandwidth() == Bandwidth::Hz2750 => Ok(Reply::Ok),
            Command::CompressionOff | Command::Public(false) | Command::Cwid(false) => {
                Ok(Reply::Ok)
            }
            Command::P2pSession | Command::ClearTxBuffer if idle => Ok(Reply::Ok),
            Command::Listen(on) if idle => {
                self.listen = on;
                self.reset()?;
                Ok(Reply::Ok)
            }
            Command::Connect {
                source,
                destination,
            } if idle && source == self.local => {
                let action = self.handshake.handle(
                    self.start.elapsed(),
                    Event::Call {
                        called: destination,
                    },
                );
                if action.is_none() {
                    return Ok(Reply::Wrong);
                }
                self.establish(action, cable).await?;
                Ok(Reply::Ok)
            }
            Command::Disconnect => {
                if self.session.is_some() && !self.pending.is_empty() {
                    self.disconnect_pending = true;
                } else {
                    self.disconnect(cable).await?;
                }
                Ok(Reply::Ok)
            }
            Command::Abort => {
                self.reset()?;
                self.notify("DISCONNECTED\r".into())?;
                Ok(Reply::Ok)
            }
            _ => Ok(Reply::Wrong),
        }
    }

    fn host_data(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() || bytes.len() > MAX_PAYLOAD - self.pending.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "virtual modem transmit buffer is full",
            ));
        }
        match self.phase {
            Phase::CallerReady
            | Phase::Responder
            | Phase::Sent
            | Phase::TurnReleased
            | Phase::Received => {
                self.initial_poll = self.phase == Phase::CallerReady;
                self.phase = Phase::Collecting;
                self.deadline = Some(Instant::now() + HOST_BATCH_WINDOW);
            }
            Phase::Collecting => self.deadline = Some(Instant::now() + HOST_BATCH_WINDOW),
            Phase::ResponderConfirm
            | Phase::PollAck
            | Phase::TurnGrant
            | Phase::Receiving
            | Phase::DataAck
            | Phase::IntermediateQueryShort
            | Phase::IntermediateQueryLong
            | Phase::FinalQueryShort
            | Phase::FinalQueryLong => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "virtual modem cannot queue DATA in this phase",
                ));
            }
        }
        self.pending.extend_from_slice(bytes);
        self.report_buffer()
    }

    fn report_buffer(&self) -> io::Result<()> {
        let acknowledged = self
            .plan
            .as_ref()
            .map_or(self.send_index * self.wide_block_bytes(), |plan| {
                self.send_index * plan.full_payload_bytes()
            });
        let remaining = self.pending.len() - acknowledged;
        self.notify(format!("BUFFER {remaining}\r"))
    }

    fn plan_delivery(&mut self) -> io::Result<()> {
        if let Mode::Wide { .. } = self.mode {
            self.wide_rate = 4;
            self.wide_lead
                .as_mut()
                .expect("wide link has a lead stream")
                .begin_delivery();
            self.delivery_len = data::wide_delivery_prefix(self.pending.len());
            self.plan = None;
        } else {
            // Stock BW500 retains level 3 for a short continuation
            // after a level-3 close, even when that close advertises level 4.
            let prefix = if self.closed_at_level3 && self.pending.len() <= 31 {
                data::TransmitPlan::level3_retry(self.pending.len())
                    .map(|plan| (self.pending.len(), plan))
            } else {
                data::TransmitPlan::prefix(self.pending.len())
            };
            let (length, plan) =
                prefix.ok_or_else(|| io::Error::other("could not plan nonempty BW500 delivery"))?;
            self.delivery_len = length;
            self.plan = Some(plan);
        }
        self.send_index = 0;
        self.intermediate_transmissions = 0;
        self.retried_data = false;
        Ok(())
    }

    fn wide_block_bytes(&self) -> usize {
        match self.wide_rate {
            2 => data::WIDE_ROBUST_FULL_BYTES,
            3 => data::WIDE_LOWER_FULL_BYTES,
            _ => data::WIDE_FULL_BYTES,
        }
    }

    async fn start_delivery(&mut self, cable: &mut Cable) -> io::Result<()> {
        self.plan_delivery()?;
        self.caller_requested_turn = false;
        let link = self.session.as_ref().expect("sending requires a session");
        if self.initial_poll {
            cable
                .send(Message::Bw500KeepaliveA.synthesize_on(&link.called, self.mode.bandwidth()))
                .await?;
            self.transition(Phase::PollAck);
        } else {
            let request = if link.caller == self.local {
                Message::Bw500CallerTurnRequest
            } else {
                Message::Bw500ResponderTurnRequest
            };
            cable
                .send(request.synthesize_on(&link.caller, self.mode.bandwidth()))
                .await?;
            self.transition(Phase::TurnGrant);
        }
        self.initial_poll = false;
        Ok(())
    }

    async fn acknowledge_delivery(&mut self, cable: &mut Cable) -> io::Result<()> {
        self.closed_at_level3 = self
            .plan
            .as_ref()
            .is_some_and(data::TransmitPlan::closes_at_level3);
        self.pending.drain(..self.delivery_len);
        self.send_index = 0;
        self.delivery_len = 0;
        self.plan = None;
        self.wide_outstanding.clear();
        self.report_buffer()?;
        if self.pending.is_empty() {
            if self.disconnect_pending {
                return self.disconnect(cable).await;
            }
            self.phase = Phase::Sent;
            self.deadline = None;
            let link = self
                .session
                .as_ref()
                .expect("acknowledging an active delivery");
            if self.mode.bandwidth() == Hz500 && link.called == self.local {
                cable
                    .send(Message::Bw500ResponderTurnRelease.synthesize(&link.called))
                    .await?;
                self.await_handed_back_data();
            }
        } else {
            self.start_delivery(cable).await?;
        }
        Ok(())
    }

    fn await_handed_back_data(&mut self) {
        self.receive_bytes.clear();
        self.last_received = None;
        self.rejected_over = false;
        self.phase = Phase::TurnReleased;
    }

    fn is_final_caller_frame(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|link| link.caller == self.local)
            && self
                .plan
                .as_ref()
                .is_some_and(|plan| self.send_index == plan.full_frames())
    }

    async fn expire(&mut self, cable: &mut Cable) -> io::Result<()> {
        if self.phase == Phase::ResponderConfirm {
            if !self.pending.is_empty() {
                self.plan_delivery()?;
                self.retry_on_confirm = true;
                return self.send_over(cable).await;
            }
            let link = self
                .session
                .as_ref()
                .expect("confirmation requires a session");
            cable
                .send(control::synthesize_connected_ack(Hz500, &link.caller))
                .await?;
            self.phase = Phase::Responder;
            self.deadline = None;
            return Ok(());
        }
        if self.phase == Phase::Collecting
            || self.phase == Phase::Received && !self.pending.is_empty()
        {
            return self.start_delivery(cable).await;
        }
        if self.phase == Phase::DataAck
            && self.mode.bandwidth() != Hz500
            && let Some(link) = &self.session
            && link.caller == self.local
        {
            let query = if self.send_index.is_multiple_of(2) {
                Message::Bw500FinalQuery
            } else {
                Message::Bw500AlternateQuery
            };
            cable
                .send(query.synthesize_on(&link.called, self.mode.bandwidth()))
                .await?;
            self.transition(Phase::IntermediateQueryLong);
            return Ok(());
        }
        if matches!(self.phase, Phase::DataAck | Phase::FinalQueryShort)
            && self.is_final_caller_frame()
        {
            let link = self.session.as_ref().expect("query requires a session");
            let (query, next) = if self.phase == Phase::DataAck {
                (Message::Bw500FinalQueryShort, Phase::FinalQueryShort)
            } else {
                (Message::Bw500FinalQuery, Phase::FinalQueryLong)
            };
            cable.send(query.synthesize(&link.called)).await?;
            self.phase = next;
            self.deadline = Some(Instant::now() + FINAL_QUERY_DELAY);
            return Ok(());
        }
        if self.intermediate_transmissions != 0
            && matches!(self.phase, Phase::DataAck | Phase::IntermediateQueryShort)
        {
            let link = self.session.as_ref().expect("query requires a session");
            let short = self.phase == Phase::DataAck;
            let alternate = self.intermediate_transmissions.is_multiple_of(2);
            let query = match (short, alternate) {
                (true, false) => Message::Bw500FinalQueryShort,
                (true, true) => Message::Bw500AlternateQueryShort,
                (false, false) => Message::Bw500FinalQuery,
                (false, true) => Message::Bw500AlternateQuery,
            };
            cable.send(query.synthesize(&link.called)).await?;
            self.phase = if short {
                Phase::IntermediateQueryShort
            } else {
                Phase::IntermediateQueryLong
            };
            self.deadline = Some(Instant::now() + FINAL_QUERY_DELAY);
            return Ok(());
        }
        if !self.pending.is_empty() {
            // Do not retire bytes or repeat DATA on a missing answer.
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                if self.phase == Phase::TurnGrant {
                    "peer did not grant the transmit turn"
                } else {
                    "virtual DATA delivery was not acknowledged"
                },
            ));
        }
        self.reset()?;
        self.notify("DISCONNECTED\r".into())
    }

    async fn audio(&mut self, samples: &[f32], cable: &mut Cable) -> io::Result<()> {
        let mut detector = BurstDetector::new(48_000);
        let mut candidates = Vec::new();
        detector.push(samples, |range| candidates.push(range));
        detector.finish(|range| candidates.push(range));
        for range in candidates {
            let start = range.start as usize;
            if self.phase == Phase::Handshake {
                let action = self.handshake.receive(
                    self.start.elapsed(),
                    samples,
                    start..range.end as usize,
                );
                if action.is_some() {
                    self.establish(action, cable).await?;
                    return Ok(());
                }
                continue;
            }
            let link = self.session.as_ref().unwrap();
            let local_caller = link.caller == self.local;
            let bandwidth = self.mode.bandwidth();
            let ack = || {
                if local_caller {
                    control::find_connected_ack(bandwidth, &link.caller, samples, start)
                } else {
                    control::find_caller_data_ack(bandwidth, &link.caller, samples, start)
                }
                .is_some()
            };
            let matches = |message: Message, key: &str| {
                message.find_on(key, samples, start, bandwidth).is_some()
            };
            if matches(
                Message::SessionDisconnect {
                    responder: local_caller,
                },
                &link.called,
            ) || !local_caller && matches(Message::SessionDisconnectShort, &link.called)
            {
                self.reset()?;
                return self.notify("DISCONNECTED\r".into());
            }
            let (peer_release, own_release) = if local_caller {
                (
                    Message::Bw500ResponderTurnRelease,
                    Message::Bw500CallerTurnRelease,
                )
            } else {
                (
                    Message::Bw500CallerTurnRelease,
                    Message::Bw500ResponderTurnRelease,
                )
            };
            match self.phase {
                Phase::ConfirmAck if ack() => {
                    self.phase = Phase::CallerReady;
                    self.deadline = None;
                    self.connected()?;
                }
                Phase::ConfirmAck if bandwidth == Hz500 => {
                    // A stock responder with queued host data can send its
                    // first DATA over immediately after the confirmation.
                    let Some(reply) = self.receive_data(samples, start..range.end as usize)? else {
                        continue;
                    };
                    cable.write(&reply).await?;
                }
                Phase::PollAck
                    if control::find_poll_ack(bandwidth, &link.caller, samples, start)
                        .is_some() =>
                {
                    cable
                        .send(
                            Message::Bw500CallerTurnRequest.synthesize_on(&link.caller, bandwidth),
                        )
                        .await?;
                    self.transition(Phase::TurnGrant);
                }
                Phase::TurnGrant => {
                    let responder_requested =
                        local_caller && matches(Message::Bw500ResponderTurnRequest, &link.caller);
                    let caller_requested = bandwidth == Hz500
                        && !local_caller
                        && (matches(Message::Bw500CallerTurnRequest, &link.caller)
                            || matches(Message::Bw500CallerTurnRequestShort, &link.caller));
                    // Give the responder the first opportunity when requests cross.
                    // A stock caller can repeat its request instead of granting it.
                    if responder_requested || caller_requested && self.caller_requested_turn {
                        cable
                            .send(control::turn_grant(bandwidth, &link.called, !local_caller))
                            .await?;
                        self.receive_bytes.clear();
                        self.last_received = None;
                        self.rejected_over = false;
                        self.transition(Phase::Receiving);
                        return Ok(());
                    }
                    if caller_requested {
                        self.caller_requested_turn = true;
                        self.deadline = Some(Instant::now() + PHASE_TIMEOUT);
                    }
                    let grant = control::find_turn_grant(
                        bandwidth,
                        &link.called,
                        local_caller,
                        samples,
                        start,
                    );
                    let drained = if local_caller {
                        Message::Bw500ResponderDrained
                    } else {
                        Message::Bw500Drained
                    };
                    let handed_over =
                        matches(peer_release, &link.called) || matches(drained, &link.called);
                    if grant.is_none() && !handed_over {
                        continue;
                    }
                    self.send_over(cable).await?;
                }
                Phase::DataAck | Phase::IntermediateQueryLong if bandwidth != Hz500 => {
                    let wide_reply = |reply: Bw500IntermediateReply| {
                        reply
                            .find_on(&link.called, local_caller, samples, start, bandwidth)
                            .is_some()
                    };
                    let queried = self.phase == Phase::IntermediateQueryLong;
                    let missing = queried && matches(Message::QueryMissingData, &link.called);
                    let nak = missing
                        || wide_reply(Bw500IntermediateReply::NotDelivered)
                        || matches(Message::WideSetupNak(bandwidth), &link.called);
                    let full = self.send_index < self.delivery_len / self.wide_block_bytes();
                    let decrease = !nak && full && wide_reply(Bw500IntermediateReply::DecreaseOne);
                    let increase = !nak
                        && full
                        && self.wide_rate == 2
                        && wide_reply(Bw500IntermediateReply::IncreaseTwo);
                    let next_rate = if increase {
                        4
                    } else if missing {
                        2
                    } else {
                        3
                    };
                    if ((nak || decrease) && self.wide_rate == 4) || increase {
                        let acknowledged = (self.send_index + usize::from(decrease || increase))
                            * self.wide_block_bytes();
                        let remaining = self.delivery_len - acknowledged;
                        let fits = match next_rate {
                            2 => {
                                data::wide_lower_fits(remaining, data::WIDE_ROBUST_FULL_BYTES)
                                    || remaining
                                        >= data::WIDE_ROBUST_FULL_BYTES + data::WIDE_FULL_BYTES
                            }
                            3 => data::wide_lower_fits(remaining, data::WIDE_LOWER_FULL_BYTES),
                            _ => data::wide_delivery_prefix(remaining) == remaining,
                        };
                        if fits {
                            self.pending.drain(..acknowledged);
                            self.delivery_len = remaining;
                            self.send_index = 0;
                            self.wide_rate = next_rate;
                            self.retried_data = nak;
                            if decrease || increase {
                                self.report_buffer()?;
                            }
                            self.send_over(cable).await?;
                            return Ok(());
                        }
                    }
                    if nak {
                        if self.retried_data {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "peer rejected the wide DATA retry",
                            ));
                        }
                        self.retried_data = true;
                        cable.write(&self.wide_outstanding).await?;
                        self.transition(Phase::DataAck);
                        return Ok(());
                    }
                    if full {
                        if !(wide_reply(Bw500IntermediateReply::SameLevel)
                            || queried && matches(Message::Bw500IntermediateQueryAck, &link.called))
                        {
                            continue;
                        }
                        self.send_index += 1;
                        self.retried_data = false;
                        self.report_buffer()?;
                        self.send_over(cable).await?;
                        return Ok(());
                    }
                    if !ack() {
                        continue;
                    }
                    self.acknowledge_delivery(cable).await?;
                }
                Phase::DataAck | Phase::IntermediateQueryShort | Phase::IntermediateQueryLong => {
                    let querying = self.phase != Phase::DataAck;
                    if self.retry_on_confirm && matches(Message::Bw500Confirm, &link.called) {
                        self.retry_on_confirm = false;
                        self.send_over(cable).await?;
                        return Ok(());
                    }
                    let plan = self.plan.as_ref().expect("transmitting requires a plan");
                    let matches = |reply: Bw500IntermediateReply| {
                        reply
                            .find(&link.called, local_caller, samples, start)
                            .is_some()
                    };
                    let over_nak = querying
                        && Message::Bw500ResponderOverNak
                            .find(&link.called, samples, start)
                            .is_some();
                    if matches(Bw500IntermediateReply::NotDelivered)
                        || over_nak
                        || !local_caller
                            && Message::Bw500CallerDataNak
                                .find(&link.called, samples, start)
                                .is_some()
                        || querying
                            && local_caller
                            && [Message::QueryMissingData, Message::Bw500DataNakQuery]
                                .into_iter()
                                .any(|message| message.find(&link.called, samples, start).is_some())
                        || local_caller
                            && Message::Bw500SetupNak
                                .find(&link.called, samples, start)
                                .is_some()
                    {
                        if !self.retried_data {
                            self.retried_data = true;
                            if !over_nak
                                && self.send_index == 0
                                && let Some(retry) =
                                    data::TransmitPlan::level3_retry(self.delivery_len)
                            {
                                self.plan = Some(retry);
                            } else if local_caller
                                && !over_nak
                                && let Some((acknowledged, retry)) = plan
                                    .level3_retry_after_prefix(self.send_index, self.delivery_len)
                            {
                                self.pending.drain(..acknowledged);
                                self.delivery_len -= acknowledged;
                                self.send_index = 0;
                                self.plan = Some(retry);
                            }
                            // Once a prefix is acknowledged, retry only the
                            // rejected frame, preserving the acknowledged bytes.
                            self.send_over(cable).await?;
                            return Ok(());
                        }
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "peer rejected DATA; no qualified retry layout remains",
                        ));
                    }
                    if self.send_index == plan.full_frames() {
                        if !ack() {
                            continue;
                        }
                        self.retry_on_confirm = false;
                        self.acknowledge_delivery(cable).await?;
                    } else {
                        if local_caller
                            && let Some((acknowledged, continuation)) =
                                plan.level4_retry_continuation(self.send_index, self.delivery_len)
                            && matches(Bw500IntermediateReply::IncreaseOne)
                        {
                            self.pending.drain(..acknowledged);
                            self.delivery_len -= acknowledged;
                            self.send_index = 0;
                            self.plan = Some(continuation);
                            self.retry_on_confirm = false;
                            self.retried_data = false;
                            self.report_buffer()?;
                            self.send_over(cable).await?;
                            return Ok(());
                        }
                        let last_full = self.send_index + 1 == plan.full_frames();
                        let query_ack = querying
                            && Message::Bw500IntermediateQueryAck
                                .find(&link.called, samples, start)
                                .is_some();
                        if !(matches(Bw500IntermediateReply::SameLevel)
                            || query_ack
                            || last_full && matches(Bw500IntermediateReply::DecreaseOne))
                        {
                            continue;
                        }
                        self.retry_on_confirm = false;
                        self.send_index += 1;
                        self.retried_data = false;
                        self.report_buffer()?;
                        self.send_over(cable).await?;
                    }
                }
                Phase::FinalQueryShort | Phase::FinalQueryLong => {
                    if ack() {
                        self.acknowledge_delivery(cable).await?;
                    } else if !self.retried_data
                        && (matches(Message::QueryMissingData, &link.called)
                            || matches(Message::Bw500DataNakQuery, &link.called))
                    {
                        let retry = if self.send_index == 0 {
                            data::TransmitPlan::level3_retry(self.delivery_len)
                                .map(|plan| (0, plan))
                        } else {
                            self.plan.as_ref().and_then(|plan| {
                                plan.level3_retry_after_prefix(self.send_index, self.delivery_len)
                            })
                        };
                        if let Some((acknowledged, plan)) = retry {
                            self.retried_data = true;
                            self.pending.drain(..acknowledged);
                            self.delivery_len -= acknowledged;
                            self.send_index = 0;
                            self.plan = Some(plan);
                            self.send_over(cable).await?;
                        }
                    } else if !self.retried_data
                        && matches(Message::Bw500ResponderOverNak, &link.called)
                        && let Some(retry) = data::TransmitPlan::level3_retry(self.delivery_len)
                    {
                        self.retried_data = true;
                        self.plan = Some(retry);
                        self.send_index = 0;
                        self.send_over(cable).await?;
                    } else if self.pending.len() == self.delivery_len
                        && matches(Message::Bw500ResponderTurnRequest, &link.caller)
                    {
                        cable
                            .send(Message::Bw500Drained.synthesize(&link.called))
                            .await?;
                        self.acknowledge_delivery(cable).await?;
                        self.await_handed_back_data();
                    } else {
                        continue;
                    }
                }
                Phase::Received
                    if bandwidth == Hz500
                        && !local_caller
                        && (matches(Message::Bw500FinalQueryShort, &link.called)
                            || matches(Message::Bw500FinalQuery, &link.called)) =>
                {
                    cable
                        .send(control::synthesize_connected_ack(bandwidth, &link.caller))
                        .await?;
                }
                Phase::Responder
                    if matches(Message::Bw500KeepaliveA, &link.called)
                        || matches(Message::Bw500KeepaliveB, &link.called) =>
                {
                    cable
                        .send(control::poll_ack(bandwidth, &link.caller))
                        .await?;
                }
                Phase::Received | Phase::Collecting
                    if !self.returned_turn
                        && self.last_received.is_some()
                        && (matches(peer_release, &link.called)
                            || !local_caller && matches(Message::Bw500Drained, &link.called)) =>
                {
                    self.returned_turn = true;
                    if self.pending.is_empty() {
                        let reply = if local_caller {
                            own_release.synthesize_on(&link.called, bandwidth)
                        } else {
                            control::poll_ack(bandwidth, &link.caller)
                        };
                        cable.send(reply).await?;
                    } else {
                        self.plan_delivery()?;
                        self.send_over(cable).await?;
                    }
                }
                Phase::CallerReady
                | Phase::Responder
                | Phase::Sent
                | Phase::TurnReleased
                | Phase::Received => {
                    let request = if local_caller {
                        Message::Bw500ResponderTurnRequest
                    } else {
                        Message::Bw500CallerTurnRequest
                    };
                    let short_request = bandwidth == Hz500
                        && !local_caller
                        && matches(Message::Bw500CallerTurnRequestShort, &link.caller);
                    if !matches(request, &link.caller) && !short_request {
                        if matches!(self.phase, Phase::Received | Phase::TurnReleased)
                            && let Some(reply) =
                                self.receive_data(samples, start..range.end as usize)?
                        {
                            cable.write(&reply).await?;
                        }
                        continue;
                    }
                    let grant = control::turn_grant(bandwidth, &link.called, !local_caller);
                    cable.send(grant).await?;
                    self.receive_bytes.clear();
                    self.last_received = None;
                    self.rejected_over = false;
                    self.transition(Phase::Receiving);
                }
                Phase::Receiving => {
                    if bandwidth == Hz500
                        && !local_caller
                        && self.last_received.is_none()
                        && !self.pending.is_empty()
                        && matches(Message::Bw500CallerTurnIdle, &link.caller)
                    {
                        self.start_delivery(cable).await?;
                        return Ok(());
                    }
                    if bandwidth == Hz500
                        && !local_caller
                        && self.last_received.is_none()
                        && !self.pending.is_empty()
                        && (matches(peer_release, &link.called)
                            || matches(Message::Bw500Drained, &link.called))
                    {
                        self.plan_delivery()?;
                        self.send_over(cable).await?;
                        return Ok(());
                    }
                    if bandwidth != Hz500
                        && !local_caller
                        && (matches(Message::Bw500FinalQuery, &link.called)
                            || matches(Message::Bw500AlternateQuery, &link.called))
                        && (self.rejected_over || self.last_received.is_some())
                    {
                        let reply = if self.rejected_over {
                            Message::QueryMissingData
                        } else {
                            Message::Bw500IntermediateQueryAck
                        };
                        cable
                            .send(reply.synthesize_on(&link.called, bandwidth))
                            .await?;
                        self.transition(Phase::Receiving);
                        return Ok(());
                    }
                    if bandwidth == Hz500
                        && !local_caller
                        && self.last_received.is_some()
                        && (matches(Message::Bw500FinalQueryShort, &link.called)
                            || matches(Message::Bw500FinalQuery, &link.called))
                    {
                        cable
                            .send(Message::Bw500IntermediateQueryAck.synthesize(&link.called))
                            .await?;
                        self.transition(Phase::Receiving);
                        return Ok(());
                    }
                    if bandwidth == Hz500
                        && local_caller
                        && self.last_received.is_none()
                        && !self.pending.is_empty()
                        && matches(Message::Bw500ResponderTurnIdle, &link.caller)
                    {
                        self.start_delivery(cable).await?;
                        return Ok(());
                    }
                    let request = if local_caller {
                        Message::Bw500ResponderTurnRequest
                    } else {
                        Message::Bw500CallerTurnRequest
                    };
                    let short_request = bandwidth == Hz500
                        && !local_caller
                        && matches(Message::Bw500CallerTurnRequestShort, &link.caller);
                    if self.last_received.is_none()
                        && (matches(request, &link.caller) || short_request)
                    {
                        cable
                            .send(control::turn_grant(bandwidth, &link.called, !local_caller))
                            .await?;
                        return Ok(());
                    }
                    let Some(reply) = self.receive_data(samples, start..range.end as usize)? else {
                        continue;
                    };
                    cable.write(&reply).await?;
                }
                _ => continue,
            }
            return Ok(());
        }
        Ok(())
    }
    async fn send_over(&mut self, cable: &mut Cable) -> io::Result<()> {
        let link = self
            .session
            .as_ref()
            .expect("transmitting requires a session");
        if let Mode::Wide { waveform, .. } = self.mode {
            let lead = self
                .wide_lead
                .as_mut()
                .expect("wide link has a lead stream");
            let payload = &self.pending[..self.delivery_len];
            self.wide_outstanding = match self.wide_rate {
                2 => data::wide_lower_delivery_frame(self.send_index, payload, &link.caller)
                    .and_then(|frame| self.wide_level2.as_ref()?.synthesize(&frame, lead)),
                3 => data::wide_lower_delivery_frame(self.send_index, payload, &link.caller)
                    .and_then(|frame| self.wide_level3.as_ref()?.synthesize(&frame, lead)),
                _ => data::wide_delivery_frame(self.send_index, payload, &link.caller)
                    .and_then(|frame| waveform.synthesize(&frame, lead)),
            }
            .ok_or_else(|| io::Error::other("could not render wide DATA"))?;
            cable.write(&self.wide_outstanding).await?;
            self.transition(Phase::DataAck);
            return Ok(());
        }
        let Mode::Bw500 { waveform, .. } = self.mode else {
            unreachable!()
        };
        let over = self.plan.as_ref().and_then(|plan| {
            plan.encode(
                self.send_index,
                &self.pending[..self.delivery_len],
                &link.caller,
            )
        });
        let (level, wire) =
            over.ok_or_else(|| io::Error::other("could not encode planned BW500 DATA"))?;
        let samples = if level == 4 {
            wire.try_into()
                .ok()
                .and_then(|frame| waveform.synthesize(&frame))
        } else {
            bw500::synthesize(
                bw500::Level::Three,
                &[bw500::Frame {
                    payload: wire[..34].to_vec(),
                    control: wire[34],
                }],
            )
        };
        let samples = samples.ok_or_else(|| io::Error::other("could not render DATA waveform"))?;
        let playback = Duration::from_secs_f64(samples.len() as f64 / 48_000.0);
        let local_caller = link.caller == self.local;
        cable.write(&samples).await?;
        self.transition(Phase::DataAck);
        if local_caller {
            if !self.is_final_caller_frame() {
                self.intermediate_transmissions += 1;
            }
            self.deadline = Some(Instant::now() + playback + FINAL_ACK_WAIT);
        }
        Ok(())
    }

    fn receive_data(
        &mut self,
        samples: &[f32],
        candidate: std::ops::Range<usize>,
    ) -> io::Result<Option<Vec<f32>>> {
        let link = self.session.as_ref().expect("receiving requires a session");
        if candidate.len() < 100_000 {
            return Ok(None);
        }
        let bandwidth = self.mode.bandwidth();
        let decoded = match self.mode {
            Mode::Wide { waveform, .. } => {
                if let Some(frame) = waveform.decode(samples, candidate.clone()) {
                    (
                        4,
                        bw500::Frame {
                            payload: frame[..data::WIDE_FULL_BYTES].to_vec(),
                            control: frame[data::WIDE_FULL_BYTES],
                        },
                    )
                } else if let Some(frame) = self
                    .wide_level3
                    .as_ref()
                    .and_then(|codec| codec.decode(samples, candidate.clone()))
                {
                    (
                        3,
                        bw500::Frame {
                            payload: frame[..data::WIDE_LOWER_FULL_BYTES].to_vec(),
                            control: frame[data::WIDE_LOWER_FULL_BYTES],
                        },
                    )
                } else if let Some(frame) = self
                    .wide_level2
                    .as_ref()
                    .and_then(|codec| codec.decode(samples, candidate))
                {
                    (
                        2,
                        bw500::Frame {
                            payload: frame[..data::WIDE_ROBUST_FULL_BYTES].to_vec(),
                            control: frame[data::WIDE_ROBUST_FULL_BYTES],
                        },
                    )
                } else {
                    if self.phase == Phase::Receiving
                        && !self.rejected_over
                        && link.called == self.local
                    {
                        let reply = Message::WideSetupNak(bandwidth).synthesize(&link.called);
                        self.rejected_over = true;
                        self.transition(Phase::Receiving);
                        return Ok(reply);
                    }
                    return Ok(None);
                }
            }
            Mode::Bw500 { waveform, .. } => {
                if let Some(frame) = waveform.decode(samples, candidate.clone()) {
                    (
                        4,
                        bw500::Frame {
                            payload: frame.frame[..43].to_vec(),
                            control: frame.frame[43],
                        },
                    )
                } else {
                    let mut decoded = bw500::decode_burst(samples, candidate);
                    if decoded.len() > 1 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "multiple DATA frames in one over are unsupported",
                        ));
                    }
                    let Some(frame) = decoded.pop() else {
                        if self.phase == Phase::Receiving && !self.rejected_over {
                            let reply = Bw500IntermediateReply::NotDelivered
                                .synthesize(&link.called, link.called == self.local);
                            self.rejected_over = true;
                            self.transition(Phase::Receiving);
                            return Ok(reply);
                        }
                        return Ok(None);
                    };
                    (frame.level.number(), frame.frame)
                }
            }
        };
        let responder_sends = link.called == self.local;
        let closing_ack = || {
            if responder_sends {
                control::synthesize_connected_ack(bandwidth, &link.caller)
            } else {
                control::caller_data_ack(bandwidth, &link.caller)
            }
        };
        if self.phase == Phase::Received {
            if self.last_received.as_ref() == Some(&decoded) {
                return Ok(closing_ack());
            }
            if !self.returned_turn {
                return Ok(None);
            }
        }
        let frame = if bandwidth == Hz500 {
            data::decode_received(decoded.0, &decoded.1, &link.caller)
        } else {
            data::decode_wide_received(&decoded.1.payload, decoded.1.control, &link.caller)
        };
        let Some(frame) = frame else {
            return Ok(None);
        };
        if self.last_received.as_ref() == Some(&decoded) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ambiguous intermediate DATA replay",
            ));
        }
        if bandwidth == Hz500
            && self.phase == Phase::Receiving
            && responder_sends
            && decoded.0 == 3
            && frame.field == 0x95
            && let Some((4, previous)) = &self.last_received
            && matches!(previous.control, 0x8d | 0x91)
            && previous.payload.starts_with(&frame.payload)
        {
            // Stock can miss the same-rate ACK and replace this frame with
            // the start of a level-3 continuation. Only the closing frame
            // commits the buffered bytes to the host.
            self.receive_bytes
                .truncate(self.receive_bytes.len() - previous.payload.len());
        }
        if frame.payload.len() > MAX_RECEIVE - self.receive_bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "virtual modem delivery exceeds receive limit",
            ));
        }
        if self.phase == Phase::ConfirmAck {
            if frame.next_receive_level.is_none()
                && (decoded.0 != 4 || !matches!(frame.field, 0x41 | 0x81 | 0x8d | 0x91 | 0x95))
            {
                return Ok(None);
            }
            self.connected()?;
        }
        self.receive_bytes.extend(frame.payload);
        self.rejected_over = false;
        let decoded_level = decoded.0;
        self.last_received = Some(decoded);
        if frame.next_receive_level.is_none() {
            let reply = if bandwidth != Hz500 && decoded_level == 2 && frame.field == 0x95 {
                Bw500IntermediateReply::IncreaseTwo
            } else if bandwidth == Hz500
                && !responder_sends
                && decoded_level == 4
                && frame.field == 0x81
            {
                Bw500IntermediateReply::DecreaseOne
            } else {
                Bw500IntermediateReply::SameLevel
            }
            .synthesize_on(&link.called, responder_sends, bandwidth);
            self.transition(Phase::Receiving);
            return Ok(reply);
        }

        if !self.receive_bytes.is_empty() {
            self.received
                .try_send(std::mem::take(&mut self.receive_bytes))
                .map_err(io::Error::other)?;
        }
        self.closed_at_level3 = false;
        self.phase = Phase::Received;
        self.returned_turn = false;
        self.deadline = (!self.pending.is_empty()).then(|| Instant::now() + HOST_BATCH_WINDOW);
        Ok(closing_ack())
    }
}

/// TCP records are a big-endian sample count followed by little-endian float
/// samples. Partial records survive a cancelled read in the input buffer.
struct Cable {
    stream: TcpStream,
    input: Vec<u8>,
}

impl Cable {
    async fn read(&mut self) -> io::Result<Vec<f32>> {
        loop {
            if self.input.len() >= 4 {
                let count = u32::from_be_bytes(self.input[..4].try_into().unwrap()) as usize;
                if !(1..=MAX_AUDIO).contains(&count) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid audio sample count",
                    ));
                }
                let end = 4 + count * 4;
                if self.input.len() >= end {
                    let samples: Vec<_> = self.input[4..end]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .copied()
                        .map(f32::from_le_bytes)
                        .collect();
                    self.input.drain(..end);
                    if samples.iter().any(|sample| !(-1.0..=1.0).contains(sample)) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid PCM sample",
                        ));
                    }
                    return Ok(samples);
                }
            }
            let mut chunk = [0; 8192];
            let count = self.stream.read(&mut chunk).await?;
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "virtual cable closed",
                ));
            }
            self.input.extend_from_slice(&chunk[..count]);
        }
    }

    async fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        let mut frame = Vec::with_capacity(4 + samples.len() * 4);
        frame.extend_from_slice(&(samples.len() as u32).to_be_bytes());
        for sample in samples {
            frame.extend_from_slice(&sample.to_le_bytes());
        }
        timeout(IO_TIMEOUT, self.stream.write_all(&frame)).await??;
        Ok(())
    }

    async fn send(&mut self, samples: Option<Vec<f32>>) -> io::Result<()> {
        self.write(&samples.ok_or_else(|| io::Error::other("could not render waveform"))?)
            .await
    }
}

async fn run(
    mut engine: Engine<'_>,
    mut cable: Cable,
    mut host: mpsc::Receiver<Request>,
) -> io::Result<()> {
    loop {
        let expiry = async {
            match engine.deadline {
                Some(deadline) => tokio::time::sleep_until(deadline).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            request = host.recv() => match request {
                Some(Request::Command(command, reply)) => { let _ = reply.send(engine.host_command(command, &mut cable).await?); }
                Some(Request::Data(bytes, reply)) => { let _ = reply.send(engine.host_data(&bytes)); }
                None => return Ok(()),
            },
            audio = cable.read() => engine.audio(&audio?, &mut cable).await?,
            _ = expiry => engine.expire(&mut cable).await?,

        }
    }
}

pub async fn serve(args: &[String], wide: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (call, profile, ports) = match args {
        [call, profile, ports @ ..] => (call, profile, ports),
        _ => return Err("expected callsign and BW500 profile path or wide bandwidth".into()),
    };
    let (control, data, audio) = match ports {
        [] => (8300, 8301, 8302),
        [control, data, audio] => (control.parse()?, data.parse()?, audio.parse()?),
        _ => return Err("expected all three optional ports".into()),
    };
    let mode = if wide {
        let bandwidth = match profile.as_str() {
            "2300" => Bandwidth::Hz2300,
            "2750" => Bandwidth::Hz2750,
            _ => return Err("wide bandwidth must be 2300 or 2750".into()),
        };
        Mode::Wide {
            bandwidth,
            waveform: Box::new(wide_level4::Waveform::new(bandwidth).unwrap()),
        }
    } else {
        let profile = std::fs::read_to_string(profile)?;
        let waveform = Box::new(Waveform::from_json(&profile)?);
        Mode::Bw500 { profile, waveform }
    };
    let identity = LinkSetup {
        caller: call.clone(),
        high_speed: false,
        requested_level: 4,
    };
    let wire = identity.encode(Profile::Bw500).ok_or("invalid callsign")?;
    let local = LinkSetup::decode(Profile::Bw500, &wire)
        .ok_or("invalid callsign")?
        .caller;
    let address = |port| SocketAddr::from(([127, 0, 0, 1], port));
    let mut listener = Listener::bind(address(control), address(data), IO_TIMEOUT).await?;
    let audio = TcpListener::bind(address(audio)).await?;
    let (control, data) = listener.addresses()?;
    println!(
        "VIRTUAL BW{} {local}. Command {control}, DATA {data}, audio {}.",
        mode.width(),
        audio.local_addr()?
    );
    loop {
        let (stream, _) = audio.accept().await?;
        let (send, host) = mpsc::channel(8);
        let (notices, control_rx) = mpsc::channel(8);
        let (received, data_rx) = mpsc::channel(1);
        let port = Port(send);
        let engine = Engine {
            local: &local,
            mode: &mode,
            wide_lead: None,
            wide_level3: wide_level4::LowerWaveform::new_level3(mode.bandwidth()),
            wide_level2: wide_level4::RobustWaveform::new_level2(mode.bandwidth()),
            wide_rate: 4,
            wide_outstanding: Vec::new(),
            handshake: mode.handshake(&local, false)?,
            session: None,
            phase: Phase::Handshake,
            listen: false,
            disconnect_pending: false,
            pending: Vec::new(),
            plan: None,
            delivery_len: 0,
            send_index: 0,
            intermediate_transmissions: 0,
            retried_data: false,
            retry_on_confirm: false,
            returned_turn: false,
            caller_requested_turn: false,
            initial_poll: false,
            closed_at_level3: false,
            rejected_over: false,
            receive_bytes: Vec::new(),
            last_received: None,
            deadline: None,
            start: Instant::now(),
            notices,
            received,
        };
        let result = tokio::select! {
            result = listener.serve_one(&port, control_rx, data_rx) => result,
            result = run(engine, Cable { stream, input: Vec::new() }, host) => result,
        };
        if let Err(error) = result {
            eprintln!("Virtual BW{} connection ended: {error}", mode.width());
        }
    }
}

/// Bridge the virtual socket to explicitly selected, isolated audio devices.
#[cfg(feature = "audio-device")]
pub async fn bridge(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use linkwise::audio::{
        device::Capture,
        playback::{Playback, Progress},
    };
    use std::collections::VecDeque;

    let [port, input, output] = args else {
        return Err("usage: --virtual-audio-device AUDIO_PORT INPUT_DEVICE OUTPUT_DEVICE".into());
    };
    let port: u16 = port.parse()?;
    let stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    let mut cable = Cable {
        stream,
        input: Vec::new(),
    };
    let capture = Capture::start(Some(input), 0)?;
    let mut playback: Option<Playback> = None;
    let mut playback_until = None;
    let mut detector = BurstDetector::new(48_000);
    let mut history = VecDeque::<f32>::new();
    let mut position = 0_u64;
    let mut completed = VecDeque::new();
    let mut clock = tokio::time::interval(Duration::from_millis(5));
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    const EDGE: usize = 2400;

    loop {
        tokio::select! {
            incoming = cable.read(), if playback.is_none() => {
                // Pad output with silence so acquisition and release are
                // observable even on a loopback device with no idle stream.
                let incoming = incoming?;
                let mut audio = Vec::with_capacity(incoming.len() + 2 * EDGE);
                audio.resize(EDGE, 0.0);
                audio.extend(incoming);
                audio.resize(audio.len() + EDGE, 0.0);
                let minimum = Duration::from_secs_f64(audio.len() as f64 / 48_000.0);
                playback = Some(Playback::start(output, 0, audio)?);
                playback_until = Some(Instant::now() + minimum + Duration::from_millis(100));
            }
            _ = clock.tick() => {
                if let (Some(active), Some(until)) = (&mut playback, playback_until)
                    && active.progress()? == Progress::EstimateElapsed
                    && Instant::now() >= until
                {
                    playback = None;
                    playback_until = None;
                }
                while let Some(block) = capture.read(Duration::ZERO)? {
                    let discard = (history.len() + block.samples.len()).saturating_sub(MAX_AUDIO + 2 * EDGE);
                    history.drain(..discard);
                    history.extend(block.samples);
                    position += block.samples.len() as u64;
                    let base = position - history.len() as u64;
                    detector.push(&block.samples, |candidate| completed.push_back(candidate));
                    while completed.front().is_some_and(|candidate| candidate.end + EDGE as u64 <= position) {
                        let candidate = completed.pop_front().expect("checked capture queue");
                        if candidate.start < base || candidate.end - candidate.start > MAX_AUDIO as u64 {
                            return Err("native virtual audio burst exceeded its capture window".into());
                        }
                        let start = candidate.start.saturating_sub(EDGE as u64).max(base);
                        let end = candidate.end + EDGE as u64;
                        let audio = history.range((start - base) as usize..(end - base) as usize)
                            .copied().collect::<Vec<_>>();
                        if audio.len() > MAX_AUDIO {
                            return Err("native virtual audio burst exceeded its cable record limit".into());
                        }
                        cable.write(&audio).await?;
                    }
                }
            }
        }
    }
}
