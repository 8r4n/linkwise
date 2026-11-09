//! Offline WAV replay for the bench receiver.

use std::{error::Error, ops::Range, path::Path};

use hound::{SampleFormat, WavReader};
use linkwise::modem::{bw500, waveform::BurstDetector};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct Report {
    sample_rate_hz: u32,
    sample_count: u32,
    burst_candidates: Vec<Range<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bw500_data_frames: Option<Vec<FrameReport>>,
}

#[derive(Serialize)]
struct FrameReport {
    level: u8,
    samples: Range<usize>,
    payload_hex: String,
    control: u8,
}

pub(crate) fn analyze(path: &Path, decode: bool) -> Result<Report, Box<dyn Error>> {
    let mut wav = WavReader::open(path)?;
    let spec = wav.spec();
    if spec.channels != 1 || spec.sample_rate == 0 {
        return Err("expected a mono WAV with a nonzero sample rate".into());
    }
    if decode && spec.sample_rate != bw500::SAMPLE_RATE_HZ {
        return Err("BW500 decoding requires a 48 kHz mono WAV".into());
    }

    let mut report = Report {
        sample_rate_hz: spec.sample_rate,
        sample_count: wav.duration(),
        burst_candidates: Vec::new(),
        bw500_data_frames: None,
    };
    let mut detector = BurstDetector::new(spec.sample_rate);
    let samples: Box<dyn Iterator<Item = Result<f32, hound::Error>>> =
        match (spec.sample_format, spec.bits_per_sample) {
            (SampleFormat::Int, 16) => Box::new(
                wav.samples::<i16>()
                    .map(|sample| sample.map(|value| f32::from(value) / 32768.0)),
            ),
            (SampleFormat::Float, 32) => Box::new(wav.samples::<f32>()),
            _ => return Err("expected 16-bit PCM or 32-bit float WAV samples".into()),
        };

    let mut block = [0.0; 1024];
    let mut filled = 0;
    let mut audio = Vec::new();
    for sample in samples {
        let sample = sample?;
        if !sample.is_finite() {
            return Err("WAV contains a non-finite sample".into());
        }
        if decode {
            audio.push(sample);
        }
        block[filled] = sample;
        filled += 1;
        if filled == block.len() {
            detector.push(&block, |burst| report.burst_candidates.push(burst));
            filled = 0;
        }
    }
    detector.push(&block[..filled], |burst| {
        report.burst_candidates.push(burst)
    });
    detector.finish(|burst| report.burst_candidates.push(burst));
    if decode {
        report.bw500_data_frames = Some(
            report
                .burst_candidates
                .iter()
                .flat_map(|range| {
                    bw500::decode_burst(&audio, range.start as usize..range.end as usize)
                })
                .map(|decoded| FrameReport {
                    level: decoded.level.number(),
                    samples: decoded.samples,
                    payload_hex: decoded
                        .frame
                        .payload
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                    control: decoded.frame.control,
                })
                .collect(),
        );
    }
    Ok(report)
}
