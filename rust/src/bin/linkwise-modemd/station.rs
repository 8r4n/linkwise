//! Native audio for the two-way modem and the receive diagnostics service.
use std::{collections::VecDeque, io, net::Ipv4Addr, ops::Range, path::Path, time::Duration};

use linkwise::{
    audio::{
        device::Capture,
        playback::{self, Playback, Progress},
    },
    modem::{bw500_waveform::Waveform, receiver::Receiver, waveform::BurstDetector},
};
use serde::Serialize;
use tokio::{io::AsyncWriteExt, net::TcpListener, sync::watch};

use super::setup::{AudioDevice, Station};

#[derive(Clone, Default, Serialize)]
struct Status {
    ready: bool,
    received_frames: u64,
    last_level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exact_fixture_matches: Option<u64>,
}

pub async fn serve(
    path: &Path,
    service_stop: impl std::future::Future<Output = ()>,
    on_bound: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    log(false, "Starting Linkwise modem station");
    let station = Station::load(path)?;
    if let Some(bandwidth) = station.bandwidth {
        let (stop, stopped) = watch::channel(false);
        return shutdown(
            stop_signal(service_stop),
            stop,
            super::virtual_host::serve_native(&station, path, bandwidth, stopped, on_bound),
        )
        .await;
    }
    let waveform = station.waveform(path)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, station.command_port))
        .await
        .map_err(|error| format!("Bind station status port {}: {error}", station.command_port))?;
    on_bound()?;
    let (stop, stopped) = watch::channel(false);
    #[cfg(windows)]
    let exact_fixture_matches = station.verify_bw500_fixture.unwrap_or(false).then_some(0);
    #[cfg(not(windows))]
    let exact_fixture_matches = None;
    let (status, current) = watch::channel(Status {
        exact_fixture_matches,
        ..Status::default()
    });
    let mut worker =
        tokio::task::spawn_blocking(move || receive(station.input, waveform, stopped, status));
    let mut signal = std::pin::pin!(stop_signal(service_stop));

    let result = loop {
        tokio::select! {
            result = &mut signal => break result,
            result = &mut worker => return Err(format!("Capture worker stopped: {result:?}")),
            incoming = listener.accept() => {
                let (mut socket, _) = match incoming {
                    Ok(connection) => connection,
                    Err(error) => break Err(format!("Accept station status connection: {error}")),
                };
                let mut response = serde_json::to_vec(&*current.borrow())
                    .expect("status fields are serializable");
                response.push(b'\n');
                tokio::select! {
                    result = &mut signal => break result,
                    _ = tokio::time::timeout(Duration::from_millis(200), socket.write_all(&response)) => {}
                }
            }
        }
    };
    drop(listener);
    notify_manager("STOPPING=1\nSTATUS=Stopping");
    stop.send_replace(true);
    tokio::time::timeout(Duration::from_secs(8), worker)
        .await
        .map_err(|_| "Audio worker did not stop within eight seconds")?
        .map_err(|error| format!("Join capture worker: {error}"))?;
    result
}

async fn stop_signal(service_stop: impl std::future::Future<Output = ()>) -> Result<(), String> {
    tokio::select! {
        biased;
        _ = service_stop => Ok(()),
        result = linkwise::signals::stop() => result.map(|_| ()).map_err(|error| error.to_string()),
    }
}

pub(super) async fn shutdown(
    signal: impl std::future::Future<Output = Result<(), String>>,
    stop: watch::Sender<bool>,
    station: impl std::future::Future<Output = Result<(), String>>,
) -> Result<(), String> {
    let mut station = std::pin::pin!(station);
    tokio::select! {
        biased;
        result = signal => result?,
        result = &mut station => return result,
    }
    notify_manager("STOPPING=1\nSTATUS=Stopping");
    stop.send_replace(true);
    match tokio::time::timeout(Duration::from_secs(3), station).await {
        Ok(result) => result,
        Err(_) => {
            log(
                true,
                "Modem shutdown deadline elapsed; closing local resources",
            );
            Ok(())
        }
    }
}

const AUDIO_RATE: usize = 48_000;
const BURST_EDGE: usize = AUDIO_RATE / 10;
const HISTORY_LIMIT: usize = AUDIO_RATE * 6 + 2 * BURST_EDGE;

pub(super) struct NativeAudio {
    capture: Capture,
    output_name: String,
    output_channel: u16,
    last_sample: tokio::time::Instant,
    detector: BurstDetector,
    history: VecDeque<f32>,
    position: u64,
    completed: VecDeque<Range<u64>>,
}

impl NativeAudio {
    pub(super) async fn open(input: &AudioDevice, output: &AudioDevice) -> io::Result<Self> {
        let capture = Capture::start(Some(&input.name), input.channel)
            .map_err(|error| io::Error::other(format!("Open input {:?}: {error}", input.name)))?;
        let mut audio = Self {
            capture,
            output_name: output.name.clone(),
            output_channel: output.channel,
            last_sample: tokio::time::Instant::now(),
            detector: BurstDetector::new(AUDIO_RATE as u32),
            history: VecDeque::with_capacity(HISTORY_LIMIT),
            position: 0,
            completed: VecDeque::new(),
        };
        playback::check_output(&output.name, output.channel).map_err(|error| {
            io::Error::other(format!("Check output {:?}: {error}", output.name))
        })?;
        while audio.capture_block()?.is_none() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Ok(audio)
    }

    fn capture_block(&mut self) -> io::Result<Option<linkwise::audio::device::Block>> {
        let block = self
            .capture
            .read(Duration::ZERO)
            .map_err(io::Error::other)?;
        if block.is_some() {
            self.last_sample = tokio::time::Instant::now();
        } else if self.last_sample.elapsed() >= Duration::from_secs(2) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "No input samples for two seconds",
            ));
        }
        Ok(block)
    }

    pub(super) async fn read(&mut self) -> io::Result<Vec<f32>> {
        loop {
            while let Some(candidate) = self.completed.front() {
                let end = candidate.end + BURST_EDGE as u64;
                if end > self.position {
                    break;
                }
                let base = self.position - self.history.len() as u64;
                let start = candidate.start.saturating_sub(BURST_EDGE as u64).max(base);
                let valid = candidate.start >= base && end - start <= HISTORY_LIMIT as u64;
                self.completed.pop_front();
                if valid {
                    return Ok(self
                        .history
                        .range((start - base) as usize..(end - base) as usize)
                        .copied()
                        .collect());
                }
            }

            if let Some(block) = self.capture_block()? {
                let discard =
                    (self.history.len() + block.samples.len()).saturating_sub(HISTORY_LIMIT);
                self.history.drain(..discard);
                self.history.extend(block.samples);
                self.position += block.samples.len() as u64;
                self.detector.push(&block.samples, |candidate| {
                    self.completed.push_back(candidate)
                });
            } else {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }

    pub(super) async fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        // Give the peer time to finish its output and resume capture.
        let mut burst = vec![0.0; AUDIO_RATE / 5];
        burst.extend_from_slice(samples);
        burst.resize(burst.len() + BURST_EDGE, 0.0);
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs_f64(burst.len() as f64 / AUDIO_RATE as f64)
            + Duration::from_secs(2);
        let mut playback =
            Playback::start(&self.output_name, self.output_channel, burst).map_err(|error| {
                io::Error::other(format!("Open output {:?}: {error}", self.output_name))
            })?;
        // Ignore input during our own burst, including samples queued before
        // transmission. The stream remains open across half-duplex turns.
        loop {
            while self.capture_block()?.is_some() {}
            if playback.progress().map_err(io::Error::other)? == Progress::EstimateElapsed {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Audio playback did not finish",
                ));
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        self.detector = BurstDetector::new(AUDIO_RATE as u32);
        self.history.clear();
        self.completed.clear();
        self.position = 0;
        Ok(())
    }
}

fn receive(
    input: AudioDevice,
    waveform: Option<Waveform>,
    stop: watch::Receiver<bool>,
    status: watch::Sender<Status>,
) {
    let mut delay = Duration::from_secs(1);
    let mut ready_once = false;
    let mut last_error = None;
    loop {
        if *stop.borrow() {
            return;
        }
        let error = match capture(&input, waveform.clone(), &stop, &status, &mut ready_once) {
            Ok(()) => return,
            Err(error) => error,
        };
        let was_ready = status.send_if_modified(|state| {
            let was_ready = state.ready;
            state.ready = false;
            was_ready
        });
        if was_ready {
            delay = Duration::from_secs(1);
        }
        eprintln!(
            "Input {:?} channel {} degraded: {error}; retry in {}s",
            input.name,
            input.channel,
            delay.as_secs()
        );
        if was_ready || last_error.as_deref() != Some(error.as_str()) {
            notify_manager(&format!("STATUS=Degraded: {error}"));
            last_error = Some(error);
        }
        for _ in 0..delay.as_millis() / 100 {
            if *stop.borrow() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        delay = (delay * 2).min(Duration::from_secs(30));
    }
}

fn capture(
    input: &AudioDevice,
    waveform: Option<Waveform>,
    stop: &watch::Receiver<bool>,
    status: &watch::Sender<Status>,
    ready_once: &mut bool,
) -> Result<(), String> {
    let capture =
        Capture::start(Some(&input.name), input.channel).map_err(|error| error.to_string())?;
    let mut receiver = waveform
        .map(Receiver::with_level4)
        .unwrap_or_default()
        .with_pilot_acquisition();
    let verify_fixture = status.borrow().exact_fixture_matches.is_some();
    let mut last_sample = std::time::Instant::now();
    while !*stop.borrow() {
        let Some(block) = capture
            .read(Duration::from_millis(100))
            .map_err(|error| error.to_string())?
        else {
            if last_sample.elapsed() >= Duration::from_secs(2) {
                return Err("no samples for two seconds".into());
            }
            continue;
        };
        last_sample = std::time::Instant::now();
        let mut frames = 0;
        let mut matches = 0;
        let mut last_level = None;
        receiver
            .push(&block.samples, |reception| {
                frames += reception.frames.len() as u64;
                if verify_fixture {
                    matches += reception
                        .frames
                        .iter()
                        .filter(|frame| {
                            frame.level == 2
                                && frame.frame.control == 0x99
                                && frame.frame.payload.iter().copied().eq(0u8..22)
                        })
                        .count() as u64;
                }
                if let Some(frame) = reception.frames.last() {
                    last_level = Some(frame.level);
                }
            })
            .map_err(|error| error.to_string())?;
        if !status.borrow().ready {
            eprintln!(
                "Receive-only station ready on input {:?} channel {}",
                input.name, input.channel
            );
            if !*ready_once {
                notify_manager("READY=1\nSTATUS=Receiving BW500");
                *ready_once = true;
            } else {
                notify_manager("STATUS=Receiving BW500");
            }
            status.send_modify(|state| state.ready = true);
        }
        if frames != 0 {
            status.send_modify(|state| {
                state.received_frames = state.received_frames.saturating_add(frames);
                state.last_level = last_level;
                if let Some(count) = &mut state.exact_fixture_matches {
                    *count = count.saturating_add(matches);
                }
            });
        }
    }
    Ok(())
}

/// Foreground output is stderr. Installed services also use their native log.
pub(super) fn log(error: bool, message: &str) {
    if !error && std::env::var("LINKWISE_LOG_LEVEL").as_deref() == Ok("error") {
        return;
    }
    eprintln!("linkwise-modemd: {message}");
    #[cfg(target_os = "macos")]
    if std::env::var_os("LINKWISE_UNIFIED_LOG").is_some()
        && let Ok(message) = std::ffi::CString::new(message)
    {
        let level = if error {
            libc::LOG_ERR
        } else {
            libc::LOG_NOTICE
        };
        // The format is fixed and message data is passed separately.
        unsafe { libc::syslog(level, c"%s".as_ptr(), message.as_ptr()) };
    }
    #[cfg(windows)]
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--windows-service")) {
        super::windows_service::log_event(message, error);
    }
}

#[cfg(target_os = "linux")]
pub(super) fn notify_manager(message: &str) {
    use std::os::{
        linux::net::SocketAddrExt,
        unix::{
            ffi::OsStrExt,
            net::{SocketAddr, UnixDatagram},
        },
    };

    if let Some(name) = std::env::var_os("NOTIFY_SOCKET") {
        let address = match name.as_bytes().strip_prefix(b"@") {
            Some(name) => SocketAddr::from_abstract_name(name),
            None => SocketAddr::from_pathname(name),
        };
        let result = address.and_then(|address| {
            UnixDatagram::unbound()?.send_to_addr(message.as_bytes(), &address)
        });
        if let Err(error) = result {
            eprintln!("Notify systemd: {error}");
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn notify_manager(message: &str) {
    if std::env::var_os("LINKWISE_UNIFIED_LOG").is_some()
        && let Ok(message) = std::ffi::CString::new(message)
    {
        // The fixed format keeps message data out of the format string.
        unsafe { libc::syslog(libc::LOG_NOTICE, c"%s".as_ptr(), message.as_ptr()) };
    }
}

#[cfg(windows)]
pub(super) fn notify_manager(message: &str) {
    if std::env::args().nth(1).as_deref() == Some("--windows-service") {
        super::windows_service::log_event(message, message.contains("Degraded"));
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(super) fn notify_manager(_: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn shutdown_notifies_station_and_drops_it_at_deadline() {
        struct Cleanup<'a>(&'a std::cell::Cell<bool>);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }

        let notified = std::cell::Cell::new(false);
        let cleaned_up = std::cell::Cell::new(false);
        let (stop, mut stopped) = watch::channel(false);
        let start = tokio::time::Instant::now();
        shutdown(std::future::ready(Ok(())), stop, async {
            let _cleanup = Cleanup(&cleaned_up);
            stopped.wait_for(|stopping| *stopping).await.unwrap();
            notified.set(true);
            std::future::pending().await
        })
        .await
        .unwrap();
        assert!(notified.get());
        assert!(cleaned_up.get());
        assert_eq!(start.elapsed(), Duration::from_secs(3));
    }
}
