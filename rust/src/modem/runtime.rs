//! Portable, single-owner modem runtime at the validated-protocol boundary.
//!
//! All mutation requires `&mut self`; callers serialize commands here rather
//! than running another protocol machine. Time is supplied explicitly from one
//! monotonic clock (virtual in tests). No native devices or host TCP listeners
//! are opened. Wire DATA/reply/query qualification remains separate work.
pub mod audio_loopback;
pub mod device;
pub mod runner;

use super::{arq, handshake, setup::LinkSetup};
use device::{DeviceEvent, ExecutionDevice, Transmission};
use std::{collections::VecDeque, time::Duration};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransmissionId {
    pub session: Uuid,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Establishing,
    Connected,
    Aborted,
    Faulted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Handshake(handshake::Failure),
    Arq(arq::Failure),
    Device,
    Watchdog,
    LeaseExpired,
    SessionExpired,
    CaptureDiscontinuity,
    EventOverflow,
    UnpresentedRelease,
    RunnerStopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidConfig,
    InvalidCall,
    Busy,
    WrongOwner,
    WrongSession,
    WrongState,
    Backpressure,
    StaleTime,
    CounterExhausted,
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub handshake_timeout: Duration,
    pub transmit_timeout: Duration,
    pub answer_timeout: Duration,
    pub session_timeout: Duration,
    pub max_buffer_bytes: usize,
    pub max_events: usize,
    pub max_queries: u8,
    pub max_retries: u8,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            handshake_timeout: Duration::from_secs(30),
            transmit_timeout: Duration::from_secs(15),
            answer_timeout: Duration::from_secs(5),
            session_timeout: Duration::from_secs(300),
            max_buffer_bytes: 64 * 1024,
            max_events: 128,
            max_queries: 2,
            max_retries: 2,
        }
    }
}

/// Installed station policy must validate admission before calling `begin`.
/// This record is correlation/expiry, not a cryptographic authorization grant.
pub struct Admission {
    pub owner: Uuid,
    pub operation: Uuid,
    pub local_call: String,
    /// None explicitly admits a listening/responder session in simulation.
    pub remote_call: Option<String>,
    pub lease_expires: Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Accounting {
    pub accepted: usize,
    pub pending: usize,
    pub outstanding: usize,
    pub acknowledged: usize,
    pub unresolved: usize,
    pub discarded: usize,
    pub transmit_attempts: u64,
}

impl Accounting {
    pub fn balanced(&self) -> bool {
        [
            self.pending,
            self.outstanding,
            self.acknowledged,
            self.unresolved,
            self.discarded,
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
            == Some(self.accepted)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub operation: Option<Uuid>,
    pub session: Option<Uuid>,
    pub state: State,
    pub local_call: Option<String>,
    pub peer: Option<String>,
    pub failure: Option<Failure>,
    pub bytes: Accounting,
    pub release_pending: bool,
    pub stale_events: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    State(State),
    Transmitting(TransmissionId),
    Acknowledged(usize),
    Fault(Failure),
}

#[derive(Clone, Copy)]
enum EngineToken {
    Handshake(u64),
    Arq(u64),
}
struct Active {
    id: TransmissionId,
    token: EngineToken,
    presented: bool,
}

pub struct Runtime<D: ExecutionDevice> {
    limits: Limits,
    device: D,
    snapshot: Snapshot,
    owner: Option<Uuid>,
    handshake: Option<handshake::Handshake>,
    sender: arq::Sender,
    block_lengths: VecDeque<usize>,
    active: Option<Active>,
    answer: Option<(TransmissionId, u64)>,
    events: VecDeque<Event>,
    next_tx: u64,
    now: Duration,
    lease_expires: Duration,
    session_expires: Duration,
    capture: Option<(Uuid, u64)>,
}

impl<D: ExecutionDevice> Runtime<D> {
    pub fn new(limits: Limits, device: D) -> Result<Self, Error> {
        if limits.handshake_timeout.is_zero()
            || limits.session_timeout.is_zero()
            || limits.max_events == 0
        {
            return Err(Error::InvalidConfig);
        }
        let sender = Self::sender(limits)?;
        Ok(Self {
            limits,
            device,
            snapshot: Snapshot {
                operation: None,
                session: None,
                state: State::Idle,
                local_call: None,
                peer: None,
                failure: None,
                bytes: Accounting::default(),
                release_pending: false,
                stale_events: 0,
            },
            owner: None,
            handshake: None,
            sender,
            block_lengths: VecDeque::new(),
            active: None,
            answer: None,
            events: VecDeque::new(),
            next_tx: 0,
            now: Duration::ZERO,
            lease_expires: Duration::ZERO,
            session_expires: Duration::ZERO,
            capture: None,
        })
    }

    fn sender(limits: Limits) -> Result<arq::Sender, Error> {
        arq::Sender::new(arq::Limits {
            answer_timeout: limits.answer_timeout,
            transmit_timeout: limits.transmit_timeout,
            max_queries: limits.max_queries,
            max_retries: limits.max_retries,
            max_buffer_bytes: limits.max_buffer_bytes,
        })
        .ok_or(Error::InvalidConfig)
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }
    /// Explicit owner-only access to retained payload. Snapshots/events contain
    /// counts only. A new session never adopts or replays these bytes.
    pub fn retained_blocks(
        &self,
        owner: Uuid,
        session: Uuid,
    ) -> Result<impl Iterator<Item = &[u8]>, Error> {
        self.session(session)?;
        self.check_owner(owner)?;
        Ok(self.sender.retained_blocks())
    }

    pub fn pop_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }
    pub fn device(&self) -> &D {
        &self.device
    }
    /// Execution adapters may deliver observations, never modify session state.
    pub fn device_mut(&mut self) -> &mut D {
        &mut self.device
    }

    pub fn begin(&mut self, now: Duration, admission: Admission) -> Result<Uuid, Error> {
        self.advance(now)?;
        if self.live()
            || self.active.is_some()
            || self.sender.buffered_bytes() != 0
            || !self.events.is_empty()
        {
            return Err(Error::Busy);
        }
        if admission.lease_expires <= now {
            return Err(Error::InvalidConfig);
        }
        let local = LinkSetup {
            caller: admission.local_call,
            high_speed: false,
            requested_level: 4,
        };
        let local_call = local.caller.to_ascii_uppercase();
        let mut handshake = handshake::Handshake::new(
            local,
            admission.remote_call.is_none(),
            self.limits.handshake_timeout,
        )
        .ok_or(Error::InvalidCall)?;
        let action = if let Some(called) = admission.remote_call {
            Some(
                handshake
                    .handle(now, handshake::Event::Call { called })
                    .ok_or(Error::InvalidCall)?,
            )
        } else {
            None
        };
        let session = Uuid::new_v4();
        self.snapshot = Snapshot {
            operation: Some(admission.operation),
            session: Some(session),
            state: State::Establishing,
            local_call: Some(local_call),
            peer: None,
            failure: None,
            bytes: Accounting::default(),
            release_pending: false,
            stale_events: 0,
        };
        self.owner = Some(admission.owner);
        self.sender = Self::sender(self.limits)?;
        self.block_lengths.clear();
        self.capture = None;
        self.handshake = Some(handshake);
        self.lease_expires = admission.lease_expires;
        self.session_expires = now.saturating_add(self.limits.session_timeout);
        self.emit(Event::State(State::Establishing));
        if let Some(action) = action {
            self.handshake_action(action);
        }
        Ok(session)
    }

    /// This is the validated decoder boundary, not a raw network command.
    pub fn receive_handshake(
        &mut self,
        now: Duration,
        session: Uuid,
        burst: handshake::Burst,
    ) -> Result<(), Error> {
        self.advance(now)?;
        self.session(session)?;
        if self.snapshot.state != State::Establishing {
            return Err(Error::WrongState);
        }
        let action = self
            .handshake
            .as_mut()
            .unwrap()
            .handle(now, handshake::Event::Received(burst));
        if let Some(action) = action {
            self.handshake_action(action);
        }
        Ok(())
    }

    /// One bounded, already packetized block. Host-byte packetization is a later
    /// link-profile stage; refusal is atomic and returns the original bytes.
    pub fn enqueue(
        &mut self,
        now: Duration,
        owner: Uuid,
        session: Uuid,
        bytes: Vec<u8>,
    ) -> Result<(), (Error, Vec<u8>)> {
        let check = self
            .advance(now)
            .and_then(|_| self.session(session))
            .and_then(|_| self.check_owner(owner));
        if let Err(e) = check {
            return Err((e, bytes));
        }
        if self.snapshot.state != State::Connected {
            return Err((Error::WrongState, bytes));
        }
        let count = bytes.len();
        let Some(total) = self.snapshot.bytes.accepted.checked_add(count) else {
            return Err((Error::CounterExhausted, bytes));
        };
        self.sender
            .enqueue(bytes)
            .map_err(|bytes| (Error::Backpressure, bytes))?;
        self.block_lengths.push_back(count);
        self.snapshot.bytes.accepted = total;
        self.snapshot.bytes.pending += count;
        self.pump();
        Ok(())
    }

    /// Validated peer/session/window answer. Local tokens are not wire sequence
    /// numbers and do not provide on-air duplicate suppression.
    pub fn answer(
        &mut self,
        now: Duration,
        id: TransmissionId,
        answer: arq::Answer,
    ) -> Result<(), Error> {
        self.advance(now)?;
        if self.snapshot.state != State::Connected
            || self.answer.is_none_or(|(expected, _)| expected != id)
        {
            self.stale();
            return Ok(());
        }
        let token = self.answer.unwrap().1;
        if let Some(action) = self.sender.answer(now, token, answer) {
            self.answer = None;
            self.arq_action(action);
            self.pump();
        }
        Ok(())
    }

    pub fn device_event(&mut self, now: Duration, event: DeviceEvent) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::StaleTime);
        }
        // Already-observed failure/release evidence has priority over protocol
        // timeouts. Presentation still goes through deadline checks first.
        if matches!(
            event,
            DeviceEvent::WatchdogReleased(_) | DeviceEvent::Failed(_)
        ) {
            self.now = now;
            self.apply_device(event);
            self.advance(now)?;
        } else {
            self.advance(now)?;
            self.apply_device(event);
        }
        Ok(())
    }

    /// Absolute half-open sample range in one continuous capture epoch.
    /// A new epoch or gap cannot silently continue an active session.
    pub fn capture_range(
        &mut self,
        now: Duration,
        session: Uuid,
        epoch: Uuid,
        start: u64,
        end: u64,
    ) -> Result<(), Error> {
        self.advance(now)?;
        self.session(session)?;
        if !self.live() {
            return Err(Error::WrongState);
        }
        if start >= end
            || self
                .capture
                .is_some_and(|(previous, next)| previous != epoch || next != start)
        {
            self.fault(Failure::CaptureDiscontinuity);
        } else {
            self.capture = Some((epoch, end));
        }
        Ok(())
    }

    /// Capture epochs/sample continuity are checked by the audio adapter. A gap
    /// faults this session rather than joining discontinuous input.
    pub fn capture_discontinuity(&mut self, now: Duration, session: Uuid) -> Result<(), Error> {
        self.advance(now)?;
        self.session(session)?;
        if self.live() {
            self.fault(Failure::CaptureDiscontinuity);
        }
        Ok(())
    }

    /// Explicit discard and release, also used on host channel loss. It remains
    /// available when event queues are full; repeated cleanup is idempotent.
    pub fn abort(&mut self, now: Duration, owner: Uuid, session: Uuid) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::StaleTime);
        }
        self.session(session)?;
        self.check_owner(owner)?;
        if self.snapshot.state == State::Aborted {
            return self.tick(now);
        }
        self.cancel_release();
        self.answer = None;
        self.handshake = None;
        self.sender.abort();
        self.block_lengths.clear();
        let b = &mut self.snapshot.bytes;
        b.discarded += b.pending + b.outstanding + b.unresolved;
        b.pending = 0;
        b.outstanding = 0;
        b.unresolved = 0;
        self.snapshot.state = State::Aborted;
        self.emit(Event::State(State::Aborted));
        self.tick(now)
    }

    pub fn tick(&mut self, now: Duration) -> Result<(), Error> {
        self.advance(now)
    }

    fn advance(&mut self, now: Duration) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::StaleTime);
        }
        self.now = now;
        if let Some(event) = self.device.poll(now) {
            self.apply_device(event);
        }
        if !self.live() {
            return Ok(());
        }
        if now >= self.lease_expires {
            self.fault(Failure::LeaseExpired);
        } else if now >= self.session_expires {
            self.fault(Failure::SessionExpired);
        } else if self.snapshot.state == State::Establishing {
            if let Some(action) = self
                .handshake
                .as_mut()
                .unwrap()
                .handle(now, handshake::Event::Tick)
            {
                self.handshake_action(action);
            }
        } else if let Some(action) = self.sender.tick(now) {
            self.arq_action(action);
        }
        Ok(())
    }

    fn apply_device(&mut self, event: DeviceEvent) {
        let id = event.id();
        if self.active.as_ref().is_none_or(|a| a.id != id) {
            self.stale();
            return;
        }
        match event {
            DeviceEvent::WatchdogReleased(_) => {
                if self.live() {
                    self.fault(Failure::Watchdog);
                }
                self.active = None;
                self.snapshot.release_pending = false;
            }
            DeviceEvent::Failed(_) => {
                if self.live() {
                    self.fault(Failure::Device);
                }
            }
            DeviceEvent::Presented(_) => {
                if !self.live() {
                    self.stale();
                    return;
                }
                let active = self.active.as_mut().unwrap();
                if active.presented {
                    self.stale();
                    return;
                }
                active.presented = true;
                self.snapshot.release_pending = true;
                if self.device.release(id).is_err() {
                    self.fault(Failure::Device);
                }
            }
            DeviceEvent::Released(_) => {
                let active = self.active.take().unwrap();
                self.snapshot.release_pending = false;
                if !self.live() {
                    return;
                }
                if !active.presented {
                    self.fault(Failure::UnpresentedRelease);
                    return;
                }
                match active.token {
                    EngineToken::Handshake(token) => {
                        let action = self.handshake.as_mut().unwrap().handle(
                            self.now,
                            handshake::Event::Transmitted {
                                token,
                                success: true,
                            },
                        );
                        if let Some(action) = action {
                            self.handshake_action(action);
                        }
                    }
                    EngineToken::Arq(token) => {
                        self.answer = Some((id, token));
                        if let Some(action) = self.sender.transmitted(self.now, token, true) {
                            self.arq_action(action);
                        }
                    }
                }
            }
        }
    }

    fn handshake_action(&mut self, action: handshake::Action) {
        if !self.live() {
            return;
        }
        match action {
            handshake::Action::Transmit { token, burst } => self.transmit(
                EngineToken::Handshake(token),
                Transmission::Establishment(burst),
            ),
            handshake::Action::Established(peer) => {
                self.snapshot.peer = Some(
                    if self.snapshot.local_call.as_deref() == Some(&peer.caller) {
                        peer.called
                    } else {
                        peer.caller
                    },
                );
                self.handshake = None;
                self.snapshot.state = State::Connected;
                self.emit(Event::State(State::Connected));
            }
            handshake::Action::Failed(reason) => self.fault(Failure::Handshake(reason)),
        }
    }

    fn arq_action(&mut self, action: arq::Action) {
        if !self.live() {
            return;
        }
        match action {
            arq::Action::Data { token, bytes } => {
                if self.snapshot.bytes.outstanding == 0 {
                    self.snapshot.bytes.pending -= bytes.len();
                    self.snapshot.bytes.outstanding = bytes.len();
                }
                self.snapshot.bytes.transmit_attempts =
                    self.snapshot.bytes.transmit_attempts.saturating_add(1);
                self.transmit(EngineToken::Arq(token), Transmission::Data(bytes));
            }
            arq::Action::Query { token } => {
                self.answer = None;
                self.transmit(EngineToken::Arq(token), Transmission::StateQuery);
            }
            arq::Action::Acknowledged { .. } => {
                let count = self
                    .block_lengths
                    .pop_front()
                    .expect("acknowledged queued block");
                self.snapshot.bytes.outstanding = 0;
                self.snapshot.bytes.acknowledged += count;
                self.emit(Event::Acknowledged(count));
            }
            arq::Action::Failed(reason) => self.fault(Failure::Arq(reason)),
        }
    }

    fn transmit(&mut self, token: EngineToken, burst: Transmission) {
        if self.active.is_some() {
            self.fault(Failure::Device);
            return;
        }
        let Some(next) = self.next_tx.checked_add(1) else {
            self.fault(Failure::Device);
            return;
        };
        let id = TransmissionId {
            session: self.snapshot.session.unwrap(),
            sequence: self.next_tx,
        };
        self.next_tx = next;
        self.active = Some(Active {
            id,
            token,
            presented: false,
        });
        let deadline = self
            .now
            .saturating_add(self.limits.transmit_timeout)
            .min(self.lease_expires)
            .min(self.session_expires);
        if self.device.start(id, burst, deadline).is_err() {
            self.fault(Failure::Device);
            return;
        }
        self.emit(Event::Transmitting(id));
    }

    fn pump(&mut self) {
        if self.snapshot.state == State::Connected {
            if let Some(action) = self.sender.start(self.now) {
                self.arq_action(action);
            }
        }
    }

    fn cancel_release(&mut self) {
        if let Some(active) = &self.active {
            self.snapshot.release_pending = true;
            let _ = self.device.release(active.id); // watchdog retains independent authority
        }
    }

    fn fault(&mut self, failure: Failure) {
        if !self.live() {
            return;
        }
        self.snapshot.state = State::Faulted;
        self.snapshot.failure = Some(failure);
        self.answer = None;
        self.handshake = None;
        self.snapshot.bytes.unresolved += self.snapshot.bytes.outstanding;
        self.snapshot.bytes.outstanding = 0;
        self.cancel_release(); // before any event publication
        if self.events.len() < self.limits.max_events {
            self.events.push_back(Event::Fault(failure));
        }
    }

    fn emit(&mut self, event: Event) {
        if self.events.len() == self.limits.max_events {
            self.fault(Failure::EventOverflow);
        } else {
            self.events.push_back(event);
        }
    }

    fn stale(&mut self) {
        self.snapshot.stale_events = self.snapshot.stale_events.saturating_add(1);
    }
    fn live(&self) -> bool {
        matches!(self.snapshot.state, State::Establishing | State::Connected)
    }
    fn session(&self, session: Uuid) -> Result<(), Error> {
        if self.snapshot.session == Some(session) {
            Ok(())
        } else {
            Err(Error::WrongSession)
        }
    }
    fn check_owner(&self, owner: Uuid) -> Result<(), Error> {
        if self.owner == Some(owner) {
            Ok(())
        } else {
            Err(Error::WrongOwner)
        }
    }
}
