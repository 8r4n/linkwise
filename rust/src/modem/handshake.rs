//! BW500 link establishment at the boundary between validated bursts and audio.
//!
//! The protocol order follows §5.3 of Saul St John's VARA HF notes. The
//! runtime owns waveform matching, turn-around, and actual playback. This
//! module covers the level-4 establishment exchange through session confirm;
//! post-confirm keepalive, payload ARQ, and teardown belong to the live link.

use std::time::Duration;

use super::setup::{LinkSetup, Profile};

const LEVEL: u8 = 4;

/// Decoders must validate CRCs and match each control waveform to its key
/// before passing a burst here. These types represent BW500 traffic only.
#[derive(Debug, PartialEq, Eq)]
pub enum Burst {
    Request { called: String },
    Response { called: String },
    Setup(LinkSetup),
    ConnectedAck { caller: String },
    Confirm { called: String, level: u8 },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Established {
    pub caller: String,
    pub called: String,
    pub responder_first_level: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    TimedOut,
    TransmitFailed,
    Aborted,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Transmit { token: u64, burst: Burst },
    Established(Established),
    Failed(Failure),
}

pub enum Event {
    Call { called: String },
    Received(Burst),
    Transmitted { token: u64, success: bool },
    Tick,
    Abort,
}

/// The only burst eligible to advance the current protocol phase.
pub enum Expected<'a> {
    Request(&'a str),
    Response(&'a str),
    Setup,
    ConnectedAck(&'a str),
    Confirm(&'a str),
}

#[derive(Clone, Copy)]
enum Wait {
    Response,
    Setup,
    ConnectedAck,
    Confirm,
}

enum AfterTransmit {
    Wait(Wait),
    Establish,
}

enum State {
    Idle,
    Sending { token: u64, after: AfterTransmit },
    Receiving(Wait),
}

struct Session {
    caller: Option<String>,
    called: String,
}

/// Reusable controller for one pending establishment. Successful completion
/// hands the session identity to the link layer, which must take ownership of
/// the radio before supplying further events.
pub struct Handshake {
    local: LinkSetup,
    listen: bool,
    timeout: Duration,
    deadline: Option<Duration>,
    last_time: Duration,
    next_token: u64,
    state: State,
    session: Option<Session>,
}

impl Handshake {
    pub fn expected(&self) -> Option<Expected<'_>> {
        match &self.state {
            State::Idle if self.listen => Some(Expected::Request(&self.local.caller)),
            State::Receiving(Wait::Response) => {
                Some(Expected::Response(&self.session.as_ref()?.called))
            }
            State::Receiving(Wait::Setup) => Some(Expected::Setup),
            State::Receiving(Wait::ConnectedAck) => Some(Expected::ConnectedAck(
                self.session.as_ref()?.caller.as_deref()?,
            )),
            State::Receiving(Wait::Confirm) => {
                Some(Expected::Confirm(&self.session.as_ref()?.called))
            }
            _ => None,
        }
    }
    /// The timeout is a local watchdog for each transmit or receive phase.
    /// It must cover the longest burst and turnaround on the selected bench.
    pub fn new(local: LinkSetup, listen: bool, timeout: Duration) -> Option<Self> {
        if timeout.is_zero() || local.requested_level != LEVEL {
            return None;
        }
        let encoded = local.encode(Profile::Bw500)?;
        let local = LinkSetup::decode(Profile::Bw500, &encoded)?;
        Some(Self {
            local,
            listen,
            timeout,
            deadline: None,
            last_time: Duration::ZERO,
            next_token: 0,
            state: State::Idle,
            session: None,
        })
    }

    /// Use a monotonic timestamp from one clock for this controller. Supply
    /// a tick at or after this deadline when no other event arrives.
    pub fn deadline(&self) -> Option<Duration> {
        self.deadline
    }

    /// Ignore stale timestamps, unrelated bursts, and completions for other
    /// transmissions. A phase that reaches its deadline returns to idle.
    pub fn handle(&mut self, now: Duration, event: Event) -> Option<Action> {
        if now < self.last_time {
            return None;
        }
        self.last_time = now;
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            return Some(self.fail(Failure::TimedOut));
        }

        match (&self.state, event) {
            (State::Idle, Event::Call { called }) => {
                let called = called.to_ascii_uppercase();
                if called == self.local.caller
                    || super::control::Message::ConnectRequest(super::control::Bandwidth::Hz500)
                        .tones(&called)
                        .is_none()
                {
                    return None;
                }
                self.session = Some(Session {
                    caller: Some(self.local.caller.clone()),
                    called: called.clone(),
                });
                Some(self.transmit(
                    now,
                    Burst::Request { called },
                    AfterTransmit::Wait(Wait::Response),
                ))
            }
            (State::Idle, Event::Received(Burst::Request { called }))
                if self.listen && called == self.local.caller =>
            {
                self.session = Some(Session {
                    caller: None,
                    called: called.clone(),
                });
                Some(self.transmit(
                    now,
                    Burst::Response { called },
                    AfterTransmit::Wait(Wait::Setup),
                ))
            }
            (
                State::Sending { token, after },
                Event::Transmitted {
                    token: done,
                    success,
                },
            ) if *token == done => {
                if !success {
                    return Some(self.fail(Failure::TransmitFailed));
                }
                match after {
                    AfterTransmit::Wait(phase) => {
                        self.state = State::Receiving(*phase);
                        self.deadline = Some(now.saturating_add(self.timeout));
                        None
                    }
                    AfterTransmit::Establish => Some(self.establish()),
                }
            }
            (State::Receiving(phase), Event::Received(burst)) => self.receive(now, *phase, burst),
            (State::Sending { .. } | State::Receiving(_), Event::Abort) => {
                Some(self.fail(Failure::Aborted))
            }
            _ => None,
        }
    }

    fn receive(&mut self, now: Duration, phase: Wait, burst: Burst) -> Option<Action> {
        let session = self.session.as_mut()?;
        let (burst, after) = match (phase, burst) {
            (Wait::Response, Burst::Response { called }) if session.called == called => (
                Burst::Setup(LinkSetup {
                    caller: self.local.caller.clone(),
                    high_speed: self.local.high_speed,
                    requested_level: self.local.requested_level,
                }),
                AfterTransmit::Wait(Wait::ConnectedAck),
            ),
            (Wait::Setup, Burst::Setup(setup))
                if setup.caller != session.called && setup.requested_level >= LEVEL =>
            {
                let caller = setup.caller;
                session.caller = Some(caller.clone());
                (
                    Burst::ConnectedAck { caller },
                    AfterTransmit::Wait(Wait::Confirm),
                )
            }
            (Wait::ConnectedAck, Burst::ConnectedAck { caller })
                if session.caller.as_ref() == Some(&caller) =>
            {
                (
                    Burst::Confirm {
                        called: session.called.clone(),
                        level: LEVEL,
                    },
                    AfterTransmit::Establish,
                )
            }
            (
                Wait::Confirm,
                Burst::Confirm {
                    called,
                    level: LEVEL,
                },
            ) if session.called == called => {
                return Some(self.establish());
            }
            _ => return None,
        };
        Some(self.transmit(now, burst, after))
    }

    fn transmit(&mut self, now: Duration, burst: Burst, after: AfterTransmit) -> Action {
        let token = self.next_token;
        self.next_token = self.next_token.wrapping_add(1);
        self.state = State::Sending { token, after };
        self.deadline = Some(now.saturating_add(self.timeout));
        Action::Transmit { token, burst }
    }

    fn reset(&mut self) -> Option<Session> {
        self.state = State::Idle;
        self.deadline = None;
        self.session.take()
    }

    fn establish(&mut self) -> Action {
        let session = self.reset().expect("establishment requires a session");
        Action::Established(Established {
            caller: session.caller.expect("setup precedes establishment"),
            called: session.called,
            responder_first_level: LEVEL,
        })
    }

    fn fail(&mut self, reason: Failure) -> Action {
        self.reset();
        Action::Failed(reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(10);

    fn station(call: &str, listen: bool) -> Handshake {
        Handshake::new(
            LinkSetup {
                caller: call.into(),
                high_speed: false,
                requested_level: 4,
            },
            listen,
            TIMEOUT,
        )
        .unwrap()
    }

    fn send(station: &mut Handshake, action: Option<Action>) -> Burst {
        let Some(Action::Transmit { token, burst }) = action else {
            panic!("expected a transmission, got {action:?}");
        };
        assert_eq!(
            station.handle(
                Duration::ZERO,
                Event::Transmitted {
                    token,
                    success: true
                }
            ),
            None,
        );
        burst
    }

    #[test]
    fn establishes_both_roles_only_after_confirm() {
        let mut caller = station("W9SSJ", false);
        let mut responder = station("W1AW", true);
        let now = Duration::ZERO;
        let action = caller.handle(
            now,
            Event::Call {
                called: "w1aw".into(),
            },
        );
        let request = send(&mut caller, action);
        let action = responder.handle(now, Event::Received(request));
        let response = send(&mut responder, action);
        let action = caller.handle(now, Event::Received(response));
        let setup = send(&mut caller, action);
        let action = responder.handle(now, Event::Received(setup));
        let ack = send(&mut responder, action);
        let action = caller.handle(now, Event::Received(ack));
        let Some(Action::Transmit {
            token,
            burst: confirm,
        }) = action
        else {
            panic!("expected session confirm, got {action:?}");
        };
        let established = || {
            Action::Established(Established {
                caller: "W9SSJ".into(),
                called: "W1AW".into(),
                responder_first_level: 4,
            })
        };
        assert_eq!(
            caller.handle(
                now,
                Event::Transmitted {
                    token,
                    success: true
                }
            ),
            Some(established()),
        );
        assert_eq!(
            responder.handle(now, Event::Received(confirm)),
            Some(established())
        );
        assert_eq!(caller.deadline(), None);
        assert_eq!(responder.deadline(), None);
    }

    #[test]
    fn ignores_wrong_peer_and_stale_transmit_completion() {
        let mut caller = station("W9SSJ", false);
        let now = Duration::ZERO;
        let action = caller.handle(
            now,
            Event::Call {
                called: "W1AW".into(),
            },
        );
        let Some(Action::Transmit { token, .. }) = action else {
            panic!("expected connect request");
        };
        assert_eq!(
            caller.handle(
                now,
                Event::Transmitted {
                    token: token + 1,
                    success: true
                }
            ),
            None
        );
        let response = || {
            Event::Received(Burst::Response {
                called: "W1AW".into(),
            })
        };
        assert_eq!(caller.handle(now, response()), None);
        assert_eq!(
            caller.handle(
                now,
                Event::Transmitted {
                    token,
                    success: true
                }
            ),
            None
        );
        assert_eq!(
            caller.handle(
                now,
                Event::Received(Burst::Response {
                    called: "W2AW".into()
                })
            ),
            None
        );
        assert!(matches!(
            caller.handle(now, response()),
            Some(Action::Transmit {
                burst: Burst::Setup(_),
                ..
            })
        ));
    }

    #[test]
    fn timeout_and_transmit_failure_return_to_idle() {
        let mut responder = station("W1AW", true);
        let request = || {
            Event::Received(Burst::Request {
                called: "W1AW".into(),
            })
        };
        let action = responder.handle(Duration::ZERO, request());
        let Some(Action::Transmit { token, .. }) = action else {
            panic!("expected connect response");
        };
        assert_eq!(
            responder.handle(
                Duration::ZERO,
                Event::Transmitted {
                    token,
                    success: false
                }
            ),
            Some(Action::Failed(Failure::TransmitFailed)),
        );
        assert!(matches!(
            responder.handle(Duration::ZERO, request()),
            Some(Action::Transmit { .. })
        ));
        assert_eq!(
            responder.handle(TIMEOUT, Event::Tick),
            Some(Action::Failed(Failure::TimedOut))
        );
        assert!(matches!(
            responder.handle(TIMEOUT, request()),
            Some(Action::Transmit { .. })
        ));
    }
}
