//! Clock-driven owner of a portable modem runtime.
use std::time::Duration;

use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{self, Instant, MissedTickBehavior},
};
use uuid::Uuid;

use super::{
    Admission, Error, Event, Failure, Runtime, Snapshot, TransmissionId, arq,
    device::{DeviceEvent, ExecutionDevice, SimulatedDevice, Transmission},
    handshake,
};

const POLL_INTERVAL: Duration = Duration::from_millis(5);

type Job<D> = Box<dyn FnOnce(&mut Runtime<D>, Duration) + Send>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestError {
    Busy,
    Stopped,
}

pub struct Handle<D: ExecutionDevice> {
    commands: mpsc::Sender<Job<D>>,
    priority: mpsc::Sender<Job<D>>,
    epoch: Instant,
    max_buffer_bytes: usize,
}

impl<D: ExecutionDevice> Clone for Handle<D> {
    fn clone(&self) -> Self {
        Self {
            commands: self.commands.clone(),
            priority: self.priority.clone(),
            epoch: self.epoch,
            max_buffer_bytes: self.max_buffer_bytes,
        }
    }
}

/// Start with an idle device on the current Tokio executor. Device methods
/// must return promptly. The driver still owns its independent release timer.
/// Dropping all handles faults a live session, requests release and returns
/// the runtime after release evidence or one transmit timeout.
pub fn spawn<D: ExecutionDevice + Send + 'static>(
    limits: super::Limits,
    device: D,
    capacity: std::num::NonZeroUsize,
) -> Result<(Handle<D>, JoinHandle<Runtime<D>>), Error> {
    if capacity.get() > tokio::sync::Semaphore::MAX_PERMITS {
        return Err(Error::InvalidConfig);
    }
    let mut runtime = Runtime::new(limits, device)?;
    let epoch = Instant::now();
    let (commands, mut normal) = mpsc::channel::<Job<D>>(capacity.get());
    let (priority, mut urgent) = mpsc::channel::<Job<D>>(capacity.get());
    let task = tokio::spawn(async move {
        let mut timer = time::interval_at(epoch, POLL_INTERVAL);
        timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            if normal.is_closed() && urgent.is_closed() {
                break;
            }
            tokio::select! {
                biased;
                _ = timer.tick() => {
                    runtime.tick(epoch.elapsed()).expect("monotonic runner clock");
                }
                Some(job) = urgent.recv() => job(&mut runtime, epoch.elapsed()),
                Some(job) = normal.recv() => job(&mut runtime, epoch.elapsed()),
            }
        }

        runtime.fault(Failure::RunnerStopped);
        runtime
            .tick(epoch.elapsed())
            .expect("monotonic runner clock");
        let shutdown = Instant::now();
        while runtime.active.is_some() && shutdown.elapsed() < limits.transmit_timeout {
            timer.tick().await;
            runtime
                .tick(epoch.elapsed())
                .expect("monotonic runner clock");
        }
        runtime
    });
    Ok((
        Handle {
            commands,
            priority,
            epoch,
            max_buffer_bytes: limits.max_buffer_bytes,
        },
        task,
    ))
}

impl<D: ExecutionDevice + Send + 'static> Handle<D> {
    /// Lease expiries are relative to the runner's monotonic epoch.
    pub fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    async fn request<T: Send + 'static>(
        &self,
        priority: bool,
        run: impl FnOnce(&mut Runtime<D>, Duration) -> T + Send + 'static,
    ) -> Result<T, RequestError> {
        let (reply, result) = oneshot::channel();
        let job: Job<D> = Box::new(move |runtime, now| {
            if priority || !reply.is_closed() {
                let _ = reply.send(run(runtime, now));
            }
        });
        if priority {
            self.priority
                .send(job)
                .await
                .map_err(|_| RequestError::Stopped)?;
        } else {
            self.commands.try_send(job).map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => RequestError::Busy,
                mpsc::error::TrySendError::Closed(_) => RequestError::Stopped,
            })?;
        }
        result.await.map_err(|_| RequestError::Stopped)
    }

    pub async fn begin(&self, admission: Admission) -> Result<Result<Uuid, Error>, RequestError> {
        self.request(false, move |runtime, now| runtime.begin(now, admission))
            .await
    }

    /// Borrow the input so callers retain their bytes if either queue refuses it.
    pub async fn enqueue(
        &self,
        owner: Uuid,
        session: Uuid,
        bytes: &[u8],
    ) -> Result<Result<(), Error>, RequestError> {
        if bytes.is_empty() || bytes.len() > self.max_buffer_bytes {
            return Ok(Err(Error::Backpressure));
        }
        let bytes = bytes.to_vec();
        self.request(false, move |runtime, now| {
            runtime
                .enqueue(now, owner, session, bytes)
                .map_err(|(error, _)| error)
        })
        .await
    }

    pub async fn receive_handshake(
        &self,
        session: Uuid,
        burst: handshake::Burst,
    ) -> Result<Result<(), Error>, RequestError> {
        self.request(false, move |runtime, now| {
            runtime.receive_handshake(now, session, burst)
        })
        .await
    }

    pub async fn answer(
        &self,
        id: TransmissionId,
        answer: arq::Answer,
    ) -> Result<Result<(), Error>, RequestError> {
        self.request(false, move |runtime, now| runtime.answer(now, id, answer))
            .await
    }

    pub async fn observe_device(
        &self,
        event: DeviceEvent,
    ) -> Result<Result<(), Error>, RequestError> {
        self.request(true, move |runtime, now| runtime.device_event(now, event))
            .await
    }

    pub async fn capture_range(
        &self,
        session: Uuid,
        epoch: Uuid,
        start: u64,
        end: u64,
    ) -> Result<Result<(), Error>, RequestError> {
        self.request(true, move |runtime, now| {
            runtime.capture_range(now, session, epoch, start, end)
        })
        .await
    }

    pub async fn capture_discontinuity(
        &self,
        session: Uuid,
    ) -> Result<Result<(), Error>, RequestError> {
        self.request(true, move |runtime, now| {
            runtime.capture_discontinuity(now, session)
        })
        .await
    }

    pub async fn abort(
        &self,
        owner: Uuid,
        session: Uuid,
    ) -> Result<Result<(), Error>, RequestError> {
        self.request(true, move |runtime, now| runtime.abort(now, owner, session))
            .await
    }

    pub async fn snapshot(&self) -> Result<Snapshot, RequestError> {
        self.request(false, |runtime, _| runtime.snapshot()).await
    }

    pub async fn pop_event(&self) -> Result<Option<Event>, RequestError> {
        self.request(false, |runtime, _| runtime.pop_event()).await
    }
}

impl Handle<SimulatedDevice> {
    /// Simulate presentation and release of one pending intent.
    pub async fn complete_simulated_transmission(
        &self,
    ) -> Result<Result<Option<(TransmissionId, Transmission)>, Error>, RequestError> {
        self.request(true, |runtime, now| {
            runtime.tick(now)?;
            let Some(transmission) = runtime.device_mut().take_transmission() else {
                return Ok(None);
            };
            let event = runtime
                .device()
                .presented()
                .expect("active simulated transmission");
            runtime.device_event(now, event)?;
            if let Some(event) = runtime.device_mut().confirm_release() {
                runtime.device_event(now, event)?;
            }
            Ok(Some(transmission))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modem::runtime::{Limits, State};
    use std::num::NonZeroUsize;

    async fn begin(handle: &Handle<SimulatedDevice>) -> (Uuid, Uuid) {
        let owner = Uuid::new_v4();
        let session = handle
            .begin(Admission {
                owner,
                operation: Uuid::new_v4(),
                local_call: "W9SSJ".into(),
                remote_call: Some("W1AW".into()),
                lease_expires: Duration::from_secs(60),
            })
            .await
            .unwrap()
            .unwrap();
        (owner, session)
    }

    #[tokio::test(start_paused = true)]
    async fn watchdog_runs_without_commands_or_event_consumer() {
        let limits = Limits {
            transmit_timeout: Duration::from_millis(100),
            ..Limits::default()
        };
        let (handle, task) = spawn(limits, SimulatedDevice::default(), NonZeroUsize::MIN).unwrap();
        begin(&handle).await;
        time::sleep(Duration::from_millis(120)).await;
        let snapshot = handle.snapshot().await.unwrap();
        assert_eq!(snapshot.failure, Some(Failure::Watchdog));
        assert!(!snapshot.release_pending);
        drop(handle);
        assert!(!task.await.unwrap().device().is_keyed());
    }

    #[tokio::test(start_paused = true)]
    async fn abort_overtakes_a_full_normal_queue() {
        let (handle, task) = spawn(
            Limits::default(),
            SimulatedDevice::default(),
            NonZeroUsize::MIN,
        )
        .unwrap();
        let (owner, session) = begin(&handle).await;
        let (reply, state) = oneshot::channel();
        handle
            .commands
            .try_send(Box::new(move |runtime, _| {
                let _ = reply.send(runtime.snapshot().state);
            }))
            .unwrap();
        assert_eq!(handle.snapshot().await, Err(RequestError::Busy));
        handle.abort(owner, session).await.unwrap().unwrap();
        assert_eq!(state.await.unwrap(), State::Aborted);
        drop(handle);
        let runtime = task.await.unwrap();
        assert!(!runtime.device().is_keyed());
        assert!(!runtime.snapshot().release_pending);
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_handles_requests_release_and_waits_for_evidence() {
        let (handle, task) = spawn(
            Limits::default(),
            SimulatedDevice::default(),
            NonZeroUsize::MIN,
        )
        .unwrap();
        begin(&handle).await;
        drop(handle);
        let runtime = task.await.unwrap();
        assert_eq!(runtime.snapshot().failure, Some(Failure::RunnerStopped));
        assert!(!runtime.device().is_keyed());
        assert!(!runtime.snapshot().release_pending);
    }
}
