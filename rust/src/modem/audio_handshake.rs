//! Audio codec boundary for BW500 link establishment.
//!
//! Transmit completion must be supplied after actual playback and PTT release;
//! returning audio does not advance the protocol. The device/session owner
//! schedules turnaround and maintains exclusive ownership of the audio route.

use super::{
    bw500_waveform::Waveform,
    control::{self, Bandwidth, Message},
    handshake::{self, Burst, Event, Expected, Handshake},
    setup::{LinkSetup, Profile},
};
use std::{ops::Range, time::Duration};

pub enum Action {
    Transmit { token: u64, audio: Vec<f32> },
    Established(handshake::Established),
    Failed(handshake::Failure),
}

pub struct AudioHandshake {
    protocol: Handshake,
    waveform: Waveform,
}

impl AudioHandshake {
    pub fn new(
        local: LinkSetup,
        listen: bool,
        timeout: Duration,
        waveform: Waveform,
    ) -> Option<Self> {
        Some(Self {
            protocol: Handshake::new(local, listen, timeout)?,
            waveform,
        })
    }

    pub fn deadline(&self) -> Option<Duration> {
        self.protocol.deadline()
    }

    pub fn handle(&mut self, now: Duration, event: Event) -> Option<Action> {
        let action = self.protocol.handle(now, event)?;
        self.encode(now, action)
    }

    /// Decode only the burst expected by this phase, using the known peer key.
    /// A setup identity is accepted only after waveform FEC and frame CRC pass.
    pub fn receive(
        &mut self,
        now: Duration,
        audio: &[f32],
        candidate: Range<usize>,
    ) -> Option<Action> {
        let burst = decode_burst(&self.waveform, self.protocol.expected()?, audio, candidate)?;
        self.handle(now, Event::Received(burst))
    }

    fn encode(&mut self, now: Duration, action: handshake::Action) -> Option<Action> {
        Some(match action {
            handshake::Action::Established(session) => Action::Established(session),
            handshake::Action::Failed(failure) => Action::Failed(failure),
            handshake::Action::Transmit { token, burst } => {
                let audio = encode_burst(&self.waveform, &burst);
                match audio {
                    Some(audio) => Action::Transmit { token, audio },
                    None => {
                        return self.handle(
                            now,
                            Event::Transmitted {
                                token,
                                success: false,
                            },
                        );
                    }
                }
            }
        })
    }
}

pub(crate) fn decode_burst(
    waveform: &Waveform,
    expected: Expected<'_>,
    audio: &[f32],
    candidate: Range<usize>,
) -> Option<Burst> {
    if candidate.start >= candidate.end
        || candidate.end > audio.len()
        || audio.iter().any(|sample| !sample.is_finite())
    {
        return None;
    }
    Some(match expected {
        Expected::Request(called) => {
            Message::ConnectRequest(Bandwidth::Hz500).find(called, audio, candidate.start)?;
            Burst::Request {
                called: called.into(),
            }
        }
        Expected::Response(called) => {
            Message::ConnectResponse(Bandwidth::Hz500).find(called, audio, candidate.start)?;
            Burst::Response {
                called: called.into(),
            }
        }
        Expected::Setup => {
            let decoded = waveform.decode(audio, candidate)?;
            Burst::Setup(LinkSetup::decode(Profile::Bw500, &decoded.frame)?)
        }
        Expected::ConnectedAck(caller) => {
            control::find_connected_ack(Bandwidth::Hz500, caller, audio, candidate.start)?;
            Burst::ConnectedAck {
                caller: caller.into(),
            }
        }
        Expected::Confirm(called) => {
            Message::Bw500Confirm.find(called, audio, candidate.start)?;
            Burst::Confirm {
                called: called.into(),
                level: 4,
            }
        }
    })
}

pub(crate) fn encode_burst(waveform: &Waveform, burst: &Burst) -> Option<Vec<f32>> {
    match burst {
        Burst::Request { called } => Message::ConnectRequest(Bandwidth::Hz500).synthesize(called),
        Burst::Response { called } => Message::ConnectResponse(Bandwidth::Hz500).synthesize(called),
        Burst::Setup(setup) => setup
            .encode(Profile::Bw500)
            .and_then(|frame| frame.try_into().ok())
            .and_then(|frame| waveform.synthesize(&frame)),
        Burst::ConnectedAck { caller } => {
            control::synthesize_connected_ack(Bandwidth::Hz500, caller)
        }
        Burst::Confirm { called, level: 4 } => Message::Bw500Confirm.synthesize(called),
        _ => None,
    }
}
