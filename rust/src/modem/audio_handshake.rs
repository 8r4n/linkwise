//! Audio codec boundary for base-rate VARA HF link establishment.
//!
//! Transmit completion must be supplied after actual playback and PTT release;
//! returning audio does not advance the protocol. The device/session owner
//! schedules turnaround and maintains exclusive ownership of the audio route.

use super::{
    bw500_waveform::Waveform,
    control::{self, Bandwidth, Message},
    handshake::{self, Burst, Event, Expected, Handshake},
    setup::{LinkSetup, Profile},
    wide_level4,
};
use std::{ops::Range, time::Duration};

pub enum Action {
    Transmit { token: u64, audio: Vec<f32> },
    Established(handshake::Established),
    Failed(handshake::Failure),
}

pub struct AudioHandshake {
    protocol: Handshake,
    waveform: SetupWaveform,
}

enum SetupWaveform {
    Bw500(Box<Waveform>),
    Wide(Bandwidth, Box<wide_level4::Waveform>),
}

#[derive(Clone, Copy)]
enum Codec<'a> {
    Bw500(&'a Waveform),
    Wide(Bandwidth, &'a wide_level4::Waveform),
}

impl Codec<'_> {
    fn bandwidth(self) -> Bandwidth {
        match self {
            Self::Bw500(_) => Bandwidth::Hz500,
            Self::Wide(bandwidth, _) => bandwidth,
        }
    }

    fn profile(self) -> Profile {
        match self {
            Self::Bw500(_) => Profile::Bw500,
            Self::Wide(..) => Profile::Wide,
        }
    }

    fn confirm(self) -> Message {
        match self {
            Self::Bw500(_) => Message::Bw500Confirm,
            Self::Wide(bandwidth, _) => Message::WideConfirm(bandwidth),
        }
    }

    fn setup_nak(self) -> Message {
        match self {
            Self::Bw500(_) => Message::Bw500SetupNak,
            Self::Wide(bandwidth, _) => Message::WideSetupNak(bandwidth),
        }
    }

    fn render_setup(self, setup: &LinkSetup) -> Option<Vec<f32>> {
        let frame = setup.encode(self.profile())?;
        match self {
            Self::Bw500(waveform) => waveform.synthesize(&frame.try_into().ok()?),
            Self::Wide(_, waveform) => waveform.synthesize(
                &frame.try_into().ok()?,
                &mut waveform.setup_lead(&setup.caller)?,
            ),
        }
    }

    fn read_setup(self, audio: &[f32], candidate: Range<usize>) -> Option<LinkSetup> {
        match self {
            Self::Bw500(waveform) => {
                let decoded = waveform.decode(audio, candidate)?;
                LinkSetup::decode(self.profile(), &decoded.frame)
            }
            Self::Wide(_, waveform) => {
                let decoded = waveform.decode(audio, candidate)?;
                LinkSetup::decode(self.profile(), &decoded)
            }
        }
    }
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
            waveform: SetupWaveform::Bw500(Box::new(waveform)),
        })
    }

    /// Construct a level-4 setup codec for one wide bandwidth.
    pub fn new_wide(
        local: LinkSetup,
        listen: bool,
        timeout: Duration,
        bandwidth: Bandwidth,
    ) -> Option<Self> {
        Some(Self {
            protocol: Handshake::new(local, listen, timeout)?,
            waveform: SetupWaveform::Wide(
                bandwidth,
                Box::new(wide_level4::Waveform::new(bandwidth)?),
            ),
        })
    }

    fn codec(&self) -> Codec<'_> {
        match &self.waveform {
            SetupWaveform::Bw500(waveform) => Codec::Bw500(waveform),
            SetupWaveform::Wide(bandwidth, waveform) => Codec::Wide(*bandwidth, waveform),
        }
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
        let burst = decode(self.codec(), self.protocol.expected()?, audio, candidate)?;
        self.handle(now, Event::Received(burst))
    }

    fn encode(&mut self, now: Duration, action: handshake::Action) -> Option<Action> {
        Some(match action {
            handshake::Action::Established(session) => Action::Established(session),
            handshake::Action::Failed(failure) => Action::Failed(failure),
            handshake::Action::Transmit { token, burst } => {
                let audio = encode(self.codec(), &burst);
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
    decode(Codec::Bw500(waveform), expected, audio, candidate)
}

fn decode(
    codec: Codec<'_>,
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
            Message::ConnectRequest(codec.bandwidth()).find(called, audio, candidate.start)?;
            Burst::Request {
                called: called.into(),
            }
        }
        Expected::Response(called) => {
            Message::ConnectResponse(codec.bandwidth()).find(called, audio, candidate.start)?;
            Burst::Response {
                called: called.into(),
            }
        }
        Expected::Setup => Burst::Setup(codec.read_setup(audio, candidate)?),
        Expected::ConnectedAck { caller, called } => {
            if control::find_connected_ack(codec.bandwidth(), caller, audio, candidate.start)
                .is_some()
            {
                Burst::ConnectedAck {
                    caller: caller.into(),
                }
            } else {
                codec.setup_nak().find(called, audio, candidate.start)?;
                Burst::SetupRejected {
                    called: called.into(),
                }
            }
        }
        Expected::Confirm(called) => {
            codec.confirm().find(called, audio, candidate.start)?;
            Burst::Confirm {
                called: called.into(),
                level: 4,
            }
        }
    })
}

pub(crate) fn encode_burst(waveform: &Waveform, burst: &Burst) -> Option<Vec<f32>> {
    encode(Codec::Bw500(waveform), burst)
}

fn encode(codec: Codec<'_>, burst: &Burst) -> Option<Vec<f32>> {
    match burst {
        Burst::Request { called } => Message::ConnectRequest(codec.bandwidth()).synthesize(called),
        Burst::Response { called } => {
            Message::ConnectResponse(codec.bandwidth()).synthesize(called)
        }
        Burst::Setup(setup) => codec.render_setup(setup),
        Burst::ConnectedAck { caller } => {
            control::synthesize_connected_ack(codec.bandwidth(), caller)
        }
        Burst::Confirm { called, level: 4 } => codec.confirm().synthesize(called),
        _ => None,
    }
}
