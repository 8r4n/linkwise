//! Energy-based burst candidates for controlled bench audio.

use std::{f64::consts::TAU, ops::Range};

/// Finds contiguous windows of audio above the bench RMS threshold.
///
/// Samples are mono and normalized to full scale. Positions are zero-based,
/// half-open sample ranges on the input stream. A candidate indicates signal
/// energy and requires waveform synchronization before protocol identification.
pub struct BurstDetector {
    window_samples: u32,
    window_fill: u32,
    window_energy: f64,
    position: u64,
    burst_start: Option<u64>,
}

impl BurstDetector {
    /// Use 10 ms windows and a 1% full-scale RMS threshold.
    pub fn new(sample_rate_hz: u32) -> Self {
        Self {
            window_samples: (sample_rate_hz / 100).max(1),
            window_fill: 0,
            window_energy: 0.0,
            position: 0,
            burst_start: None,
        }
    }

    /// Reports each completed candidate once a quiet window follows it.
    pub fn push(&mut self, samples: &[f32], mut report: impl FnMut(Range<u64>)) {
        for &sample in samples {
            let sample = f64::from(sample);
            self.window_energy += sample * sample;
            self.window_fill += 1;
            self.position += 1;
            if self.window_fill == self.window_samples {
                self.close_window(&mut report);
            }
        }
    }

    /// Processes a final partial window and reports any candidate at EOF.
    pub fn finish(mut self, mut report: impl FnMut(Range<u64>)) {
        if self.window_fill > 0 {
            self.close_window(&mut report);
        }
        if let Some(start) = self.burst_start {
            report(start..self.position);
        }
    }

    fn close_window(&mut self, report: &mut impl FnMut(Range<u64>)) {
        const MIN_MEAN_SQUARE: f64 = 0.01 * 0.01;
        let start = self.position - u64::from(self.window_fill);
        if self.window_energy >= MIN_MEAN_SQUARE * f64::from(self.window_fill) {
            self.burst_start.get_or_insert(start);
        } else if let Some(burst_start) = self.burst_start.take() {
            report(burst_start..start);
        }
        self.window_fill = 0;
        self.window_energy = 0.0;
    }
}

pub(super) struct Tone {
    pub(super) quadrature: Vec<(f64, f64)>,
}

impl Tone {
    pub(super) fn new(bin: usize, count: usize) -> Self {
        Self::at_bin(bin as f64, count)
    }

    pub(super) fn at_bin(bin: f64, count: usize) -> Self {
        let quadrature = (0..count)
            .map(|i| {
                let (sin, cos) = (TAU * bin * i as f64 / count as f64).sin_cos();
                (cos, sin)
            })
            .collect();
        Self { quadrature }
    }

    pub(super) fn power(&self, samples: &[f32]) -> f64 {
        let mut real = 0.0;
        let mut imag = 0.0;
        for (&sample, &(cos, sin)) in samples.iter().zip(&self.quadrature) {
            real += f64::from(sample) * cos;
            imag += f64::from(sample) * sin;
        }
        real * real + imag * imag
    }
}

pub(super) fn energy(samples: &[f32]) -> f64 {
    samples
        .iter()
        .map(|&sample| f64::from(sample).powi(2))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_is_independent_of_input_chunks() {
        let audio = [
            0.0, 0.0, 0.0, 0.0, 0.5, -0.5, 0.5, -0.5, 0.0, 0.0, 0.0, 0.0, 0.5,
        ];
        for chunk_size in [1, 3, 4, audio.len()] {
            let mut detector = BurstDetector::new(400);
            let mut bursts = Vec::new();
            for chunk in audio.chunks(chunk_size) {
                detector.push(chunk, |burst| bursts.push(burst));
            }
            detector.finish(|burst| bursts.push(burst));
            assert_eq!(bursts, [4..8, 12..13]);
        }
    }

    #[test]
    fn quiet_audio_yields_no_candidates() {
        let mut detector = BurstDetector::new(400);
        detector.push(&[0.0; 5], |_| panic!("silence produced a candidate"));
        detector.finish(|_| panic!("silence at EOF produced a candidate"));
    }
}
