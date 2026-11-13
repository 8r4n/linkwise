//! Level-4 two-band DBPSK waveform, from the published VARA physical/coding
//! specification. Measured grid phases and pulse taps are supplied separately.
//! No upstream implementation or proprietary table file is embedded here.

use std::{f64::consts::TAU, ops::Range};

use serde::Deserialize;

use super::{bw500_level4, coding::Random24};

const HOP: usize = 512;
const COLUMNS: usize = 393;
const PREFIX: usize = 9 * HOP;
const PULSE: usize = 513;
type Cell = (f64, f64);

#[derive(Deserialize)]
struct Measurements {
    grid: Vec<(usize, u8)>,
    pulse: Vec<f64>,
}

/// Explicit external measurements; construction validates every required cell.
#[derive(Clone)]
pub struct Waveform {
    reference: [[Cell; COLUMNS]; 2],
    pulse: [f64; PULSE],
    pilots: [[bool; COLUMNS]; 2],
}

#[derive(Debug, PartialEq, Eq)]
pub struct Decoded {
    pub samples: Range<usize>,
    pub frame: [u8; bw500_level4::FRAME_BYTES],
}

fn multiply(a: Cell, b: Cell) -> Cell {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}

impl Waveform {
    pub fn from_json(json: &str) -> Result<Self, String> {
        if json.len() > 128 * 1024 {
            return Err("measurement profile exceeds 128 KiB".into());
        }
        let measurements: Measurements = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let pulse: [f64; PULSE] = measurements
            .pulse
            .try_into()
            .map_err(|_| "level-4 pulse must contain 513 samples")?;
        if pulse
            .iter()
            .any(|value| !value.is_finite() || value.abs() > 2.0)
            || pulse[256].abs() < 0.1
        {
            return Err("invalid level-4 pulse".into());
        }
        let mut reference = [[(0.0, 0.0); COLUMNS]; 2];
        let mut seen = [[false; COLUMNS]; 2];
        for (index, phase) in measurements.grid {
            if phase >= 128 {
                return Err("phase must be a 128-point lattice index".into());
            }
            let band = usize::from(index >= 471);
            let column = index.saturating_sub(band * 471);
            if column >= COLUMNS {
                return Err("unexpected reference index".into());
            }
            if seen[band][column] {
                return Err("duplicate reference cell".into());
            }
            seen[band][column] = true;
            let (sin, cos) = (TAU * f64::from(phase) / 128.0).sin_cos();
            reference[band][column] = (cos, sin);
        }
        if seen.iter().flatten().any(|&value| !value) {
            return Err("missing measured reference cell".into());
        }
        let mut random = Random24(6_437_280);
        random.permutation(748);
        random.permutation(368);
        let mut pilots = [[false; COLUMNS]; 2];
        for band in &mut pilots {
            let mut count = 0;
            while count < 18 {
                let column = 5 + random.draw(386);
                if !band[column] {
                    band[column] = true;
                    count += 1;
                }
            }
        }
        Ok(Self {
            reference,
            pulse,
            pilots,
        })
    }

    /// One CRC-protected frame, at 48 kHz. Nine initial symbol slots precede
    /// the reference; acquisition lead randomness is not synthesized here.
    pub fn synthesize(&self, frame: &[u8; bw500_level4::FRAME_BYTES]) -> Option<Vec<f32>> {
        let bits = bw500_level4::encode(frame)?;
        let length = PREFIX + COLUMNS * HOP + 1;
        let mut audio = vec![0.0; length];
        let mut bit = 0;
        for band in 0..2 {
            let frequency = [1350.0, 1650.0][band];
            let mut polarity = 1.0;
            for column in 0..COLUMNS {
                if column > 0 && !self.pilots[band][column] {
                    if bits[bit] != 0 {
                        polarity = -polarity;
                    }
                    bit += 1;
                }
                let reference = self.reference[band][column];
                let center = PREFIX + column * HOP + band * HOP / 2;
                for (tap, &gain) in self.pulse.iter().enumerate() {
                    let n = center + tap - HOP / 2;
                    let (sin, cos) = (TAU * frequency * n as f64 / 48_000.0).sin_cos();
                    audio[n] +=
                        (0.25 * polarity * gain * (reference.0 * cos + reference.1 * sin)) as f32;
                }
            }
        }
        debug_assert_eq!(bit, bw500_level4::CODED_CELLS);
        Some(audio)
    }

    fn symbol(&self, mixed: &[Cell], center: usize) -> Option<Cell> {
        let first = center.checked_sub(HOP / 2)?;
        let samples = mixed.get(first..first + PULSE)?;
        Some(
            samples
                .iter()
                .zip(self.pulse)
                .fold((0.0, 0.0), |sum, (sample, gain)| {
                    (sum.0 + sample.0 * gain, sum.1 + sample.1 * gain)
                }),
        )
    }

    fn derotate(&self, mixed: &[Cell], band: usize, column: usize, start: usize) -> Option<Cell> {
        let center = start + column * HOP + band * HOP / 2;
        Some(multiply(
            self.symbol(mixed, center)?,
            self.reference[band][column],
        ))
    }

    fn lock_score(&self, mixed: &[Vec<Cell>; 2], start: usize) -> f64 {
        let mut score = 0.0;
        for (band, input) in mixed.iter().enumerate() {
            for column in 1..COLUMNS {
                if !self.pilots[band][column] {
                    continue;
                }
                let Some(a) = self.derotate(input, band, column, start) else {
                    return -1.0;
                };
                let Some(b) = self.derotate(input, band, column - 1, start) else {
                    return -1.0;
                };
                let scale = ((a.0 * a.0 + a.1 * a.1) * (b.0 * b.0 + b.1 * b.1)).sqrt();
                score += (a.0 * b.0 + a.1 * b.1) / scale.max(f64::EPSILON);
            }
        }
        score / 36.0
    }

    /// Acquire one level-4 frame near an energy candidate. Timing is locked
    /// against fixed pilots before the CRC-checked FEC decoder is attempted.
    /// Nominal-rate controlled audio only; no carrier/sample-clock tracking.
    pub fn decode(&self, audio: &[f32], candidate: Range<usize>) -> Option<Decoded> {
        if candidate.start >= candidate.end
            || candidate.end > audio.len()
            || audio.iter().any(|x| !x.is_finite())
        {
            return None;
        }
        let mixed: [Vec<Cell>; 2] = [1350.0, 1650.0].map(|frequency| {
            audio
                .iter()
                .enumerate()
                .map(|(n, &sample)| {
                    let (sin, cos) = (TAU * frequency * n as f64 / 48_000.0).sin_cos();
                    (f64::from(sample) * cos, -f64::from(sample) * sin)
                })
                .collect()
        });
        let required = (COLUMNS - 1) * HOP + HOP + 1;
        let last = audio
            .len()
            .min(candidate.end.saturating_add(HOP))
            .checked_sub(required)?;
        let first = candidate.start.saturating_sub(HOP).max(HOP / 2);
        let through = candidate.start.saturating_add(PREFIX + HOP).min(last);
        let (coarse, _) = (first..=through)
            .step_by(32)
            .map(|start| (start, self.lock_score(&mixed, start)))
            .max_by(|a, b| a.1.total_cmp(&b.1))?;
        let (start, score) = (coarse.saturating_sub(32).max(first)
            ..=coarse.saturating_add(32).min(through))
            .step_by(2)
            .map(|start| (start, self.lock_score(&mixed, start)))
            .max_by(|a, b| a.1.total_cmp(&b.1))?;
        if score < 0.65 {
            return None;
        }
        let mut cells = [0.0; bw500_level4::CODED_CELLS];
        let mut index = 0;
        for (band, input) in mixed.iter().enumerate() {
            for column in 1..COLUMNS {
                if self.pilots[band][column] {
                    continue;
                }
                let a = self.derotate(input, band, column, start)?;
                let b = self.derotate(input, band, column - 1, start)?;
                cells[index] = -(a.0 * b.0 + a.1 * b.1);
                index += 1;
            }
        }
        let scale = cells.iter().map(|v| v.abs()).sum::<f64>() / cells.len() as f64;
        if scale <= f64::EPSILON {
            return None;
        }
        for cell in &mut cells {
            *cell *= 6.0 / scale;
        }
        let frame = bw500_level4::decode(&cells)?;
        Some(Decoded {
            samples: start - HOP / 2..start + required,
            frame,
        })
    }
}
