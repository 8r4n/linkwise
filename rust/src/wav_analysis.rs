//! Offline WAV replay for the bench receiver.

use std::{error::Error, ops::Range, path::Path};

use hound::{SampleFormat, WavReader};
use linkwise::modem::{
    bw500,
    receiver::{Receiver, Reception},
    waveform::BurstDetector,
};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct Report {
    sample_rate_hz: u32,
    sample_count: u32,
    burst_candidates: Vec<Range<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bw500_data_frames: Option<Vec<FrameReport>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    truncated_energy_candidates: Option<usize>,
}

#[derive(Serialize)]
struct FrameReport {
    level: u8,
    samples: Range<u64>,
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
        truncated_energy_candidates: None,
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
    let mut receiver = if decode {
        Some(configured_receiver()?)
    } else {
        None
    };
    let mut receptions = Vec::new();
    for sample in samples {
        let sample = sample?;
        if !sample.is_finite() {
            return Err("WAV contains a non-finite sample".into());
        }
        block[filled] = sample;
        filled += 1;
        if filled == block.len() {
            detector.push(&block, |burst| report.burst_candidates.push(burst));
            if let Some(receiver) = &mut receiver {
                receiver.push(&block, |event| receptions.push(event))?;
            }
            filled = 0;
        }
    }
    detector.push(&block[..filled], |burst| {
        report.burst_candidates.push(burst)
    });
    detector.finish(|burst| report.burst_candidates.push(burst));
    if let Some(mut receiver) = receiver {
        receiver.push(&block[..filled], |event| receptions.push(event))?;
        receiver.finish(|event| receptions.push(event));
        let truncated = receptions.iter().filter(|event| event.truncated).count();
        if truncated != 0 {
            report.truncated_energy_candidates = Some(truncated);
        }
        report.bw500_data_frames = Some(
            receptions
                .into_iter()
                .flat_map(|event: Reception| event.frames)
                .map(|decoded| FrameReport {
                    level: decoded.level,
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

pub(crate) fn configured_receiver() -> Result<Receiver, Box<dyn Error>> {
    match std::env::var_os("LINKWISE_BW500_PROFILE") {
        None => Ok(Receiver::new().with_pilot_acquisition()),
        Some(path) => {
            let json = std::fs::read_to_string(path)?;
            let waveform = linkwise::modem::bw500_waveform::Waveform::from_json(&json)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            Ok(Receiver::with_level4(waveform).with_pilot_acquisition())
        }
    }
}
