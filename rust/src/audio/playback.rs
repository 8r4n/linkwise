//! Optional native 48 kHz mono playback on one configured output channel.
//! CPAL provides a presentation estimate. Hardware qualification is needed
//! before using that estimate to authorize a modem turnaround.
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

use cpal::{
    FromSample, Sample, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

const RATE: u32 = 48_000;
const MAX_SAMPLES: usize = RATE as usize * 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Queuing,
    Scheduled { estimated_end: Instant },
    EstimateElapsed,
}

#[derive(Debug, thiserror::Error)]
#[error("audio output failed or provided an invalid presentation timestamp")]
pub struct PlaybackError;

/// Owns the stream. Dropping it stops further callbacks. Already queued device
/// buffers may still be in flight, so drop is not presentation evidence.
pub struct Playback {
    _stream: cpal::Stream,
    scheduled: Receiver<Instant>,
    estimated_end: Option<Instant>,
    failed: Arc<AtomicBool>,
}

pub fn output_devices() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    cpal::default_host()
        .output_devices()?
        .map(|device| device.name().map_err(Into::into))
        .collect()
}

impl Playback {
    /// Use a named device and channel. The entire burst is prepared before
    /// opening the stream. Idle frames and unselected channels are silent.
    pub fn start(
        name: &str,
        channel: u16,
        samples: Vec<f32>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if name.is_empty()
            || samples.is_empty()
            || samples.len() > MAX_SAMPLES
            || samples.iter().any(|x| !(-1.0..=1.0).contains(x))
        {
            return Err(
                "playback requires a named device and 1 to 720000 normalized samples".into(),
            );
        }
        let host = cpal::default_host();
        let device = host
            .output_devices()?
            .find(|device| device.name().ok().as_deref() == Some(name))
            .ok_or("requested output device is unavailable")?;
        let supported = device
            .supported_output_configs()?
            .find(|config| {
                config.channels() > channel
                    && config.min_sample_rate().0 <= RATE
                    && config.max_sample_rate().0 >= RATE
                    && matches!(
                        config.sample_format(),
                        cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16
                    )
            })
            .ok_or("output device needs a supported f32/i16/u16 channel at 48 kHz")?
            .with_sample_rate(cpal::SampleRate(RATE));
        let config = supported.config();
        let (send, scheduled) = mpsc::sync_channel(1);
        let failed = Arc::new(AtomicBool::new(false));
        let mut cursor = 0;
        macro_rules! stream {
            ($sample:ty) => {{
                let callback_failed = failed.clone();
                let stream_failed = failed.clone();
                device.build_output_stream(
                    &config,
                    move |output: &mut [$sample], info| {
                        let now = Instant::now();
                        let channels = usize::from(config.channels);
                        if callback_failed.load(Ordering::Acquire) {
                            output.fill(<$sample>::from_sample(0.0));
                            return;
                        }
                        if output.len() % channels != 0 {
                            output.fill(<$sample>::from_sample(0.0));
                            callback_failed.store(true, Ordering::Release);
                            return;
                        }
                        let previous = cursor;
                        fill(
                            output,
                            channels,
                            usize::from(channel),
                            &samples,
                            &mut cursor,
                        );
                        if previous < samples.len() && cursor == samples.len() {
                            let timestamp = info.timestamp();
                            let end = timestamp
                                .playback
                                .duration_since(&timestamp.callback)
                                .and_then(|lead| {
                                    lead.checked_add(Duration::from_secs_f64(
                                        (cursor - previous) as f64 / f64::from(RATE),
                                    ))
                                })
                                .and_then(|delay| now.checked_add(delay));
                            if end.is_none_or(|end| send.try_send(end).is_err()) {
                                callback_failed.store(true, Ordering::Release);
                            }
                        }
                    },
                    move |_| stream_failed.store(true, Ordering::Release),
                    None,
                )
            }};
        }
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => stream!(f32),
            cpal::SampleFormat::I16 => stream!(i16),
            cpal::SampleFormat::U16 => stream!(u16),
            _ => unreachable!("filtered sample formats"),
        }?;
        stream.play()?;
        Ok(Self {
            _stream: stream,
            scheduled,
            estimated_end: None,
            failed,
        })
    }

    pub fn progress(&mut self) -> Result<Progress, PlaybackError> {
        if self.failed.load(Ordering::Acquire) {
            return Err(PlaybackError);
        }
        if self.estimated_end.is_none() {
            match self.scheduled.try_recv() {
                Ok(end) => self.estimated_end = Some(end),
                Err(TryRecvError::Empty) => return Ok(Progress::Queuing),
                Err(TryRecvError::Disconnected) => return Err(PlaybackError),
            }
        }
        let estimated_end = self.estimated_end.expect("received end estimate");
        Ok(if Instant::now() >= estimated_end {
            Progress::EstimateElapsed
        } else {
            Progress::Scheduled { estimated_end }
        })
    }
}

fn fill<T: SizedSample + FromSample<f32>>(
    output: &mut [T],
    channels: usize,
    channel: usize,
    samples: &[f32],
    cursor: &mut usize,
) {
    output.fill(T::from_sample(0.0));
    for (frame, &sample) in output.chunks_exact_mut(channels).zip(&samples[*cursor..]) {
        frame[channel] = T::from_sample(sample);
        *cursor += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::fill;

    #[test]
    fn selected_channel_consumes_samples_once_and_fills_remaining_frames_with_silence() {
        let mut cursor = 0;
        let samples = [0.25, -0.5, 0.75];
        let mut first = [1.0_f32; 4];
        let mut last = [1.0_f32; 4];
        fill(&mut first, 2, 1, &samples, &mut cursor);
        fill(&mut last, 2, 1, &samples, &mut cursor);
        assert_eq!(first, [0.0, 0.25, 0.0, -0.5]);
        assert_eq!(last, [0.0, 0.75, 0.0, 0.0]);
        assert_eq!(cursor, samples.len());
    }
}
