//! Bounded stop-and-wait sender at the validated-PHY boundary.
//!
//! A missing answer requests peer state; it never licenses retransmitting DATA.
//! Only a decoded negative reply retries the pending block. Local tokens bind
//! callbacks to a transmit/receive window; they are not sequence numbers on air.
//! Wire encoding, reply recognition and rate-dependent repacketization remain
//! the responsibility of the session/PHY owner.

use std::{collections::VecDeque, time::Duration};

#[derive(Clone, Copy)]
pub struct Limits {
    pub answer_timeout: Duration,
    pub transmit_timeout: Duration,
    pub max_queries: u8,
    pub max_retries: u8,
    pub max_buffer_bytes: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Data { token: u64, bytes: Vec<u8> },
    Query { token: u64 },
    Acknowledged { remaining_bytes: usize },
    Failed(Failure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    TransmitFailed,
    TransmitTimedOut,
    AnswerTimedOut,
    RetryLimit,
    Aborted,
}

#[derive(Clone, Copy, Debug)]
pub enum Answer {
    Delivered,
    NotDelivered,
}

enum State {
    Ready,
    Sending { token: u64, deadline: Duration },
    Waiting { token: u64, deadline: Duration },
    Failed,
}

pub struct Sender {
    limits: Limits,
    queue: VecDeque<Vec<u8>>,
    buffered: usize,
    state: State,
    retries: u8,
    queries: u8,
    next_token: u64,
    last_time: Duration,
}

impl Sender {
    pub fn new(limits: Limits) -> Option<Self> {
        if limits.answer_timeout.is_zero()
            || limits.transmit_timeout.is_zero()
            || limits.max_buffer_bytes == 0
        {
            return None;
        }
        Some(Self {
            limits,
            queue: VecDeque::new(),
            buffered: 0,
            state: State::Ready,
            retries: 0,
            queries: 0,
            next_token: 0,
            last_time: Duration::ZERO,
        })
    }

    /// Blocks are already framed by the session owner. Refusal changes no state.
    pub fn enqueue(&mut self, bytes: Vec<u8>) -> Result<(), Vec<u8>> {
        if bytes.is_empty()
            || matches!(self.state, State::Failed)
            || bytes.len() > self.limits.max_buffer_bytes.saturating_sub(self.buffered)
        {
            return Err(bytes);
        }
        self.buffered += bytes.len();
        self.queue.push_back(bytes);
        Ok(())
    }

    pub fn buffered_bytes(&self) -> usize {
        self.buffered
    }

    /// Read-only retained payload, in send order; includes the outstanding
    /// block. Inspection never retires, discards, or authorizes replay.
    pub fn retained_blocks(&self) -> impl Iterator<Item = &[u8]> {
        self.queue.iter().map(Vec::as_slice)
    }

    pub fn start(&mut self, now: Duration) -> Option<Action> {
        if !self.advance(now) || !matches!(self.state, State::Ready) || self.queue.is_empty() {
            return None;
        }
        Some(self.data(now))
    }

    pub fn transmitted(&mut self, now: Duration, token: u64, success: bool) -> Option<Action> {
        if !self.advance(now) {
            return None;
        }
        if !matches!(self.state, State::Sending { token: expected, .. } if token == expected) {
            return None;
        }
        if matches!(self.state, State::Sending { deadline, .. } if now >= deadline) {
            return Some(self.fail(Failure::TransmitTimedOut));
        }
        if !success {
            return Some(self.fail(Failure::TransmitFailed));
        }
        self.state = State::Waiting {
            token,
            deadline: now.saturating_add(self.limits.answer_timeout),
        };
        None
    }

    /// The PHY owner must validate the complete reply, peer/session key and
    /// receive window before binding it to the current token.
    pub fn answer(&mut self, now: Duration, token: u64, answer: Answer) -> Option<Action> {
        if !self.advance(now) {
            return None;
        }
        if !matches!(self.state, State::Waiting { token: expected, deadline } if token == expected && now < deadline)
        {
            return None;
        }
        match answer {
            Answer::Delivered => {
                self.buffered -= self.queue.pop_front().expect("pending block").len();
                self.state = State::Ready;
                self.queries = 0;
                self.retries = 0;
                Some(Action::Acknowledged {
                    remaining_bytes: self.buffered,
                })
            }
            Answer::NotDelivered => {
                if self.retries >= self.limits.max_retries {
                    return Some(self.fail(Failure::RetryLimit));
                }
                self.retries += 1;
                self.queries = 0;
                Some(self.data(now))
            }
        }
    }

    pub fn tick(&mut self, now: Duration) -> Option<Action> {
        if !self.advance(now) {
            return None;
        }
        if matches!(self.state, State::Sending { deadline, .. } if now >= deadline) {
            return Some(self.fail(Failure::TransmitTimedOut));
        }
        if !matches!(self.state, State::Waiting { deadline, .. } if now >= deadline) {
            return None;
        }
        if self.queries >= self.limits.max_queries {
            return Some(self.fail(Failure::AnswerTimedOut));
        }
        self.queries += 1;
        let token = self.token();
        self.state = State::Sending {
            token,
            deadline: now.saturating_add(self.limits.transmit_timeout),
        };
        Some(Action::Query { token })
    }

    /// Abort explicitly discards queued bytes. Other failures retain them as
    /// unresolved; a new session must not silently treat them as delivered.
    pub fn abort(&mut self) -> Action {
        self.queue.clear();
        self.buffered = 0;
        self.fail(Failure::Aborted)
    }

    fn advance(&mut self, now: Duration) -> bool {
        if now < self.last_time {
            return false;
        }
        self.last_time = now;
        true
    }

    fn token(&mut self) -> u64 {
        let token = self.next_token;
        self.next_token = self
            .next_token
            .checked_add(1)
            .expect("transmission token space exhausted");
        token
    }

    fn data(&mut self, now: Duration) -> Action {
        let token = self.token();
        self.state = State::Sending {
            token,
            deadline: now.saturating_add(self.limits.transmit_timeout),
        };
        Action::Data {
            token,
            bytes: self.queue.front().expect("pending block").clone(),
        }
    }

    fn fail(&mut self, failure: Failure) -> Action {
        self.state = State::Failed;
        Action::Failed(failure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sender() -> Sender {
        Sender::new(Limits {
            answer_timeout: Duration::from_secs(1),
            transmit_timeout: Duration::from_secs(10),
            max_queries: 2,
            max_retries: 1,
            max_buffer_bytes: 100,
        })
        .unwrap()
    }

    #[test]
    fn missing_answer_queries_without_retransmitting_and_retires_once() {
        let mut sender = sender();
        sender.enqueue(vec![1, 2, 3]).unwrap();
        sender.enqueue(vec![4, 5]).unwrap();
        assert_eq!(
            sender.start(Duration::ZERO),
            Some(Action::Data {
                token: 0,
                bytes: vec![1, 2, 3]
            })
        );
        assert!(
            sender
                .answer(Duration::ZERO, 0, Answer::Delivered)
                .is_none()
        );
        sender.transmitted(Duration::ZERO, 0, true);
        assert_eq!(
            sender.tick(Duration::from_secs(1)),
            Some(Action::Query { token: 1 })
        );
        assert_eq!(sender.buffered_bytes(), 5);
        sender.transmitted(Duration::from_secs(1), 1, true);
        assert!(
            sender
                .answer(Duration::from_secs(1), 0, Answer::Delivered)
                .is_none()
        );
        assert_eq!(
            sender.answer(Duration::from_secs(1), 1, Answer::Delivered),
            Some(Action::Acknowledged { remaining_bytes: 2 })
        );
        assert!(
            sender
                .answer(Duration::from_secs(1), 1, Answer::Delivered)
                .is_none()
        );
        assert_eq!(
            sender.start(Duration::from_secs(1)),
            Some(Action::Data {
                token: 2,
                bytes: vec![4, 5]
            })
        );
    }

    #[test]
    fn negative_answer_alone_retries_and_limits_preserve_unresolved_bytes() {
        let mut sender = sender();
        sender.enqueue(vec![0xff; 4]).unwrap();
        sender.start(Duration::ZERO);
        sender.transmitted(Duration::ZERO, 0, true);
        assert_eq!(
            sender.answer(Duration::ZERO, 0, Answer::NotDelivered),
            Some(Action::Data {
                token: 1,
                bytes: vec![0xff; 4]
            })
        );
        sender.transmitted(Duration::ZERO, 1, true);
        assert_eq!(
            sender.answer(Duration::ZERO, 1, Answer::NotDelivered),
            Some(Action::Failed(Failure::RetryLimit))
        );
        assert_eq!(sender.buffered_bytes(), 4);
        assert!(sender.start(Duration::ZERO).is_none());
        assert!(sender.enqueue(vec![2]).is_err());
        sender.abort();
        assert_eq!(sender.buffered_bytes(), 0);
    }

    #[test]
    fn query_budget_exhaustion_and_transmit_failure_do_not_acknowledge_data() {
        let mut sender = sender();
        sender.enqueue(vec![9]).unwrap();
        sender.start(Duration::ZERO);
        sender.transmitted(Duration::ZERO, 0, true);
        for (second, token) in [(1, 1), (2, 2)] {
            let now = Duration::from_secs(second);
            assert_eq!(sender.tick(now), Some(Action::Query { token }));
            sender.transmitted(now, token, true);
        }
        assert_eq!(
            sender.tick(Duration::from_secs(3)),
            Some(Action::Failed(Failure::AnswerTimedOut))
        );
        assert_eq!(sender.buffered_bytes(), 1);
        let mut sender = self::sender();
        sender.enqueue(vec![7]).unwrap();
        sender.start(Duration::ZERO);
        assert_eq!(
            sender.transmitted(Duration::ZERO, 0, false),
            Some(Action::Failed(Failure::TransmitFailed))
        );
        assert_eq!(sender.buffered_bytes(), 1);
    }

    #[test]
    fn absent_or_late_playback_completion_is_a_fault() {
        for late_callback in [false, true] {
            let mut sender = sender();
            sender.enqueue(vec![42]).unwrap();
            sender.start(Duration::ZERO);
            let now = Duration::from_secs(10);
            let action = if late_callback {
                sender.transmitted(now, 0, true)
            } else {
                sender.tick(now)
            };
            assert_eq!(action, Some(Action::Failed(Failure::TransmitTimedOut)));
            assert_eq!(sender.buffered_bytes(), 1);
        }
    }
}
