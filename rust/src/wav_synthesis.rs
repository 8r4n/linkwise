//! File-only bench stimulus generation; never plays audio or asserts PTT.
use linkwise::modem::{
    bw500_waveform::Waveform,
    control::{self, Bandwidth, Message},
    setup::{LinkSetup, Profile},
};
use serde::Deserialize;
use std::{error::Error, path::Path};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Stimulus {
    Request { called: String },
    Setup { caller: String },
    Confirm { called: String },
    Ack { caller: String },
}

pub(crate) fn render(input: &Path, output: &Path) -> Result<usize, Box<dyn Error>> {
    let request: Stimulus = serde_json::from_slice(&std::fs::read(input)?)?;
    let audio = match request {
        Stimulus::Request { called } => {
            Message::ConnectRequest(Bandwidth::Hz500).synthesize(&called)
        }
        Stimulus::Confirm { called } => Message::Bw500Confirm.synthesize(&called),
        Stimulus::Ack { caller } => control::synthesize_connected_ack(Bandwidth::Hz500, &caller),
        Stimulus::Setup { caller } => {
            let path = std::env::var_os("LINKWISE_BW500_PROFILE")
                .ok_or("setup synthesis requires LINKWISE_BW500_PROFILE")?;
            let waveform = Waveform::from_json(&std::fs::read_to_string(path)?)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            LinkSetup {
                caller,
                high_speed: false,
                requested_level: 4,
            }
            .encode(Profile::Bw500)
            .and_then(|frame| frame.try_into().ok())
            .and_then(|frame| waveform.synthesize(&frame))
        }
    }
    .ok_or("invalid bench stimulus identity or frame")?;
    let mut writer = hound::WavWriter::create(
        output,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for &sample in &audio {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(audio.len())
}
