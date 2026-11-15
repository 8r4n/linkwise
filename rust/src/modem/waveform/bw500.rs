//! Causal acquisition for BW500 audio at 48 kHz.

use std::{borrow::Cow, f64::consts::TAU, ops::Range};

const RATE: f64 = 48_000.0;
const WINDOW: usize = 480;
const TAPS: usize = 385;
const EDGE: u64 = 240;
const MIN_BAND_POWER: f64 = 0.000009;
const MIN_WIDEBAND_POWER: f64 = 0.000025;
const BAND_TO_OUTSIDE: f64 = 0.065;
const RECEIVER_SHARE_OF_RAW: f64 = 0.5;
const BAND_SHARE_OF_RECEIVER: f64 = 0.5;

struct Bandpass {
    taps: [f32; TAPS],
    history: [f32; TAPS],
    next: usize,
}

impl Default for Bandpass {
    fn default() -> Self {
        Self::new(1110.0, 1890.0)
    }
}

impl Bandpass {
    fn new(low_hz: f64, high_hz: f64) -> Self {
        let low = low_hz / RATE;
        let high = high_hz / RATE;
        Self {
            taps: std::array::from_fn(|i| {
                let n = i as f64 - (TAPS / 2) as f64;
                let ideal = if n == 0.0 {
                    2.0 * (high - low)
                } else {
                    ((TAU * high * n).sin() - (TAU * low * n).sin()) / (std::f64::consts::PI * n)
                };
                let phase = TAU * i as f64 / (TAPS - 1) as f64;
                (ideal * (0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos())) as f32
            }),
            history: [0.0; TAPS],
            next: 0,
        }
    }

    fn sample(&mut self, sample: f32) -> f32 {
        self.history[self.next] = sample;
        self.next += 1;
        if self.next == TAPS {
            self.next = 0;
        }
        let (earlier, later) = self.history.split_at(self.next);
        self.taps
            .iter()
            .zip(later.iter().chain(earlier))
            .map(|(tap, x)| tap * x)
            .sum()
    }

    fn filter(&mut self, samples: &[f32]) -> Vec<f32> {
        samples.iter().map(|&sample| self.sample(sample)).collect()
    }
}

/// Detect sustained energy concentrated in the occupied band using
/// simultaneous receiver-passband and broadband energy measurements.
pub struct Acquisition {
    filter: Bandpass,
    receiver_filter: Bandpass,
    position: u64,
    count: usize,
    band_energy: f64,
    receiver_energy: f64,
    raw_energy: f64,
    start: Option<u64>,
    last_active: u64,
}

impl Default for Acquisition {
    fn default() -> Self {
        Self {
            filter: Bandpass::default(),
            receiver_filter: Bandpass::new(300.0, 2700.0),
            position: 0,
            count: 0,
            band_energy: 0.0,
            receiver_energy: 0.0,
            raw_energy: 0.0,
            start: None,
            last_active: 0,
        }
    }
}

impl Acquisition {
    pub fn push(&mut self, samples: &[f32], mut emit: impl FnMut(Range<u64>)) {
        for &sample in samples {
            self.push_sample(sample, &mut emit);
        }
    }

    fn process(&mut self, samples: &[f32], mut emit: impl FnMut(Range<u64>)) -> Vec<f32> {
        samples
            .iter()
            .map(|&sample| self.push_sample(sample, &mut emit))
            .collect()
    }

    fn push_sample(&mut self, sample: f32, emit: &mut impl FnMut(Range<u64>)) -> f32 {
        let band = self.filter.sample(sample);
        self.band_energy += f64::from(band).powi(2);
        self.receiver_energy += f64::from(self.receiver_filter.sample(sample)).powi(2);
        self.raw_energy += f64::from(sample).powi(2);
        self.count += 1;
        self.position += 1;
        if self.count == WINDOW {
            self.close(emit);
        }
        band
    }

    pub fn finish(mut self, mut emit: impl FnMut(Range<u64>)) {
        if self.count > 0 {
            self.close(&mut emit);
        }
        self.emit(&mut emit);
    }

    fn close(&mut self, emit: &mut impl FnMut(Range<u64>)) {
        // When most of the energy is already in the receiver passband,
        // flat receiver noise can clear the broadband gate by itself.
        let receiver_limited = self.receiver_energy > RECEIVER_SHARE_OF_RAW * self.raw_energy;
        // Stock control bursts can briefly spread energy beyond the
        // acquisition band. Bridge those dips in receiver-filtered audio.
        let quiet_windows = if receiver_limited { 5 } else { 3 };
        let active = self.band_energy >= MIN_BAND_POWER * self.count as f64
            && (!receiver_limited
                || self.band_energy > BAND_SHARE_OF_RECEIVER * self.receiver_energy)
            && self.band_energy > BAND_TO_OUTSIDE * (self.raw_energy - self.band_energy);
        if active {
            self.start.get_or_insert(self.position - self.count as u64);
            self.last_active = self.position;
        } else if self.position - self.last_active >= quiet_windows * WINDOW as u64 {
            self.emit(emit);
        }
        self.count = 0;
        self.band_energy = 0.0;
        self.receiver_energy = 0.0;
        self.raw_energy = 0.0;
    }

    fn emit(&mut self, emit: &mut impl FnMut(Range<u64>)) {
        if let Some(start) = self.start.take()
            && self.last_active - start >= 2 * WINDOW as u64
        {
            emit(start.saturating_sub(EDGE)..(self.last_active + EDGE).min(self.position));
        }
    }
}

/// Guarded records with substantial out-of-band energy need the filtered path.
/// Keep the established raw decoder path on quiet records, including DATA
/// whose leading symbols have unusually low energy.
fn needs_filter(samples: &[f32]) -> bool {
    if samples.len() < 6 * WINDOW {
        return false;
    }
    let guard = 3 * WINDOW;
    [&samples[..guard], &samples[samples.len() - guard..]]
        .into_iter()
        .any(|edge| {
            let filtered = Bandpass::default().filter(edge);
            // Ignore the filter startup interval.
            let band = super::energy(&filtered[WINDOW..]);
            let raw = super::energy(&edge[WINDOW..]);
            raw >= MIN_WIDEBAND_POWER * (guard - WINDOW) as f64 && band < BAND_TO_OUTSIDE * raw
        })
}

/// Prepare a complete ingress record for the existing BW500 decoders.
pub fn prepare(samples: &[f32]) -> (Cow<'_, [f32]>, Vec<Range<u64>>) {
    let mut candidates = Vec::new();
    if needs_filter(samples) {
        let mut detector = Acquisition::default();
        let filtered = detector.process(samples, |range| candidates.push(range));
        detector.finish(|range| candidates.push(range));
        (Cow::Owned(filtered), candidates)
    } else {
        let mut detector = super::BurstDetector::new(RATE as u32);
        detector.push(samples, |range| candidates.push(range));
        detector.finish(|range| candidates.push(range));
        (Cow::Borrowed(samples), candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_acquisition_closes_on_receiver_passband_noise() {
        let burst_start = 12_000;
        let burst_end = burst_start + 48_000;
        let mut seed = 283_u32;
        let white: Vec<_> = (0..burst_end + 12_000)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed as i32) as f32 / i32::MAX as f32
            })
            .collect();
        let noise = Bandpass::new(300.0, 2700.0).filter(&white);
        let gain = 0.007 / (super::super::energy(&noise) / noise.len() as f64).sqrt();
        let samples: Vec<_> = noise
            .into_iter()
            .enumerate()
            .map(|(n, noise)| {
                let tone = |hz: f64| (TAU * hz * n as f64 / RATE).sin();
                let signal = if (36_000..37_920).contains(&n) {
                    // A brief energy dip in the occupied band, as in stock
                    // connection controls.
                    0.04 * tone(1500.0) + 0.1 * tone(700.0)
                } else if (burst_start..burst_end).contains(&n) {
                    0.1 * tone(1500.0)
                } else {
                    0.0
                };
                (f64::from(noise) * gain + signal) as f32
            })
            .collect();
        let mut detector = Acquisition::default();
        let mut streaming = Vec::new();
        for chunk in samples.chunks(997) {
            detector.push(chunk, |range| streaming.push(range));
        }
        assert_eq!(streaming.len(), 1, "{streaming:?}");
        assert!(streaming[0].start.abs_diff(burst_start as u64) <= WINDOW as u64);
        assert!(streaming[0].end.abs_diff(burst_end as u64) <= 2 * WINDOW as u64);
    }

    #[test]
    fn streaming_acquisition_separates_bursts_across_chunk_boundaries() {
        let samples: Vec<_> = (0..24_000)
            .map(|n| {
                let waveform = if (4800..9600).contains(&n) || (14_400..19_200).contains(&n) {
                    0.1 * (TAU * 1500.0 * n as f64 / RATE).sin()
                } else {
                    0.0
                };
                (waveform + 0.15 * (TAU * 4300.0 * n as f64 / RATE).sin()) as f32
            })
            .collect();
        let (filtered, expected) = prepare(&samples);
        assert!(matches!(filtered, Cow::Owned(_)));
        assert_eq!(expected.len(), 2);
        for (range, start) in expected.iter().zip([4800, 14_400]) {
            assert!(range.start.abs_diff(start) < WINDOW as u64);
            assert!(range.end.abs_diff(start + 4800) <= WINDOW as u64 * 2);
        }

        let mut detector = Acquisition::default();
        let mut streaming = Vec::new();
        for chunk in samples.chunks(317) {
            detector.push(chunk, |range| streaming.push(range));
        }
        assert_eq!(streaming, expected);
    }
}
