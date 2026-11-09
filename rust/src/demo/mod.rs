//! Local, file-backed modem demonstration. All DSP and verification run in Rust.
//! A codec loopback or capture replay is not a live VARA/Winlink session.

mod server;
pub use server::serve;

use crate::modem::{
    bw500::{self, Level},
    bw500_waveform::Waveform,
    data,
    receiver::Receiver,
};
use rustfft::{FftPlanner, num_complex::Complex};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Cursor,
    ops::Range,
    path::{Path, PathBuf},
    time::Instant,
};

const FS: usize = 48_000;
const FFT: usize = 2048;
const HOP: usize = 1024;
const BINS: usize = 129;
const GUARD: usize = 4800;
const MAX_SAMPLES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Native,
    Reference,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    pub source: Source,
    #[serde(default = "default_level")]
    pub level: u8,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub fixture: Option<String>,
    pub snr_db: Option<f32>,
    #[serde(default)]
    pub dropout: bool,
    #[serde(default = "default_seed")]
    pub seed: u64,
}
fn default_level() -> u8 {
    3
}
fn default_seed() -> u64 {
    20_261_109
}

#[derive(Clone, Serialize)]
pub struct FrameReport {
    pub slot: Option<usize>,
    pub level: u8,
    pub start_s: f64,
    pub end_s: f64,
    pub control: u8,
    pub payload_hex: String,
    pub text: String,
    pub matches_expected: bool,
}

#[derive(Clone, Serialize)]
pub struct Event {
    pub at_ms: f64,
    pub stage: &'static str,
    pub detail: String,
}

#[derive(Clone, Serialize)]
pub struct Envelope {
    pub min: Vec<f32>,
    pub max: Vec<f32>,
    pub step_s: f64,
}

#[derive(Clone, Serialize)]
pub struct Analysis {
    pub fft_size: usize,
    pub hop_samples: usize,
    pub bins: usize,
    pub rows: usize,
    pub bin_hz: f64,
    pub time_start_s: f64,
    pub time_step_s: f64,
    pub waterfall_dbfs: Vec<f32>,
    pub selected_row: usize,
    pub envelope: Envelope,
}

#[derive(Clone, Serialize)]
pub struct Report {
    pub id: String,
    pub source: Source,
    pub source_label: String,
    pub level: u8,
    pub expected_text: String,
    pub decoded_text: Option<String>,
    pub payload_verified: bool,
    pub expected_frames: usize,
    pub decoded_frames: usize,
    pub matched_frames: usize,
    pub candidate_count: usize,
    pub sample_rate_hz: usize,
    pub sample_count: usize,
    pub duration_s: f64,
    pub measured_snr_db: Option<f64>,
    pub clipped_samples: usize,
    pub peak: f32,
    pub rms: f64,
    pub seed: u64,
    pub dropout: Option<Range<f64>>,
    pub frames: Vec<FrameReport>,
    pub events: Vec<Event>,
    pub analysis: Analysis,
    pub elapsed_ms: f64,
    pub input_sha256: Option<String>,
}

pub(crate) struct Run {
    pub report: Report,
    pub audio: Vec<f32>,
}

#[derive(Clone)]
pub(crate) struct Context {
    pub profile: Option<String>,
    pub fixtures: PathBuf,
}

#[derive(Clone, Serialize)]
pub(crate) struct Fixture {
    pub id: &'static str,
    pub label: &'static str,
    pub level: u8,
    pub sha256: &'static str,
    pub expected: &'static [&'static str],
}

pub(crate) const FIXTURES: &[Fixture] = &[
    Fixture {
        id: "bw500-level1-arm1",
        label: "Level 1 · stock capture A",
        level: 1,
        sha256: "df1a22c065afccf01daa6e342590c1f7c251c197c5b09816c909aa0f6d69f311",
        expected: &["524d53205472696d6f"],
    },
    Fixture {
        id: "bw500-level1-arm2",
        label: "Level 1 · stock capture B",
        level: 1,
        sha256: "e596bb2fc1a1b37be65e7f2e2ef95d1d8ef00ca1ed66af12a77a4dad202522a3",
        expected: &["524d53205472696d6f"],
    },
    Fixture {
        id: "bw500-level2-arm1",
        label: "Level 2 · stock capture A",
        level: 2,
        sha256: "407cddd522840044094d9a47cd1a7a9752801ce1fb1f89ec9d981d4601c48ce3",
        expected: &["646520312e342e332e302057616473776f7274682049"],
    },
    Fixture {
        id: "bw500-level2-arm2",
        label: "Level 2 · stock capture B",
        level: 2,
        sha256: "990e7298553ac88a763debd8e9564a3ef074047405a2e1a0f2bec3f26d82a911",
        expected: &["646520312e342e332e302057616473776f7274682049"],
    },
    Fixture {
        id: "bw500-level1-two-frames",
        label: "Level 1 · two stock frames",
        level: 1,
        sha256: "49362d3b65662df303afd84932f2685d772c047985a6a7c209d876538613029f",
        expected: &["323520666565742077", "697468203530207761"],
    },
];

pub(crate) fn capacity(level: u8) -> Option<(usize, usize)> {
    match level {
        1 => Some((9, 3)),
        2 => Some((22, 3)),
        3 => Some((34, 3)),
        4 => Some((43, 1)),
        _ => None,
    }
}

fn level(value: u8) -> Result<Level, String> {
    match value {
        1 => Ok(Level::One),
        2 => Ok(Level::Two),
        3 => Ok(Level::Three),
        _ => Err("unsupported index level".into()),
    }
}

fn unhex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("fixture ASCII"), 16)
                .expect("fixture hex")
        })
        .collect()
}

struct Input {
    audio: Vec<f32>,
    expected: Vec<Vec<u8>>,
    starts: Vec<usize>,
    level: u8,
    text_len: usize,
    label: String,
    hash: Option<String>,
}

fn synthesize(request: &RunRequest, context: &Context) -> Result<Input, String> {
    let (bytes, limit) = capacity(request.level).ok_or("level must be 1–4")?;
    let message = request.message.as_bytes();
    if message.is_empty() || message.len() > bytes * limit {
        return Err(format!(
            "level {} accepts 1–{} UTF-8 bytes",
            request.level,
            bytes * limit
        ));
    }
    let payloads: Vec<Vec<u8>> = message
        .chunks(bytes)
        .map(|part| {
            let mut payload = part.to_vec();
            payload.resize(bytes, 0);
            payload
        })
        .collect();
    let mut audio = vec![0.0; GUARD];
    let starts;
    if request.level == 4 {
        let json = context.profile.as_ref().ok_or(
            "level 4 needs the external measurement profile; run tools/fetch-modem-profile.py",
        )?;
        let waveform = Waveform::from_json(json)?;
        let wire: [u8; 46] = data::full(4, &payloads[0], 0x99)
            .ok_or("invalid level-4 payload")?
            .try_into()
            .map_err(|_| "invalid level-4 container")?;
        audio.extend(
            waveform
                .synthesize(&wire)
                .ok_or("level-4 synthesis failed")?,
        );
        starts = vec![GUARD + 4352];
    } else {
        let frames: Vec<_> = payloads
            .iter()
            .map(|payload| bw500::Frame {
                payload: payload.clone(),
                control: 0x99,
            })
            .collect();
        let encoded =
            bw500::synthesize(level(request.level)?, &frames).ok_or("synthesis failed")?;
        let stride = (encoded.len() - 4096) / frames.len();
        starts = (0..frames.len())
            .map(|i| GUARD + 4096 + i * stride)
            .collect();
        audio.extend(encoded);
    }
    audio.extend([0.0; GUARD]);
    // Audio amplitude is a bench convention, unrelated to RF transmit power.
    for sample in &mut audio {
        *sample *= 0.25;
    }
    Ok(Input {
        audio,
        expected: payloads,
        starts,
        level: request.level,
        text_len: message.len(),
        label: "Native Rust codec loopback".into(),
        hash: None,
    })
}

fn replay(request: &RunRequest, context: &Context) -> Result<Input, String> {
    let fixture = FIXTURES
        .iter()
        .find(|f| Some(f.id) == request.fixture.as_deref())
        .ok_or("choose a known reference recording")?;
    let bytes=std::fs::read(context.fixtures.join(format!("{}.wav",fixture.id)))
        .map_err(|_| "reference recording is unavailable; run python3 tools/test-modem.py from the repository root")?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if hash != fixture.sha256 {
        return Err("reference recording SHA-256 mismatch; refusing altered source audio".into());
    }
    let mut wav = hound::WavReader::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let spec = wav.spec();
    if spec.channels != 1
        || spec.sample_rate != FS as u32
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("reference recording must be 48 kHz mono PCM16".into());
    }
    if wav.duration() as usize > MAX_SAMPLES {
        return Err("recording exceeds demo sample limit".into());
    }
    let audio = wav
        .samples::<i16>()
        .map(|sample| sample.map(|v| f32::from(v) / 32768.0))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let expected: Vec<_> = fixture.expected.iter().map(|hex| unhex(hex)).collect();
    let stride = match fixture.level {
        1 => 124 * 2048,
        2 => 228 * 1024,
        _ => return Err("unsupported reference level".into()),
    };
    let starts = (0..expected.len())
        .map(|i| GUARD + 4096 + i * stride)
        .collect();
    let text_len = expected.iter().map(Vec::len).sum();
    Ok(Input {
        audio,
        expected,
        starts,
        level: fixture.level,
        text_len,
        label: fixture.label.into(),
        hash: Some(hash),
    })
}

struct Noise(u64);
impl Noise {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 1.0) / (9_007_199_254_740_992.0 + 1.0)
    }
    fn gaussian(&mut self) -> f64 {
        (-2.0 * self.uniform().ln()).sqrt() * (std::f64::consts::TAU * self.uniform()).cos()
    }
}

pub(crate) fn run(request: RunRequest, context: &Context) -> Result<Run, String> {
    run_streaming(request, context, false, |_| Ok(()))
}

#[derive(Serialize)]
pub(super) struct Progress {
    received_samples: usize,
    total_samples: usize,
    duration_s: f64,
    decoded_frames: usize,
    decoded_text: String,
    analysis: Analysis,
}

pub(super) fn run_streaming(
    request: RunRequest,
    context: &Context,
    streaming: bool,
    mut progress: impl FnMut(Progress) -> Result<(), String>,
) -> Result<Run, String> {
    if request
        .snr_db
        .is_some_and(|v| !v.is_finite() || !(-5.0..=40.0).contains(&v))
    {
        return Err("channel SNR must be between -5 and 40 dB".into());
    }
    let started = Instant::now();
    let mut events = Vec::new();
    let mut event = |stage, detail| {
        events.push(Event {
            at_ms: started.elapsed().as_secs_f64() * 1000.0,
            stage,
            detail,
        })
    };
    let input = match request.source {
        Source::Native => synthesize(&request, context)?,
        Source::Reference => replay(&request, context)?,
    };
    if input.audio.len() > MAX_SAMPLES {
        return Err("demo exceeds sample budget".into());
    }
    event(
        "input",
        format!(
            "{} · {} expected frame(s)",
            input.label,
            input.expected.len()
        ),
    );
    let mut audio = input.audio.clone();
    let active = GUARD..audio.len() - GUARD;
    let signal_power = input.audio[active.clone()]
        .iter()
        .map(|v| f64::from(*v).powi(2))
        .sum::<f64>()
        / active.len() as f64;
    let mut measured_snr = None;
    if let Some(snr) = request.snr_db {
        let scale = (signal_power / 10.0_f64.powf(f64::from(snr) / 10.0)).sqrt();
        let mut noise = Noise(request.seed.max(1));
        let mut noise_power = 0.0;
        for (i, sample) in audio.iter_mut().enumerate() {
            let value = noise.gaussian() * scale;
            *sample += value as f32;
            if active.contains(&i) {
                noise_power += value * value;
            }
        }
        measured_snr = Some(10.0 * (signal_power / (noise_power / active.len() as f64)).log10());
        event(
            "channel",
            format!("Seeded Gaussian noise · requested {snr:.0} dB SNR"),
        );
    } else {
        event("channel", "Clean sample path; no noise injection".into());
    }
    let dropout = request.dropout.then(|| {
        let range = active.start + active.len() / 3..active.start + active.len() * 2 / 3;
        audio[range.clone()].fill(0.0);
        range.start as f64 / FS as f64..range.end as f64 / FS as f64
    });
    if dropout.is_some() {
        event(
            "channel",
            "Middle third erased; the receiver must detect and validate what remains".into(),
        );
    }
    let mut receiver = if input.level == 4 {
        Receiver::with_level4(Waveform::from_json(
            context.profile.as_ref().ok_or("missing level-4 profile")?,
        )?)
    } else {
        Receiver::new()
    };
    let mut receptions = Vec::new();
    let mut received = 0;
    let mut next_update = 5120;
    for block in audio.chunks(1024) {
        receiver
            .push(block, |event| receptions.push(event))
            .map_err(|e| e.to_string())?;
        received += block.len();
        if streaming && (received >= next_update || received == audio.len()) {
            progress(Progress {
                received_samples: received,
                total_samples: audio.len(),
                duration_s: audio.len() as f64 / FS as f64,
                decoded_frames: receptions.iter().map(|r| r.frames.len()).sum(),
                decoded_text: receptions
                    .iter()
                    .flat_map(|r| &r.frames)
                    .map(|f| {
                        String::from_utf8_lossy(&f.frame.payload)
                            .trim_end_matches('\0')
                            .to_owned()
                    })
                    .collect::<Vec<_>>()
                    .join(""),
                analysis: analyze(&audio[..received]),
            })?;
            next_update = received + 5120;
        }
    }
    receiver.finish(|event| receptions.push(event));
    let candidate_count = receptions.len();
    event(
        "acquisition",
        format!("{candidate_count} energy candidate(s); candidates are not connections"),
    );
    let mut matched = vec![false; input.expected.len()];
    let mut recovered = vec![None; input.expected.len()];
    let frames: Vec<_> = receptions
        .into_iter()
        .flat_map(|r| r.frames)
        .map(|decoded| {
            let slot = input
                .starts
                .iter()
                .enumerate()
                .min_by_key(|(_, start)| decoded.samples.start.abs_diff(**start as u64))
                .filter(|(_, start)| decoded.samples.start.abs_diff(**start as u64) <= 1024)
                .map(|(i, _)| i);
            let valid = slot.is_some_and(|i| {
                decoded.level == input.level
                    && decoded.frame.control == 0x99
                    && decoded.frame.payload == input.expected[i]
            });
            if let Some(i) = slot
                && valid
            {
                matched[i] = true;
                recovered[i] = Some(decoded.frame.payload.clone());
            }
            FrameReport {
                slot,
                level: decoded.level,
                start_s: decoded.samples.start as f64 / FS as f64,
                end_s: decoded.samples.end as f64 / FS as f64,
                control: decoded.frame.control,
                payload_hex: decoded
                    .frame
                    .payload
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect(),
                text: String::from_utf8_lossy(&decoded.frame.payload)
                    .trim_end_matches('\0')
                    .into(),
                matches_expected: valid,
            }
        })
        .collect();
    let verified = matched.iter().all(|v| *v) && frames.len() == input.expected.len();
    let expected_bytes: Vec<_> = input
        .expected
        .iter()
        .flatten()
        .copied()
        .take(input.text_len)
        .collect();
    let decoded_text = verified.then(|| {
        let bytes: Vec<_> = recovered
            .into_iter()
            .flatten()
            .flatten()
            .take(input.text_len)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    });
    event(
        "decode",
        format!(
            "{} CRC-valid frame(s); {} of {} expected frames matched",
            frames.len(),
            matched.iter().filter(|v| **v).count(),
            input.expected.len()
        ),
    );
    event(
        "verify",
        if verified {
            "Payload bytes match the expected input".into()
        } else {
            "Complete payload verification failed; missing frames are not treated as delivered"
                .into()
        },
    );
    let analysis = analyze(&audio);
    event(
        "analysis",
        format!(
            "Rust Hann STFT · {FFT} samples · {HOP}-sample hop · {} rows",
            analysis.rows
        ),
    );
    let peak = audio.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
    let rms =
        (audio.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / audio.len() as f64).sqrt();
    Ok(Run {
        report: Report {
            id: uuid::Uuid::new_v4().to_string(),
            source: request.source,
            source_label: input.label,
            level: input.level,
            expected_text: String::from_utf8_lossy(&expected_bytes).into_owned(),
            decoded_text,
            payload_verified: verified,
            expected_frames: input.expected.len(),
            decoded_frames: frames.len(),
            matched_frames: matched.iter().filter(|v| **v).count(),
            candidate_count,
            sample_rate_hz: FS,
            sample_count: audio.len(),
            duration_s: audio.len() as f64 / FS as f64,
            measured_snr_db: measured_snr,
            clipped_samples: audio.iter().filter(|v| v.abs() > 1.0).count(),
            peak,
            rms,
            seed: request.seed,
            dropout,
            frames,
            events,
            analysis,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            input_sha256: input.hash,
        },
        audio,
    })
}

fn analyze(audio: &[f32]) -> Analysis {
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT);
    let window: Vec<_> = (0..FFT)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (FFT - 1) as f32).cos())
        .collect();
    let gain = window.iter().sum::<f32>();
    let mut buffer = vec![Complex::new(0.0, 0.0); FFT];
    let mut scratch = vec![Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
    let mut waterfall = Vec::new();
    let mut best_energy = -1.0_f32;
    let mut selected = 0;
    for (row, start) in (0..=audio.len() - FFT).step_by(HOP).enumerate() {
        let mut energy = 0.0;
        for i in 0..FFT {
            buffer[i] = Complex::new(audio[start + i] * window[i], 0.0);
            energy += audio[start + i].powi(2);
        }
        fft.process_with_scratch(&mut buffer, &mut scratch);
        if energy > best_energy {
            best_energy = energy;
            selected = row;
        }
        waterfall.extend(buffer[..BINS].iter().enumerate().map(|(i, value)| {
            let amplitude = value.norm() * (if i == 0 { 1.0 } else { 2.0 }) / gain;
            (20.0 * amplitude.max(0.000001).log10()).max(-120.0)
        }));
    }
    let stride = 512;
    let mut min = Vec::new();
    let mut max = Vec::new();
    for block in audio.chunks(stride) {
        min.push(block.iter().copied().fold(f32::INFINITY, f32::min));
        max.push(block.iter().copied().fold(f32::NEG_INFINITY, f32::max));
    }
    Analysis {
        fft_size: FFT,
        hop_samples: HOP,
        bins: BINS,
        rows: waterfall.len() / BINS,
        bin_hz: FS as f64 / FFT as f64,
        time_start_s: FFT as f64 / 2.0 / FS as f64,
        time_step_s: HOP as f64 / FS as f64,
        waterfall_dbfs: waterfall,
        selected_row: selected,
        envelope: Envelope {
            min,
            max,
            step_s: stride as f64 / FS as f64,
        },
    }
}

pub(crate) fn wav(audio: &[f32]) -> Result<Vec<u8>, String> {
    let mut output = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(
            &mut output,
            hound::WavSpec {
                channels: 1,
                sample_rate: FS as u32,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .map_err(|e| e.to_string())?;
        for &sample in audio {
            writer.write_sample(sample).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;
    }
    Ok(output.into_inner())
}

pub(crate) fn context() -> Result<Context, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repository root");
    let explicit = std::env::var_os("LINKWISE_BW500_PROFILE");
    let path = explicit
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("reference/vara/fixtures/level4/bw500-level4.json"));
    let profile = if path.exists() || explicit.is_some() {
        let json = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read level-4 profile: {e}"))?;
        Waveform::from_json(&json)?;
        Some(json)
    } else {
        None
    };
    let fixtures = std::env::var_os("LINKWISE_BW500_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("reference/vara/fixtures/bw500"));
    Ok(Context { profile, fixtures })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ctx() -> Context {
        Context {
            profile: None,
            fixtures: PathBuf::from("/unavailable"),
        }
    }
    fn request(level: u8) -> RunRequest {
        RunRequest {
            source: Source::Native,
            level,
            message: "HELLO FROM RUST".into(),
            fixture: None,
            snr_db: None,
            dropout: false,
            seed: 42,
        }
    }
    #[test]
    fn actual_codec_roundtrips_and_stft_has_correct_tone_axis() {
        for level in 1..=3 {
            let run = run(request(level), &ctx()).unwrap();
            assert!(run.report.payload_verified, "level {level}");
            assert_eq!(run.report.decoded_text.as_deref(), Some("HELLO FROM RUST"));
            assert_eq!(run.report.analysis.bins, 129);
            assert_eq!(
                run.report.analysis.waterfall_dbfs.len(),
                run.report.analysis.rows * 129
            );
            assert!(
                run.report
                    .analysis
                    .waterfall_dbfs
                    .iter()
                    .all(|v| v.is_finite())
            );
            let bytes = wav(&run.audio).unwrap();
            let wav = hound::WavReader::new(Cursor::new(bytes)).unwrap();
            assert_eq!(wav.duration() as usize, run.audio.len());
        }
        let tone: Vec<_> = (0..FS)
            .map(|i| 0.5 * (std::f32::consts::TAU * 1500.0 * i as f32 / FS as f32).sin())
            .collect();
        let result = analyze(&tone);
        let spectrum = &result.waterfall_dbfs[..BINS];
        let peak = spectrum
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap();
        assert_eq!(peak.0, 64);
        assert!((*peak.1 + 6.0206).abs() < 0.05);
    }
    #[test]
    fn seeded_channel_is_reproducible_and_dropout_is_not_reported_as_success() {
        let mut input = request(3);
        input.snr_db = Some(24.0);
        let a = run(input.clone(), &ctx()).unwrap();
        let b = run(input, &ctx()).unwrap();
        assert_eq!(a.audio, b.audio);
        assert!(a.report.payload_verified);
        assert!((a.report.measured_snr_db.unwrap() - 24.0).abs() < 0.2);
        let mut input = request(3);
        input.dropout = true;
        let failed = run(input, &ctx()).unwrap();
        assert!(!failed.report.payload_verified);
        assert!(failed.report.decoded_text.is_none());
    }
    #[test]
    fn streaming_progress_tracks_samples_and_cancellation_stops_decode() {
        let mut last = 0;
        let mut updates = 0;
        let streamed = run_streaming(request(3), &ctx(), true, |p| {
            assert!(p.received_samples > last);
            assert!(p.received_samples <= p.total_samples);
            assert_eq!(p.analysis.waterfall_dbfs.len(), p.analysis.rows * BINS);
            last = p.received_samples;
            updates += 1;
            Ok(())
        })
        .unwrap();
        assert!(updates > 1);
        assert_eq!(last, streamed.audio.len());
        assert!(streamed.report.payload_verified);
        let cancelled = run_streaming(request(3), &ctx(), true, |_| Err("cancelled".into()));
        assert_eq!(cancelled.err().as_deref(), Some("cancelled"));
    }

    #[test]
    fn rejects_oversized_messages_and_nonexistent_reference_ids() {
        let mut input = request(1);
        input.message = "X".repeat(28);
        assert!(run(input, &ctx()).is_err());
        let mut input = request(1);
        input.source = Source::Reference;
        input.fixture = Some("../../secret".into());
        assert!(run(input, &ctx()).is_err());
    }
}
