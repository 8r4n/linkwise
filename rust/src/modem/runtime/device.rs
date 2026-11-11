//! Device execution boundary. Transmissions are protocol intents, not PCM or
//! qualified on-air frames. A future waveform/device adapter must encode them.
use std::time::Duration;

use super::TransmissionId;
use crate::modem::handshake::Burst;

#[derive(Debug)]
pub enum Transmission {
    Establishment(Burst),
    Data(Vec<u8>),
    StateQuery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceEvent {
    Presented(TransmissionId),
    /// Release includes the adapter's qualified turnaround evidence.
    Released(TransmissionId),
    Failed(TransmissionId),
    /// Independent watchdog has released the device, not merely requested it.
    WatchdogReleased(TransmissionId),
}

impl DeviceEvent {
    pub fn id(self) -> TransmissionId {
        match self {
            Self::Presented(id)
            | Self::Released(id)
            | Self::Failed(id)
            | Self::WatchdogReleased(id) => id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceError;

pub trait ExecutionDevice {
    /// Accept one owned burst with an independently enforced release deadline.
    fn start(
        &mut self,
        id: TransmissionId,
        burst: Transmission,
        deadline: Duration,
    ) -> Result<(), DeviceError>;
    /// Idempotent, priority cancellation/release; acceptance is not confirmation.
    fn release(&mut self, id: TransmissionId) -> Result<(), DeviceError>;
    /// Poll autonomous device faults, including independent watchdog evidence.
    fn poll(&mut self, now: Duration) -> Option<DeviceEvent>;
}

/// Single-slot, virtual-time device. No audio, CAT or RF is accessed.
#[derive(Default)]
pub struct SimulatedDevice {
    active: Option<(TransmissionId, Duration)>,
    burst: Option<Transmission>,
    release_requested: bool,
    keyed: bool,
}

impl SimulatedDevice {
    pub fn take_transmission(&mut self) -> Option<(TransmissionId, Transmission)> {
        Some((self.active?.0, self.burst.take()?))
    }

    pub fn is_keyed(&self) -> bool {
        self.keyed
    }
    pub fn release_requested(&self) -> bool {
        self.release_requested
    }

    pub fn presented(&self) -> Option<DeviceEvent> {
        self.active.map(|(id, _)| DeviceEvent::Presented(id))
    }

    pub fn confirm_release(&mut self) -> Option<DeviceEvent> {
        if !self.release_requested {
            return None;
        }
        let (id, _) = self.active.take()?;
        self.keyed = false;
        self.burst = None;
        self.release_requested = false;
        Some(DeviceEvent::Released(id))
    }
}

impl ExecutionDevice for SimulatedDevice {
    fn start(
        &mut self,
        id: TransmissionId,
        burst: Transmission,
        deadline: Duration,
    ) -> Result<(), DeviceError> {
        if self.active.is_some() {
            return Err(DeviceError);
        }
        self.active = Some((id, deadline));
        self.burst = Some(burst);
        self.keyed = true;
        self.release_requested = false;
        Ok(())
    }

    fn release(&mut self, id: TransmissionId) -> Result<(), DeviceError> {
        if self.active.is_none() {
            return Ok(());
        }
        if self.active.is_some_and(|(current, _)| current != id) {
            return Err(DeviceError);
        }
        self.release_requested = true;
        self.burst = None;
        Ok(())
    }

    fn poll(&mut self, now: Duration) -> Option<DeviceEvent> {
        let (id, deadline) = self.active?;
        if now < deadline {
            return None;
        }
        self.active = None;
        self.burst = None;
        self.keyed = false;
        self.release_requested = false;
        Some(DeviceEvent::WatchdogReleased(id))
    }
}
