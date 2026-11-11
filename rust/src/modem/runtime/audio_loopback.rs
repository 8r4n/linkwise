//! A two-station bench using real BW500 audio codecs and local test framing.
//! The DATA envelope and answer/query records are private to this bench.
use std::time::Duration;

use uuid::Uuid;

use super::{
    Admission, Limits, Runtime, State, TransmissionId,
    device::{SimulatedDevice, Transmission},
};
use crate::modem::{arq::Answer, audio_handshake, bw500_waveform::Waveform, data};

const PAYLOAD: usize = 43;
const HEADER: usize = 10;
const BLOCK: usize = PAYLOAD - HEADER;
const MAGIC: &[u8; 4] = b"LWB1";
const ANSWER_TIMEOUT: Duration = Duration::from_secs(6);
const MAX_TRANSFER: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Data = 1,
    Query = 2,
    Delivered = 3,
    Missing = 4,
}

struct Record {
    kind: Kind,
    sequence: u32,
    bytes: Vec<u8>,
}

impl Record {
    fn frame(&self) -> Result<[u8; 46], String> {
        if self.bytes.len() > BLOCK || (self.kind == Kind::Data) == self.bytes.is_empty() {
            return Err("invalid loopback record payload".into());
        }
        let mut body = [0; PAYLOAD];
        body[..4].copy_from_slice(MAGIC);
        body[4] = self.kind as u8;
        body[5..9].copy_from_slice(&self.sequence.to_be_bytes());
        body[9] = self.bytes.len() as u8;
        body[HEADER..HEADER + self.bytes.len()].copy_from_slice(&self.bytes);
        data::full(4, &body, 0x81)
            .and_then(|frame| frame.try_into().ok())
            .ok_or_else(|| "could not encode loopback frame".into())
    }

    fn decode(frame: &[u8]) -> Option<Self> {
        // Full DATA frames have no callsign-dependent short trailer.
        let frame = data::decode(4, frame, "")?;
        let body = frame.payload;
        if frame.field != 0x81 || body.get(..4)? != MAGIC {
            return None;
        }
        let kind = match body[4] {
            1 => Kind::Data,
            2 => Kind::Query,
            3 => Kind::Delivered,
            4 => Kind::Missing,
            _ => return None,
        };
        let end = HEADER + usize::from(body[9]);
        let bytes = body.get(HEADER..end)?;
        if (kind == Kind::Data) == bytes.is_empty() || body[end..].iter().any(|&b| b != 0) {
            return None;
        }
        Some(Self {
            kind,
            sequence: u32::from_be_bytes(body[5..9].try_into().ok()?),
            bytes: bytes.to_vec(),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub caller_received: Vec<u8>,
    pub responder_received: Vec<u8>,
    pub caller_data_attempts: u64,
    pub responder_data_attempts: u64,
    pub state_queries: usize,
    pub duplicate_frames: usize,
}

struct Station {
    runtime: Runtime<SimulatedDevice>,
    owner: Uuid,
    session: Uuid,
    receiver: Receiver,
}

fn error(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}

impl Station {
    fn new(local: &str, remote: Option<&str>) -> Result<Self, String> {
        let owner = Uuid::new_v4();
        let mut runtime = Runtime::new(
            Limits {
                answer_timeout: ANSWER_TIMEOUT,
                session_timeout: Duration::from_secs(3600),
                ..Limits::default()
            },
            SimulatedDevice::default(),
        )
        .map_err(error)?;
        let session = runtime
            .begin(
                Duration::ZERO,
                Admission {
                    owner,
                    operation: Uuid::new_v4(),
                    local_call: local.into(),
                    remote_call: remote.map(str::to_owned),
                    lease_expires: Duration::from_secs(3600),
                },
            )
            .map_err(error)?;
        Ok(Self {
            runtime,
            owner,
            session,
            receiver: Receiver::default(),
        })
    }

    fn take(&mut self) -> Result<(TransmissionId, Transmission), String> {
        self.runtime
            .device_mut()
            .take_transmission()
            .ok_or_else(|| {
                format!(
                    "expected pending transmission, got {:?}",
                    self.runtime.snapshot()
                )
            })
    }

    fn complete(&mut self, now: Duration) -> Result<(), String> {
        let presented = self
            .runtime
            .device()
            .presented()
            .ok_or("missing presentation")?;
        self.runtime.device_event(now, presented).map_err(error)?;
        let released = self
            .runtime
            .device_mut()
            .confirm_release()
            .ok_or("missing release")?;
        self.runtime.device_event(now, released).map_err(error)?;
        while self.runtime.pop_event().is_some() {}
        Ok(())
    }
}

struct Bench<'a> {
    waveform: &'a Waveform,
    now: Duration,
    queries: usize,
    duplicates: usize,
}

impl Bench<'_> {
    /// The cable model adds fixed attenuation, sample offset and turnaround.
    fn channel(&mut self, samples: Vec<f32>) -> Vec<f32> {
        let mut audio = vec![0.0; 73];
        audio.extend(samples.into_iter().map(|sample| sample * 0.35));
        audio.extend([0.0; 480]);
        self.now +=
            Duration::from_secs_f64(audio.len() as f64 / 48_000.0) + Duration::from_millis(85);
        audio
    }

    fn establish(&mut self, caller: &mut Station, responder: &mut Station) -> Result<(), String> {
        for caller_turn in [true, false, true, false, true] {
            let (from, to) = if caller_turn {
                (&mut *caller, &mut *responder)
            } else {
                (&mut *responder, &mut *caller)
            };
            let (_, Transmission::Establishment(burst)) = from.take()? else {
                return Err("expected handshake transmission".into());
            };
            let samples = audio_handshake::encode_burst(self.waveform, &burst)
                .ok_or("could not synthesize handshake")?;
            let audio = self.channel(samples);
            from.complete(self.now)?;
            let expected = to
                .runtime
                .handshake
                .as_ref()
                .and_then(|handshake| handshake.expected())
                .ok_or("receiver did not expect handshake audio")?;
            let decoded =
                audio_handshake::decode_burst(self.waveform, expected, &audio, 0..audio.len())
                    .ok_or("could not decode handshake audio")?;
            to.runtime
                .receive_handshake(self.now, to.session, decoded)
                .map_err(error)?;
        }
        if [caller, responder]
            .iter()
            .any(|station| station.runtime.snapshot().state != State::Connected)
        {
            return Err("audio handshake did not establish both stations".into());
        }
        Ok(())
    }

    fn transmit(&mut self, record: Record, erase: bool) -> Result<Option<Record>, String> {
        let mut samples = self
            .waveform
            .synthesize(&record.frame()?)
            .ok_or("could not synthesize loopback frame")?;
        if erase {
            samples.fill(0.0);
        }
        let audio = self.channel(samples);
        Ok(self
            .waveform
            .decode(&audio, 0..audio.len())
            .and_then(|frame| Record::decode(&frame.frame)))
    }

    fn transfer(
        &mut self,
        sender: &mut Station,
        receiver: &mut Station,
        input: &[u8],
        erase_data: bool,
    ) -> Result<Vec<u8>, String> {
        for (index, bytes) in input.chunks(BLOCK).enumerate() {
            let sequence = index as u32;
            sender
                .runtime
                .enqueue(self.now, sender.owner, sender.session, bytes.to_vec())
                .map_err(|(error, _)| format!("{error:?}"))?;
            let mut erase_data = sequence == 0 && erase_data;
            let mut erase_answer = sequence == 0 && !erase_data;
            let mut acknowledged = false;
            for _ in 0..5 {
                let (id, tx) = sender.take()?;
                let record = match tx {
                    Transmission::Data(bytes) => Record {
                        kind: Kind::Data,
                        sequence,
                        bytes,
                    },
                    Transmission::StateQuery => {
                        self.queries += 1;
                        Record {
                            kind: Kind::Query,
                            sequence,
                            bytes: Vec::new(),
                        }
                    }
                    _ => return Err("unexpected handshake during transfer".into()),
                };
                let was_data = record.kind == Kind::Data;
                let decoded = self.transmit(record, was_data && erase_data)?;
                if was_data {
                    erase_data = false;
                }
                sender.complete(self.now)?;
                if let Some(decoded) = decoded {
                    receiver.runtime.tick(self.now).map_err(error)?;
                    if receiver.runtime.snapshot().state != State::Connected {
                        return Err("receiving session is no longer connected".into());
                    }
                    let receiver = &mut receiver.receiver;
                    let reply = receiver.accept(&decoded)?;
                    if sequence == 0
                        && was_data
                        && reply.kind == Kind::Delivered
                        && receiver.next == 1
                        && receiver.duplicates == 0
                    {
                        // Replay a decoded DATA frame to exercise delivery deduplication.
                        receiver.accept(&decoded)?;
                    }
                    let answer = self.transmit(reply, erase_answer)?;
                    erase_answer = false;
                    if let Some(answer) = answer.filter(|answer| answer.sequence == sequence) {
                        let status = match answer.kind {
                            Kind::Delivered => Answer::Delivered,
                            Kind::Missing => Answer::NotDelivered,
                            _ => return Err("invalid decoded answer kind".into()),
                        };
                        sender.runtime.answer(self.now, id, status).map_err(error)?;
                        if matches!(status, Answer::Delivered) {
                            acknowledged = true;
                            break;
                        }
                        continue;
                    }
                }
                self.now += ANSWER_TIMEOUT;
                sender.runtime.tick(self.now).map_err(error)?;
            }
            if !acknowledged {
                return Err("loopback block did not receive an answer".into());
            }
        }
        let stats = sender.runtime.snapshot().bytes;
        if stats.acknowledged != input.len() || !stats.balanced() {
            return Err("loopback byte accounting mismatch".into());
        }
        self.duplicates += receiver.receiver.duplicates;
        Ok(std::mem::take(&mut receiver.receiver.bytes))
    }
}

#[derive(Default)]
struct Receiver {
    next: u32,
    bytes: Vec<u8>,
    duplicates: usize,
}

impl Receiver {
    fn accept(&mut self, record: &Record) -> Result<Record, String> {
        match record.kind {
            Kind::Data if record.sequence == self.next => {
                self.bytes.extend_from_slice(&record.bytes);
                self.next += 1;
            }
            Kind::Data if record.sequence < self.next => self.duplicates += 1,
            Kind::Query => {}
            _ => return Err("unexpected loopback receive record".into()),
        }
        Ok(Record {
            sequence: record.sequence,
            kind: if record.sequence < self.next {
                Kind::Delivered
            } else {
                Kind::Missing
            },
            bytes: Vec::new(),
        })
    }
}

/// Run sequential turns over a simulated half-duplex channel. Each direction
/// injects one loss, and both endpoints must reproduce the original bytes.
pub fn run(waveform: &Waveform, outgoing: &[u8], incoming: &[u8]) -> Result<Report, String> {
    if [outgoing, incoming]
        .iter()
        .any(|bytes| bytes.is_empty() || bytes.len() > MAX_TRANSFER)
    {
        return Err(format!("each direction requires 1–{MAX_TRANSFER} bytes"));
    }
    let mut caller = Station::new("W9SSJ", Some("W1AW"))?;
    let mut responder = Station::new("W1AW", None)?;
    let mut bench = Bench {
        waveform,
        now: Duration::ZERO,
        queries: 0,
        duplicates: 0,
    };
    bench.establish(&mut caller, &mut responder)?;
    let responder_received = bench.transfer(&mut caller, &mut responder, outgoing, false)?;
    let caller_received = bench.transfer(&mut responder, &mut caller, incoming, true)?;
    if responder_received != outgoing || caller_received != incoming {
        return Err("loopback payload mismatch".into());
    }
    Ok(Report {
        caller_received,
        responder_received,
        caller_data_attempts: caller.runtime.snapshot().bytes.transmit_attempts,
        responder_data_attempts: responder.runtime.snapshot().bytes.transmit_attempts,
        state_queries: bench.queries,
        duplicate_frames: bench.duplicates,
    })
}
