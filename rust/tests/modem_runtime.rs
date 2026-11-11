use linkwise::modem::{
    arq::Answer,
    runtime::{
        Admission, Error, Failure, Limits, Runtime, State, TransmissionId,
        device::{DeviceEvent, ExecutionDevice, SimulatedDevice, Transmission},
    },
};
use std::time::Duration;
use uuid::Uuid;

type Modem = Runtime<SimulatedDevice>;
const ZERO: Duration = Duration::ZERO;
fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}
fn drain(m: &mut Modem) {
    while m.pop_event().is_some() {}
}
fn admission(owner: Uuid, local: &str, remote: Option<&str>) -> Admission {
    Admission {
        owner,
        operation: Uuid::new_v4(),
        local_call: local.into(),
        remote_call: remote.map(str::to_owned),
        lease_expires: secs(100),
    }
}
fn finish(m: &mut Modem, now: Duration) -> (TransmissionId, Transmission) {
    let burst = m.device_mut().take_transmission().unwrap();
    let presented = m.device().presented().unwrap();
    m.device_event(now, presented).unwrap();
    assert!(m.device().release_requested());
    let released = m.device_mut().confirm_release().unwrap();
    m.device_event(now, released).unwrap();
    burst
}
fn connected(limits: Limits) -> (Modem, Uuid, Uuid) {
    let owner = Uuid::new_v4();
    let mut a = Modem::new(limits, SimulatedDevice::default()).unwrap();
    let mut b = Modem::new(limits, SimulatedDevice::default()).unwrap();
    let sa = a
        .begin(ZERO, admission(owner, "W9SSJ", Some("W1AW")))
        .unwrap();
    let sb = b
        .begin(ZERO, admission(Uuid::new_v4(), "W1AW", None))
        .unwrap();
    for caller_turn in [true, false, true, false, true] {
        let (from, to, sid) = if caller_turn {
            (&mut a, &mut b, sb)
        } else {
            (&mut b, &mut a, sa)
        };
        let (_, Transmission::Establishment(burst)) = finish(from, ZERO) else {
            panic!("handshake")
        };
        to.receive_handshake(ZERO, sid, burst).unwrap();
        drain(from);
        drain(to);
    }
    assert_eq!(a.snapshot().state, State::Connected);
    assert_eq!(b.snapshot().state, State::Connected);
    (a, owner, sa)
}

#[test]
fn two_owners_establish_then_account_binary_blocks_once() {
    let (mut m, owner, session) = connected(Limits::default());
    m.enqueue(ZERO, owner, session, vec![0, 255, 13, 10])
        .unwrap();
    m.enqueue(ZERO, owner, session, vec![128, 0]).unwrap();
    let b = m.snapshot().bytes;
    assert_eq!((b.accepted, b.pending, b.outstanding), (6, 2, 4));
    let (id, Transmission::Data(bytes)) = finish(&mut m, ZERO) else {
        panic!()
    };
    assert_eq!(bytes, vec![0, 255, 13, 10]);
    m.answer(ZERO, id, Answer::Delivered).unwrap();
    m.answer(ZERO, id, Answer::Delivered).unwrap();
    assert_eq!(m.snapshot().bytes.acknowledged, 4);
    let (id, _) = finish(&mut m, ZERO);
    m.answer(ZERO, id, Answer::Delivered).unwrap();
    let b = m.snapshot().bytes;
    assert!(b.balanced());
    assert_eq!((b.accepted, b.acknowledged, b.transmit_attempts), (6, 6, 2));
}

#[test]
fn presentation_alone_does_not_complete_transmission() {
    let (mut m, owner, session) = connected(Limits::default());
    m.enqueue(ZERO, owner, session, vec![1]).unwrap();
    let (id, _) = m.device_mut().take_transmission().unwrap();
    m.device_event(ZERO, DeviceEvent::Presented(id)).unwrap();
    m.answer(ZERO, id, Answer::Delivered).unwrap();
    assert_eq!(m.snapshot().bytes.acknowledged, 0);
    assert!(m.snapshot().release_pending);
    let release = m.device_mut().confirm_release().unwrap();
    m.device_event(ZERO, release).unwrap();
    m.answer(ZERO, id, Answer::Delivered).unwrap();
    assert_eq!(m.snapshot().bytes.acknowledged, 1);
}

#[test]
fn missing_answer_queries_instead_of_repeating_data() {
    let (mut m, owner, session) = connected(Limits::default());
    m.enqueue(ZERO, owner, session, vec![7, 8]).unwrap();
    let (old, _) = finish(&mut m, ZERO);
    m.tick(secs(5)).unwrap();
    let (query, tx) = finish(&mut m, secs(5));
    assert!(matches!(tx, Transmission::StateQuery));
    m.answer(secs(5), old, Answer::Delivered).unwrap();
    assert_eq!(m.snapshot().bytes.acknowledged, 0);
    m.answer(secs(5), query, Answer::Delivered).unwrap();
    assert_eq!(m.snapshot().bytes.acknowledged, 2);
    assert_eq!(m.snapshot().bytes.transmit_attempts, 1);
}

#[test]
fn decoded_negative_retries_without_double_counting_accepted_bytes() {
    let (mut m, owner, session) = connected(Limits {
        max_retries: 1,
        ..Limits::default()
    });
    m.enqueue(ZERO, owner, session, vec![9]).unwrap();
    let (first, _) = finish(&mut m, ZERO);
    m.answer(ZERO, first, Answer::NotDelivered).unwrap();
    let (retry, tx) = finish(&mut m, ZERO);
    assert!(matches!(tx, Transmission::Data(bytes) if bytes == vec![9]));
    m.answer(ZERO, retry, Answer::NotDelivered).unwrap();
    assert_eq!(
        m.snapshot().failure,
        Some(Failure::Arq(linkwise::modem::arq::Failure::RetryLimit))
    );
    let b = m.snapshot().bytes;
    assert_eq!((b.accepted, b.unresolved, b.transmit_attempts), (1, 1, 2));
    assert!(b.balanced());
}

#[test]
fn bounded_enqueue_refuses_atomically_and_does_not_replace_owner() {
    let (mut m, owner, session) = connected(Limits {
        max_buffer_bytes: 3,
        ..Limits::default()
    });
    m.enqueue(ZERO, owner, session, vec![1, 2]).unwrap();
    let before = m.snapshot().bytes;
    assert_eq!(
        m.enqueue(ZERO, owner, session, vec![3, 4]),
        Err((Error::Backpressure, vec![3, 4]))
    );
    assert_eq!(
        m.enqueue(ZERO, Uuid::new_v4(), session, vec![3]),
        Err((Error::WrongOwner, vec![3]))
    );
    assert_eq!(
        m.begin(ZERO, admission(owner, "W9SSJ", Some("W1AW"))),
        Err(Error::Busy)
    );
    assert_eq!(m.snapshot().bytes, before);
}

#[test]
fn abort_is_idempotent_and_late_callbacks_cannot_acknowledge() {
    let (mut m, owner, session) = connected(Limits::default());
    m.enqueue(ZERO, owner, session, vec![1, 2]).unwrap();
    let (id, _) = m.device_mut().take_transmission().unwrap();
    m.abort(ZERO, owner, session).unwrap();
    m.abort(ZERO, owner, session).unwrap();
    m.device_event(ZERO, DeviceEvent::Presented(id)).unwrap();
    m.answer(ZERO, id, Answer::Delivered).unwrap();
    assert_eq!(m.snapshot().state, State::Aborted);
    assert_eq!(m.snapshot().bytes.discarded, 2);
    assert!(m.snapshot().bytes.balanced());
    drain(&mut m);
    assert_eq!(
        m.begin(ZERO, admission(owner, "W9SSJ", Some("W1AW"))),
        Err(Error::Busy)
    );
    let event = m.device_mut().confirm_release().unwrap();
    m.device_event(ZERO, event).unwrap();
    let new = m
        .begin(ZERO, admission(owner, "W9SSJ", Some("W1AW")))
        .unwrap();
    assert_ne!(session, new);
    m.device_event(ZERO, DeviceEvent::Released(id)).unwrap();
    assert_eq!(m.snapshot().state, State::Establishing);
    assert!(m.device().is_keyed());
}

#[test]
fn watchdog_releases_without_session_progress_and_keeps_unresolved_bytes() {
    let (mut m, owner, session) = connected(Limits::default());
    m.enqueue(ZERO, owner, session, vec![1, 2]).unwrap();
    m.enqueue(ZERO, owner, session, vec![3]).unwrap();
    // The execution device releases even without invoking Runtime::tick.
    let event = m.device_mut().poll(secs(15)).unwrap();
    assert!(!m.device().is_keyed());
    m.device_event(secs(15), event).unwrap();
    let s = m.snapshot();
    assert_eq!(s.failure, Some(Failure::Watchdog));
    assert_eq!((s.bytes.unresolved, s.bytes.pending), (2, 1));
    assert!(!s.release_pending);
    assert!(s.bytes.balanced());
    drain(&mut m);
    assert_eq!(
        m.begin(secs(15), admission(owner, "W9SSJ", Some("W1AW"))),
        Err(Error::Busy)
    );
    m.abort(secs(15), owner, session).unwrap();
    assert_eq!(m.snapshot().bytes.discarded, 3);
}

#[test]
fn capture_fault_invalidates_playback_and_priority_abort_survives_full_events() {
    let (mut m, owner, session) = connected(Limits {
        max_events: 16,
        ..Limits::default()
    });
    m.enqueue(ZERO, owner, session, vec![1]).unwrap();
    let (id, _) = m.device_mut().take_transmission().unwrap();
    m.capture_discontinuity(ZERO, session).unwrap();
    m.device_event(ZERO, DeviceEvent::Presented(id)).unwrap();
    assert_eq!(m.snapshot().failure, Some(Failure::CaptureDiscontinuity));
    assert_eq!(m.snapshot().bytes.unresolved, 1);
    assert!(m.device().release_requested());
    m.abort(ZERO, owner, session).unwrap();
    assert_eq!(m.snapshot().bytes.discarded, 1);
}

#[test]
fn event_overflow_faults_and_requests_release_without_losing_accounting() {
    let mut m = Modem::new(
        Limits {
            max_events: 1,
            ..Limits::default()
        },
        SimulatedDevice::default(),
    )
    .unwrap();
    let owner = Uuid::new_v4();
    let session = m
        .begin(ZERO, admission(owner, "W9SSJ", Some("W1AW")))
        .unwrap();
    assert_eq!(m.snapshot().failure, Some(Failure::EventOverflow));
    assert!(m.device().release_requested());
    m.abort(ZERO, owner, session).unwrap();
    assert_eq!(m.snapshot().state, State::Aborted);
    assert!(m.snapshot().bytes.balanced());
    assert!(m.pop_event().is_some());
    assert!(m.pop_event().is_none());
}

#[test]
fn lease_expiry_and_late_completion_are_terminal() {
    let owner = Uuid::new_v4();
    let mut m = Modem::new(Limits::default(), SimulatedDevice::default()).unwrap();
    let mut grant = admission(owner, "W9SSJ", Some("W1AW"));
    grant.lease_expires = secs(1);
    m.begin(ZERO, grant).unwrap();
    let event = m.device().presented().unwrap();
    m.device_event(secs(1), event).unwrap();
    assert_eq!(m.snapshot().state, State::Faulted);
    assert!(!m.device().is_keyed());
    assert_eq!(m.tick(ZERO), Err(Error::StaleTime));
}

#[test]
fn invalid_admission_does_not_mutate_or_key_device() {
    let mut m = Modem::new(Limits::default(), SimulatedDevice::default()).unwrap();
    let owner = Uuid::new_v4();
    assert_eq!(
        m.begin(ZERO, admission(owner, "W9SSJ", Some("W9SSJ"))),
        Err(Error::InvalidCall)
    );
    assert_eq!(m.snapshot().state, State::Idle);
    assert!(!m.device().is_keyed());
}

#[test]
fn capture_epoch_changes_and_sample_gaps_fault_instead_of_joining_streams() {
    for change_epoch in [false, true] {
        let (mut m, _, session) = connected(Limits::default());
        let epoch = Uuid::new_v4();
        m.capture_range(ZERO, session, epoch, 100, 1124).unwrap();
        m.capture_range(ZERO, session, epoch, 1124, 2148).unwrap();
        let next_epoch = if change_epoch { Uuid::new_v4() } else { epoch };
        let next_start = if change_epoch { 2148 } else { 2149 };
        m.capture_range(ZERO, session, next_epoch, next_start, 3172)
            .unwrap();
        assert_eq!(m.snapshot().failure, Some(Failure::CaptureDiscontinuity));
    }
}

#[test]
fn exhausted_queries_preserve_payload_until_explicit_discard() {
    let limits = Limits {
        max_queries: 1,
        ..Limits::default()
    };
    let (mut m, owner, session) = connected(limits);
    m.enqueue(ZERO, owner, session, vec![4, 5, 6]).unwrap();
    finish(&mut m, ZERO);
    m.tick(secs(5)).unwrap();
    finish(&mut m, secs(5));
    m.tick(secs(10)).unwrap();
    assert_eq!(
        m.snapshot().failure,
        Some(Failure::Arq(linkwise::modem::arq::Failure::AnswerTimedOut))
    );
    assert_eq!(m.snapshot().bytes.unresolved, 3);
    assert_eq!(
        m.retained_blocks(owner, session)
            .unwrap()
            .collect::<Vec<_>>(),
        vec![&[4, 5, 6][..]]
    );
    assert!(matches!(
        m.retained_blocks(Uuid::new_v4(), session),
        Err(Error::WrongOwner)
    ));
    assert_eq!(m.snapshot().bytes.discarded, 0);
    assert!(m.snapshot().bytes.balanced());
    m.abort(secs(10), owner, session).unwrap();
    assert_eq!(m.snapshot().bytes.discarded, 3);
}

#[test]
fn total_session_deadline_applies_even_without_pending_transmission() {
    let (mut m, _, _) = connected(Limits {
        session_timeout: secs(2),
        ..Limits::default()
    });
    m.tick(secs(2)).unwrap();
    assert_eq!(m.snapshot().failure, Some(Failure::SessionExpired));
    assert!(!m.device().is_keyed());
}

#[test]
fn handshake_receive_deadline_does_not_report_connected() {
    let mut m = Modem::new(
        Limits {
            handshake_timeout: secs(2),
            ..Limits::default()
        },
        SimulatedDevice::default(),
    )
    .unwrap();
    m.begin(ZERO, admission(Uuid::new_v4(), "W9SSJ", Some("W1AW")))
        .unwrap();
    finish(&mut m, ZERO);
    m.tick(secs(2)).unwrap();
    assert_eq!(
        m.snapshot().failure,
        Some(Failure::Handshake(
            linkwise::modem::handshake::Failure::TimedOut
        ))
    );
    assert_eq!(m.snapshot().state, State::Faulted);
    assert_eq!(m.snapshot().peer, None);
}

#[test]
fn failed_release_cannot_complete_handshake_and_watchdog_still_releases() {
    struct RefusesRelease(SimulatedDevice);
    impl ExecutionDevice for RefusesRelease {
        fn start(
            &mut self,
            id: TransmissionId,
            tx: Transmission,
            deadline: Duration,
        ) -> Result<(), linkwise::modem::runtime::device::DeviceError> {
            self.0.start(id, tx, deadline)
        }
        fn release(
            &mut self,
            _: TransmissionId,
        ) -> Result<(), linkwise::modem::runtime::device::DeviceError> {
            Err(linkwise::modem::runtime::device::DeviceError)
        }
        fn poll(&mut self, now: Duration) -> Option<DeviceEvent> {
            self.0.poll(now)
        }
    }
    let mut m = Runtime::new(
        Limits::default(),
        RefusesRelease(SimulatedDevice::default()),
    )
    .unwrap();
    m.begin(ZERO, admission(Uuid::new_v4(), "W9SSJ", Some("W1AW")))
        .unwrap();
    let event = m.device().0.presented().unwrap();
    m.device_event(ZERO, event).unwrap();
    assert_eq!(m.snapshot().failure, Some(Failure::Device));
    assert!(m.device().0.is_keyed());
    assert!(m.snapshot().release_pending);
    m.tick(secs(15)).unwrap();
    assert!(!m.device().0.is_keyed());
    assert!(!m.snapshot().release_pending);
    assert_eq!(m.snapshot().state, State::Faulted);
}

#[test]
fn many_blocks_retries_and_duplicate_answers_preserve_conservation() {
    let (mut m, owner, session) = connected(Limits::default());
    let mut accepted = 0;
    for size in 1..=64 {
        let bytes: Vec<_> = (0..size).map(|n| (n * 37) as u8).collect();
        m.enqueue(ZERO, owner, session, bytes.clone()).unwrap();
        let (mut id, tx) = finish(&mut m, ZERO);
        assert!(matches!(tx, Transmission::Data(data) if data == bytes));
        if size % 3 == 0 {
            m.answer(ZERO, id, Answer::NotDelivered).unwrap();
            (id, _) = finish(&mut m, ZERO);
        }
        m.answer(ZERO, id, Answer::Delivered).unwrap();
        m.answer(ZERO, id, Answer::Delivered).unwrap();
        accepted += size;
        assert!(m.snapshot().bytes.balanced());
        assert_eq!(m.snapshot().bytes.acknowledged, accepted);
        drain(&mut m);
    }
}
