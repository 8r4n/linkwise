//! Two-path Gaussian-scatter profile for finite real 48 kHz transmit samples.

use std::f64::consts::{PI, TAU};

use rustfft::{FftPlanner, num_complex::Complex32};

const SAMPLE_RATE: usize = 48_000;
pub(crate) const DELAY_SAMPLES: usize = 48;
const DOPPLER_SPREAD_HZ: f64 = 0.5; // Two standard deviations of the power spectrum.
const COEFFICIENT_RATE: usize = 32;
const SAMPLES_PER_COEFFICIENT: usize = SAMPLE_RATE / COEFFICIENT_RATE;
const TAP_RADIUS: usize = 4 * COEFFICIENT_RATE;
const ANALYTIC_GUARD: usize = 256;
const AUDIO_FILTER_SIZE: usize = 513;
const AUDIO_FILTER_DELAY: usize = AUDIO_FILTER_SIZE / 2;

pub(crate) struct Channel {
    planner: FftPlanner<f32>,
    scratch: Vec<Complex32>,
    audio_filter: Vec<f32>,
    filter_response: Vec<Complex32>,
    noise_gain: f64,
    tap_kernel: Vec<f64>,
}

impl Channel {
    pub(crate) fn new() -> Self {
        let tap_kernel: Vec<_> = (0..=2 * TAP_RADIUS)
            .map(|index| {
                let time = (index as f64 - TAP_RADIUS as f64) / COEFFICIENT_RATE as f64;
                (-(PI * DOPPLER_SPREAD_HZ * time).powi(2)).exp()
            })
            .collect();
        let norm = tap_kernel.iter().map(|v| v * v).sum::<f64>().sqrt();
        let audio_filter = audio_filter();
        // The noise passes through the shaping filter and the measurement
        // filter. The squared norm of their convolution is the mean power
        // gain of these two filters for unit-variance white noise.
        let mut cascade = vec![0.0; 2 * AUDIO_FILTER_SIZE - 1];
        for (i, &a) in audio_filter.iter().enumerate() {
            for (j, &b) in audio_filter.iter().enumerate() {
                cascade[i + j] += f64::from(a) * f64::from(b);
            }
        }
        Self {
            planner: FftPlanner::new(),
            scratch: Vec::new(),
            audio_filter,
            filter_response: Vec::new(),
            noise_gain: cascade.iter().map(|v| v * v).sum(),
            tap_kernel: tap_kernel.into_iter().map(|v| v / norm).collect(),
        }
    }

    fn transform(&mut self, data: &mut [Complex32], inverse: bool) {
        let fft = if inverse {
            self.planner.plan_fft_inverse(data.len())
        } else {
            self.planner.plan_fft_forward(data.len())
        };
        self.scratch
            .resize(fft.get_inplace_scratch_len(), Complex32::default());
        fft.process_with_scratch(data, &mut self.scratch);
    }

    pub(crate) fn propagate(&mut self, samples: &[f32], seed: u32, start: u64, output: &mut [f32]) {
        let size = (samples.len() + 2 * ANALYTIC_GUARD).next_power_of_two();
        let mut analytic = vec![Complex32::default(); size];
        for (value, &sample) in analytic[ANALYTIC_GUARD..].iter_mut().zip(samples) {
            value.re = sample;
        }
        self.transform(&mut analytic, false);
        // A real-to-analytic conversion retains DC and Nyquist, doubles
        // positive-frequency interior bins, and removes negative bins.
        for value in &mut analytic[1..size / 2] {
            *value *= 2.0;
        }
        analytic[size / 2 + 1..].fill(Complex32::default());
        self.transform(&mut analytic, true);

        let taps = TapSeries::new(&self.tap_kernel, seed, start, output.len());
        let inverse_size = 1.0 / size as f32;
        output.fill(0.0);
        for (i, sample) in analytic[ANALYTIC_GUARD..][..samples.len()]
            .iter()
            .enumerate()
        {
            let sample = sample * inverse_size;
            output[i] += (sample * taps.at(start + i as u64, 0)).re;
            output[i + DELAY_SAMPLES] +=
                (sample * taps.at(start + (i + DELAY_SAMPLES) as u64, 1)).re;
        }
    }

    pub(crate) fn add_noise(&mut self, samples: &[f32], seed: u32, snr_db: f64, noise: &mut [f32]) {
        let desired = self.reference_power(samples) * 10_f64.powf(-snr_db / 10.0);
        let scale = (desired / self.noise_gain).sqrt() as f32;
        // Generate filter history on both sides. Keep the stationary valid
        // convolution segment and leave transmit guard samples out of it.
        let mut white = vec![0.0; noise.len() + AUDIO_FILTER_SIZE - 1];
        for (i, pair) in white.chunks_mut(2).enumerate() {
            let [a, b] = gaussian_pair(u64::from(seed) ^ 0x885c_8dbf_86a5_13e3, i as u64);
            pair[0] = a as f32;
            if let Some(second) = pair.get_mut(1) {
                *second = b as f32;
            }
        }
        let filtered = self.filtered(&white);
        let factor = scale / filtered.len() as f32;
        for (out, value) in noise.iter_mut().zip(&filtered[AUDIO_FILTER_SIZE - 1..]) {
            *out = value.re * factor;
        }
    }

    pub(crate) fn reference_power(&mut self, samples: &[f32]) -> f64 {
        let filtered = self.filtered(samples);
        let inverse_size = 1.0 / filtered.len() as f64;
        filtered[AUDIO_FILTER_DELAY..][..samples.len()]
            .iter()
            .map(|value| (f64::from(value.re) * inverse_size).powi(2))
            .sum::<f64>()
            / samples.len() as f64
    }

    fn filtered(&mut self, samples: &[f32]) -> Vec<Complex32> {
        let size = (samples.len() + AUDIO_FILTER_SIZE - 1).next_power_of_two();
        let mut values = vec![Complex32::default(); size];
        if self.filter_response.len() != size {
            for (value, &tap) in values.iter_mut().zip(&self.audio_filter) {
                value.re = tap;
            }
            self.transform(&mut values, false);
            std::mem::swap(&mut self.filter_response, &mut values);
            values.resize(size, Complex32::default());
            values.fill(Complex32::default());
        }
        for (value, &sample) in values.iter_mut().zip(samples) {
            value.re = sample;
        }
        self.transform(&mut values, false);
        for (value, response) in values.iter_mut().zip(&self.filter_response) {
            *value *= response;
        }
        self.transform(&mut values, true);
        values
    }
}

fn audio_filter() -> Vec<f32> {
    // Same 513-tap Hamming band-pass design as the Python reference. Cutoff
    // frequencies are 300 and 3300 Hz and pass-band midpoint gain is one.
    let low = 300.0 / (SAMPLE_RATE as f64 / 2.0);
    let high = 3300.0 / (SAMPLE_RATE as f64 / 2.0);
    let mut gain = 0.0;
    let taps: Vec<_> = (0..AUDIO_FILTER_SIZE)
        .map(|i| {
            let offset = i as f64 - AUDIO_FILTER_DELAY as f64;
            let window = 0.54 - 0.46 * (TAU * i as f64 / (AUDIO_FILTER_SIZE - 1) as f64).cos();
            let tap = window * (high * sinc(high * offset) - low * sinc(low * offset));
            gain += tap * (PI * (low + high) / 2.0 * offset).cos();
            tap
        })
        .collect();
    taps.into_iter().map(|tap| (tap / gain) as f32).collect()
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let phase = PI * x;
    phase.sin() / phase
}

struct TapSeries {
    first: u64,
    values: Vec<[Complex32; 2]>,
}

impl TapSeries {
    fn new(kernel: &[f64], seed: u32, start: u64, count: usize) -> Self {
        let first = start / SAMPLES_PER_COEFFICIENT as u64;
        let last = (start + count as u64 - 1) / SAMPLES_PER_COEFFICIENT as u64 + 1;
        let count = (last - first + 1) as usize;
        let key = u64::from(seed) ^ 0xf9d4_3314_3af2_63b9;
        let first_white = first.wrapping_sub(TAP_RADIUS as u64);
        let white: Vec<_> = (0..count + kernel.len() - 1)
            .map(|i| {
                let at = first_white.wrapping_add(i as u64);
                [
                    gaussian_pair(key, at),
                    gaussian_pair(key ^ 0x0c56_ebd7_de0a_974f, at),
                ]
            })
            .collect();
        let values = white
            .windows(kernel.len())
            .map(|points| {
                let mut sum = [[0.0; 2]; 2];
                for (&weight, point) in kernel.iter().zip(points) {
                    for path in 0..2 {
                        sum[path][0] += point[path][0] * weight;
                        sum[path][1] += point[path][1] * weight;
                    }
                }
                // Independent I and Q each have variance 1/4. The two
                // equal independent paths therefore have total mean power 1.
                sum.map(|[i, q]| Complex32::new((i / 2.0) as f32, (q / 2.0) as f32))
            })
            .collect();
        Self { first, values }
    }

    fn at(&self, sample: u64, path: usize) -> Complex32 {
        let interval = SAMPLES_PER_COEFFICIENT as u64;
        let index = (sample / interval - self.first) as usize;
        let fraction = (sample % interval) as f32 / interval as f32;
        let left = self.values[index][path];
        left + (self.values[index + 1][path] - left) * fraction
    }
}

fn gaussian_pair(seed: u64, index: u64) -> [f64; 2] {
    let counter = index.wrapping_mul(2);
    let length = (-2.0 * uniform(seed, counter).ln()).sqrt();
    let phase = TAU * uniform(seed, counter.wrapping_add(1));
    let (sin, cos) = phase.sin_cos();
    [length * cos, length * sin]
}

fn uniform(seed: u64, index: u64) -> f64 {
    // Counter-based SplitMix64 means absolute positions have identical
    // random coefficients even when an overlapping window is queried later.
    let mut bits = seed.wrapping_add(index.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    bits ^= bits >> 31;
    ((bits >> 11) as f64 + 0.5) / ((1_u64 << 53) as f64)
}
