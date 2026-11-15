//! Offline stock-capture sensitivity study. No radio or live modem is used.

use std::{
    env,
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
};

use super::*;
use crate::modem::{
    audio_handshake,
    bw500_waveform::Waveform,
    control::{self, Bandwidth, Message},
    handshake::{Burst, Expected},
    waveform::BurstDetector,
};
use rustfft::{FftPlanner, num_complex::Complex32};

const RATE: usize = 48_000;
const WINDOW: usize = 480;
const GUARD: usize = 24_000;
const REF_BW: f64 = 3000.0;

#[derive(Clone, Copy)]
enum Kind {
    Data(Level),
    Level4Data,
    Setup,
    Ack,
    Request,
    Response,
}

#[derive(PartialEq)]
enum Decoded {
    Data(Frame),
    Level4([u8; 46]),
    KeyedControl,
}

fn candidates(audio: &[f32], flush: bool) -> Vec<Range<usize>> {
    let mut detector = BurstDetector::new(RATE as u32);
    let mut ranges = Vec::new();
    let mut record = |r: Range<u64>| ranges.push(r.start as usize..r.end as usize);
    for chunk in audio.chunks(997) {
        detector.push(chunk, &mut record);
    }
    if flush {
        detector.finish(record);
    }
    ranges
}

fn decode(kind: Kind, waveform: &Waveform, audio: &[f32], r: Range<usize>) -> Option<Decoded> {
    match kind {
        Kind::Data(level) => decode_burst(audio, r)
            .into_iter()
            .find(|frame| frame.level == level)
            .map(|frame| Decoded::Data(frame.frame)),
        Kind::Level4Data => waveform
            .decode(audio, r)
            .map(|frame| Decoded::Level4(frame.frame)),
        Kind::Setup => {
            if let Burst::Setup(setup) =
                audio_handshake::decode_burst(waveform, Expected::Setup, audio, r)?
            {
                Some(Decoded::Level4(
                    setup
                        .encode(crate::modem::setup::Profile::Bw500)?
                        .try_into()
                        .ok()?,
                ))
            } else {
                None
            }
        }
        Kind::Ack => control::find_connected_ack(Bandwidth::Hz500, "W1AW", audio, r.start)
            .map(|_| Decoded::KeyedControl),
        Kind::Request => Message::ConnectRequest(Bandwidth::Hz500)
            .find("W1AW", audio, r.start)
            .map(|_| Decoded::KeyedControl),
        Kind::Response => Message::ConnectResponse(Bandwidth::Hz500)
            .find("KN6MQS", audio, r.start)
            .map(|_| Decoded::KeyedControl),
    }
}

fn aligned(
    kind: Kind,
    waveform: &Waveform,
    clean: &Range<usize>,
    audio: &[f32],
    start: usize,
) -> Option<Decoded> {
    match kind {
        Kind::Data(level) => {
            let mode = Mode::new(level);
            decode_aligned(
                &mode,
                &mode.tones(),
                &audio[start..start + mode.samples_per_frame()],
            )
            .map(Decoded::Data)
        }
        Kind::Level4Data | Kind::Setup => {
            let raw = waveform.decode_aligned(&Waveform::mix(audio), start)?.frame;
            if matches!(kind, Kind::Setup) {
                let profile = crate::modem::setup::Profile::Bw500;
                let setup = crate::modem::setup::LinkSetup::decode(profile, &raw)?;
                Some(Decoded::Level4(setup.encode(profile)?.try_into().ok()?))
            } else {
                Some(Decoded::Level4(raw))
            }
        }
        Kind::Ack | Kind::Request | Kind::Response => decode(kind, waveform, audio, clean.clone()),
    }
}

fn load(path: PathBuf) -> Vec<f32> {
    let mut wav = hound::WavReader::open(path).unwrap();
    let spec = wav.spec();
    assert_eq!(spec.sample_rate, RATE as u32);
    let channels = spec.channels as usize;
    match spec.sample_format {
        hound::SampleFormat::Float => wav
            .samples::<f32>()
            .step_by(channels)
            .map(Result::unwrap)
            .collect(),
        hound::SampleFormat::Int => {
            let scale = (1_u64 << (spec.bits_per_sample - 1)) as f32;
            wav.samples::<i32>()
                .step_by(channels)
                .map(|s| s.unwrap() as f32 / scale)
                .collect()
        }
    }
}

// SplitMix64 with Box–Muller pairs. Unit normal at each real sample.
fn white(seed: usize, count: usize) -> Vec<f32> {
    let mut state = seed as u64;
    let mut uniform = || {
        state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        (((z ^ (z >> 31)) >> 11) as f64 + 0.5) / (1_u64 << 53) as f64
    };
    (0..count.div_ceil(2))
        .flat_map(|_| {
            let mag = (-2.0 * uniform().ln()).sqrt();
            let (sin, cos) = (std::f64::consts::TAU * uniform()).sin_cos();
            [(mag * cos) as f32, (mag * sin) as f32]
        })
        .take(count)
        .collect()
}

// Zero-phase offline filter, used to estimate the value of restricting the
// detector and demodulator noise bandwidth before designing a streaming filter.
fn bandpass(audio: &[f32], planner: &mut FftPlanner<f32>) -> Vec<f32> {
    let size = audio.len().next_power_of_two();
    let mut spectrum = vec![Complex32::default(); size];
    for (bin, &sample) in spectrum.iter_mut().zip(audio) {
        bin.re = sample;
    }
    planner.plan_fft_forward(size).process(&mut spectrum);
    for (bin, value) in spectrum.iter_mut().enumerate() {
        let hz = bin.min(size - bin) * RATE / size;
        if !(1150..=1850).contains(&hz) {
            *value = Complex32::default();
        }
    }
    planner.plan_fft_inverse(size).process(&mut spectrum);
    spectrum[..audio.len()]
        .iter()
        .map(|x| x.re / size as f32)
        .collect()
}

// Translate the analytic signal so both stock sub-bands move by the
// same number of Hz without creating a mirror carrier.
fn translate(audio: &[f32], hz: f32, planner: &mut FftPlanner<f32>) -> Vec<f32> {
    let size = audio.len().next_power_of_two();
    let mut spectrum = vec![Complex32::default(); size];
    for (bin, &sample) in spectrum.iter_mut().zip(audio) {
        bin.re = sample;
    }
    planner.plan_fft_forward(size).process(&mut spectrum);
    for bin in &mut spectrum[1..size / 2] {
        *bin *= 2.0;
    }
    spectrum[size / 2 + 1..].fill(Complex32::default());
    planner.plan_fft_inverse(size).process(&mut spectrum);
    let radians = std::f32::consts::TAU * hz / RATE as f32;
    spectrum[..audio.len()]
        .iter()
        .enumerate()
        .map(|(index, value)| {
            (value * Complex32::from_polar(1.0, radians * index as f32)).re / size as f32
        })
        .collect()
}

fn band_candidates(audio: &[f32]) -> Vec<Range<usize>> {
    let power: Vec<_> = audio
        .chunks_exact(WINDOW)
        .map(|chunk| energy(chunk) / WINDOW as f64)
        .collect();
    let background = &power[3..GUARD / WINDOW - 5];
    let mean = background.iter().sum::<f64>() / background.len() as f64;
    let deviation = (background.iter().map(|p| (p - mean).powi(2)).sum::<f64>()
        / (background.len() - 1) as f64)
        .sqrt();
    let threshold = (mean + 3.0 * deviation).max(0.003_f64.powi(2));
    let mut ranges = Vec::new();
    let mut start = None;
    let mut last = 0;
    // Three quiet windows close a candidate. Demand two active windows.
    for (index, &energy) in power.iter().enumerate() {
        if energy >= threshold {
            start.get_or_insert(index);
            last = index;
        } else if index >= last + 3
            && let Some(first) = start.take()
            && last > first
        {
            ranges.push(first * WINDOW..(last + 1) * WINDOW);
        }
    }
    ranges
}

fn exact(
    kind: Kind,
    waveform: &Waveform,
    audio: &[f32],
    ranges: &[Range<usize>],
    expected: &Decoded,
) -> bool {
    ranges
        .iter()
        .any(|range| decode(kind, waveform, audio, range.clone()).as_ref() == Some(expected))
}

#[test]
#[ignore = "requires a stock capture manifest and measured BW500 profile"]
fn stock_awgn_sweep() {
    let manifest = std::fs::read_to_string(env::var_os("BW500_STUDY_INPUT").unwrap()).unwrap();
    let waveform = Waveform::from_json(
        &std::fs::read_to_string(env::var_os("BW500_STUDY_PROFILE").unwrap()).unwrap(),
    )
    .unwrap();
    let mut csv = BufWriter::new(File::create(env::var_os("BW500_STUDY_CSV").unwrap()).unwrap());
    let trials: usize = env::var("BW500_STUDY_TRIALS")
        .unwrap_or("24".into())
        .parse()
        .unwrap();
    writeln!(csv, "kind,fixture,snr_db,seed,baseline_start,baseline_stream_start,baseline_decode,band_start,band_decode,clean_window_decode,aligned_decode,baseline_noise_only,band_noise_only,baseline_noise_decode,band_noise_decode,filtered_decode,filtered_noise_decode").unwrap();
    let gain: f32 = env::var("BW500_STUDY_GAIN").map_or(1.0, |value| value.parse().unwrap());
    let delay: usize =
        env::var("BW500_STUDY_ECHO_SAMPLES").map_or(0, |value| value.parse().unwrap());
    let min_snr: i32 = env::var("BW500_STUDY_MIN_SNR").map_or(-16, |value| value.parse().unwrap());
    let max_snr: i32 = env::var("BW500_STUDY_MAX_SNR").map_or(44, |value| value.parse().unwrap());
    let step: usize = env::var("BW500_STUDY_STEP").map_or(4, |value| value.parse().unwrap());
    let offset_hz: f32 =
        env::var("BW500_STUDY_OFFSET_HZ").map_or(0.0, |value| value.parse().unwrap());
    let interferer_db: Option<f64> = env::var("BW500_STUDY_INTERFERER_DB")
        .ok()
        .map(|value| value.parse().unwrap());
    let mut planner = FftPlanner::new();
    for (fixture_index, line) in manifest.lines().enumerate() {
        let (label, path) = line.split_once('\t').unwrap();
        let kind = match label {
            "data1" => Kind::Data(Level::One),
            "data2" => Kind::Data(Level::Two),
            "data3" => Kind::Data(Level::Three),
            "data4" => Kind::Level4Data,
            "setup" => Kind::Setup,
            "ack" => Kind::Ack,
            "request" => Kind::Request,
            "response" => Kind::Response,
            _ => panic!("unknown fixture kind {label}"),
        };
        let path = PathBuf::from(path);
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        let raw = load(path);
        let range = candidates(&raw, true)
            .into_iter()
            .max_by_key(|r| r.len())
            .unwrap();
        let mut clean = vec![0.0; GUARD];
        clean.extend_from_slice(
            &raw[range.start.saturating_sub(2 * WINDOW)..(range.end + 2 * WINDOW).min(raw.len())],
        );
        clean.resize(clean.len() + GUARD, 0.0);
        let truth = candidates(&clean, false)
            .into_iter()
            .max_by_key(|r| r.len())
            .unwrap();
        let expected = decode(kind, &waveform, &clean, truth.clone())
            .unwrap_or_else(|| panic!("clean decode failed for {name} at {truth:?}"));
        let start = match kind {
            Kind::Level4Data | Kind::Setup => {
                waveform
                    .decode(&clean, truth.clone())
                    .unwrap()
                    .samples
                    .start
                    + 256
            }
            Kind::Data(level) => {
                acquire(&Mode::new(level), &Mode::new(level).tones(), &clean, &truth).unwrap()
            }
            Kind::Ack | Kind::Request | Kind::Response => truth.start,
        };
        assert!(aligned(kind, &waveform, &truth, &clean, start).as_ref() == Some(&expected));
        let detect = |ranges: &[Range<usize>]| {
            ranges
                .iter()
                .any(|r| r.start.abs_diff(truth.start) <= 2 * WINDOW)
        };
        let rms = (energy(&clean[truth.clone()]) / truth.len() as f64).sqrt();
        eprintln!("{label} {name}, clean RMS {rms:.4}, range {truth:?}, aligned {start}");
        let path_test = |audio: &[f32], ranges: &[Range<usize>]| {
            exact(kind, &waveform, audio, ranges, &expected)
        };
        let signal: Vec<_> = clean
            .iter()
            .enumerate()
            .map(|(i, sample)| {
                let echo = if delay > 0 && i >= delay {
                    0.5 * clean[i - delay]
                } else {
                    0.0
                };
                gain * (sample + echo)
            })
            .collect();
        let signal = if offset_hz != 0.0 {
            translate(&signal, offset_hz, &mut planner)
        } else {
            signal
        };
        let rms = (energy(&signal[truth.clone()]) / truth.len() as f64).sqrt();
        let interferer = interferer_db.map(|db| {
            let peak = rms * 2.0_f64.sqrt() * 10_f64.powf(db / 20.0);
            let radians = std::f64::consts::TAU * 1500.0 / RATE as f64;
            (0..signal.len())
                .map(|i| (peak * (radians * i as f64).cos()) as f32)
                .collect::<Vec<_>>()
        });
        for snr in (min_snr..=max_snr).step_by(step) {
            let sigma =
                rms * (RATE as f64 / (2.0 * REF_BW)).sqrt() * 10_f64.powf(-f64::from(snr) / 20.0);
            for trial in 0..trials {
                let seed = 1 + fixture_index * 100_000 + trial;
                let noise: Vec<_> = white(seed, clean.len())
                    .into_iter()
                    .enumerate()
                    .map(|(i, x)| {
                        x * sigma as f32 + interferer.as_ref().map_or(0.0, |tone| tone[i])
                    })
                    .collect();
                let impaired: Vec<_> = signal.iter().zip(&noise).map(|(s, n)| s + n).collect();
                // The virtual host flushes at the end of each ingress packet.
                let filtered = bandpass(&impaired, &mut planner);
                let filtered_noise = bandpass(&noise, &mut planner);
                let band = band_candidates(&filtered);
                let noise_band = band_candidates(&filtered_noise);
                let filtered_pass = path_test(&filtered, &band);
                let filtered_false = noise_band
                    .iter()
                    .any(|r| decode(kind, &waveform, &filtered_noise, r.clone()).is_some());
                let base = candidates(&impaired, true);
                let base_streaming = candidates(&impaired, false);
                let noise_base = candidates(&noise, true);
                let flag = u8::from;
                writeln!(
                    csv,
                    "{label},{name},{snr},{seed},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    flag(detect(&base)),
                    flag(detect(&base_streaming)),
                    flag(path_test(&impaired, &base)),
                    flag(detect(&band)),
                    flag(path_test(&impaired, &band)),
                    flag(
                        decode(kind, &waveform, &impaired, truth.clone()).as_ref()
                            == Some(&expected)
                    ),
                    flag(if offset_hz == 0.0 {
                        aligned(kind, &waveform, &truth, &impaired, start).as_ref()
                            == Some(&expected)
                    } else {
                        let corrected = translate(&impaired, -offset_hz, &mut planner);
                        aligned(kind, &waveform, &truth, &corrected, start).as_ref()
                            == Some(&expected)
                    }),
                    flag(!noise_base.is_empty()),
                    flag(!noise_band.is_empty()),
                    flag(
                        noise_base
                            .iter()
                            .any(|r| decode(kind, &waveform, &noise, r.clone()).is_some())
                    ),
                    flag(
                        noise_band
                            .iter()
                            .any(|r| decode(kind, &waveform, &noise, r.clone()).is_some())
                    ),
                    flag(filtered_pass),
                    flag(filtered_false),
                )
                .unwrap();
            }
        }
        csv.flush().unwrap();
    }
}

// Report the normalized two-tone diagnostics used by the stock ACK matcher.
// The threshold experiment retains the aggregate requirement and all 22
// expected tones, but lowers the bottleneck caused by a single faded tone.
fn ack_scores(audio: &[f32], start: usize, key: &str) -> (f64, f64) {
    let pairs = control::connected_ack_tones(Bandwidth::Hz500, key).unwrap();
    let tones = pairs.map(|pair| pair.map(|bin| Tone::new(bin as usize, 2048)));
    (start.saturating_sub(2048)..=start + 2048)
        .step_by(32)
        .map(|offset| {
            let mut total = 0.0;
            let mut weakest = 1.0_f64;
            for (pair, column) in tones.iter().zip(audio[offset..].chunks_exact(2048)) {
                let scale = (energy(column) * 1024.0).max(f64::EPSILON);
                for tone in pair {
                    let score = tone.power(column) / scale;
                    total += score;
                    weakest = weakest.min(score);
                }
            }
            (total / pairs.len() as f64, weakest)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap()
}

#[test]
#[ignore = "requires stock keyed ACK captures"]
fn stock_ack_echo_diagnostic() {
    let manifest = std::fs::read_to_string(env::var_os("BW500_STUDY_INPUT").unwrap()).unwrap();
    let mut csv = BufWriter::new(File::create(env::var_os("BW500_STUDY_CSV").unwrap()).unwrap());
    writeln!(csv, "fixture,channel,snr_db,seed,key,input,total,weakest").unwrap();
    let mut planner = FftPlanner::new();
    for (index, path) in manifest
        .lines()
        .filter_map(|line| line.strip_prefix("ack\t"))
        .enumerate()
    {
        let path = PathBuf::from(path);
        let fixture = path.file_name().unwrap().to_str().unwrap();
        let raw = load(path.clone());
        let truth = candidates(&raw, true)
            .into_iter()
            .max_by_key(|r| r.len())
            .unwrap();
        let mut clean = vec![0.0; GUARD];
        clean.extend_from_slice(&raw[truth.start - 960..truth.end + 960]);
        clean.resize(clean.len() + GUARD, 0.0);
        let start = GUARD + 960;
        for delay in [0, 96] {
            let signal: Vec<_> = clean
                .iter()
                .enumerate()
                .map(|(i, sample)| {
                    sample
                        + if delay > 0 && i >= delay {
                            0.5 * clean[i - delay]
                        } else {
                            0.0
                        }
                })
                .collect();
            let rms = (energy(&signal[start..start + truth.len()]) / truth.len() as f64).sqrt();
            for snr in [0, 4, 8, 12, 24, 44] {
                for seed in 1..=16 {
                    let sigma = rms
                        * (RATE as f64 / (2.0 * REF_BW)).sqrt()
                        * 10_f64.powf(-f64::from(snr) / 20.0);
                    let noise: Vec<_> = white(500_000 + index * 100_000 + seed, signal.len())
                        .into_iter()
                        .map(|sample| sample * sigma as f32)
                        .collect();
                    let received: Vec<_> = signal.iter().zip(&noise).map(|(s, n)| s + n).collect();
                    for (input, audio) in [("signal", received), ("noise", noise)] {
                        let audio = bandpass(&audio, &mut planner);
                        let keys = if input == "signal" {
                            &["W1AW", "W1A", "KN6MQS"][..]
                        } else {
                            &["W1AW"][..]
                        };
                        for &key in keys {
                            let (total, weakest) = ack_scores(&audio, start, key);
                            writeln!(csv, "{fixture},{delay},{snr},{seed},{key},{input},{total:.8},{weakest:.8}").unwrap();
                        }
                    }
                }
            }
        }
    }
}
