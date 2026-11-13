use std::{
    error::Error,
    ffi::OsString,
    fs::File,
    io::{self, BufWriter, Read, Write},
    path::Path,
};

use linkwise::modem::bw500::{self, Frame, Level};

pub(crate) struct Failure {
    pub status: i32,
    pub source: Box<dyn Error>,
}

impl Failure {
    pub fn operation(error: impl Into<Box<dyn Error>>) -> Self {
        Self {
            status: 1,
            source: error.into(),
        }
    }

    pub fn usage(message: &'static str) -> Self {
        Self {
            status: 2,
            source: message.into(),
        }
    }
}

pub(crate) fn interruption(event: io::Result<i32>, message: &'static str) -> Failure {
    match event {
        Ok(status) => Failure {
            status,
            source: message.into(),
        },
        Err(error) => Failure::operation(error),
    }
}

pub(crate) async fn run(args: &[OsString]) -> Result<(), Failure> {
    let command = args[0].to_str().expect("recognized command");
    match (command, &args[1..]) {
        ("decode-frame", [input]) => decode(Path::new(input)).map_err(Failure::operation),
        ("encode-frame", [level, control, input, output, ..]) if args.len() <= 6 => {
            let pcm = match args.get(5).map(|format| format.to_str()) {
                None | Some(Some("wav")) => false,
                Some(Some("pcm-f32le")) => true,
                _ => return Err(Failure::usage("format must be wav or pcm-f32le")),
            };
            let samples = synthesize(level, control, input)?;
            write_audio(Path::new(output), &samples, pcm).map_err(Failure::operation)
        }
        ("transmit-frame", [level, control, input]) => {
            let (level, control, input) = (level.clone(), control.clone(), input.clone());
            let operation =
                async {
                    let (sender, receiver) = tokio::sync::oneshot::channel();
                    // Stdin can block indefinitely. Dropping the receiver lets the
                    // process exit without waiting for the input thread.
                    std::thread::spawn(move || {
                        let _ = sender.send(
                            synthesize(&level, &control, &input)
                                .map_err(|error| (error.status, error.source.to_string())),
                        );
                    });
                    let samples = receiver.await.map_err(Failure::operation)?.map_err(
                        |(status, message)| Failure {
                            status,
                            source: message.into(),
                        },
                    )?;
                    play(samples).await.map_err(Failure::operation)
                };
            tokio::select! {
                biased;
                event = linkwise::signals::stop() => Err(interruption(event, "audio transmission interrupted")),
                result = operation => result,
            }
        }
        _ => Err(Failure::usage(
            "invalid frame command operands; see linkwise --help",
        )),
    }
}

fn synthesize(level: &OsString, control: &OsString, input: &OsString) -> Result<Vec<f32>, Failure> {
    let (level, size) = match level.to_str() {
        Some("1") => (Level::One, 9),
        Some("2") => (Level::Two, 22),
        Some("3") => (Level::Three, 34),
        _ => return Err(Failure::usage("BW500 level must be 1, 2, or 3")),
    };
    let control = control
        .to_str()
        .and_then(|value| {
            match value
                .strip_prefix("0x")
                .or_else(|| value.strip_prefix("0X"))
            {
                Some(hex) => u8::from_str_radix(hex, 16).ok(),
                None => value.parse().ok(),
            }
        })
        .ok_or_else(|| Failure::usage("control must be a decimal or hexadecimal byte"))?;
    let mut payload = Vec::with_capacity(size + 1);
    let input: Box<dyn Read> = if input == "-" {
        Box::new(io::stdin().lock())
    } else {
        Box::new(File::open(input).map_err(Failure::operation)?)
    };
    input
        .take((size + 1) as u64)
        .read_to_end(&mut payload)
        .map_err(Failure::operation)?;
    if payload.len() != size {
        return Err(Failure::operation(format!(
            "BW500 level {} requires exactly {size} payload bytes",
            level.number()
        )));
    }
    let samples = bw500::synthesize(level, &[Frame { payload, control }])
        .ok_or_else(|| Failure::operation("BW500 frame synthesis failed"))?;
    Ok(samples)
}

fn decode(path: &Path) -> Result<(), Box<dyn Error>> {
    let report = super::wav_analysis::analyze(path, true)?;
    let frames = report.bw500_data_frames.unwrap_or_default();
    let [frame] = frames.as_slice() else {
        return Err(format!(
            "expected exactly one CRC-valid frame, found {}",
            frames.len()
        )
        .into());
    };
    let mut stdout = io::stdout().lock();
    for offset in (0..frame.payload_hex.len()).step_by(2) {
        stdout.write_all(&[u8::from_str_radix(
            &frame.payload_hex[offset..offset + 2],
            16,
        )?])?;
    }
    stdout.flush()?;
    Ok(())
}

fn write_audio(path: &Path, samples: &[f32], pcm: bool) -> io::Result<()> {
    let output: Box<dyn Write> = if path == Path::new("-") {
        Box::new(io::stdout().lock())
    } else {
        Box::new(File::create(path)?)
    };
    let mut output = BufWriter::new(output);
    if !pcm {
        // The complete burst is available, so stdout needs no seeking.
        let len = (samples.len() * 4) as u32;
        output.write_all(b"RIFF")?;
        output.write_all(&(48 + len).to_le_bytes())?;
        output.write_all(b"WAVEfmt ")?;
        output.write_all(&16u32.to_le_bytes())?;
        output.write_all(&3u16.to_le_bytes())?; // IEEE float
        output.write_all(&1u16.to_le_bytes())?;
        output.write_all(&48_000u32.to_le_bytes())?;
        output.write_all(&192_000u32.to_le_bytes())?;
        output.write_all(&4u16.to_le_bytes())?;
        output.write_all(&32u16.to_le_bytes())?;
        output.write_all(b"fact")?;
        output.write_all(&4u32.to_le_bytes())?;
        output.write_all(&(samples.len() as u32).to_le_bytes())?;
        output.write_all(b"data")?;
        output.write_all(&len.to_le_bytes())?;
    }
    for sample in samples {
        output.write_all(&sample.to_le_bytes())?;
    }
    output.flush()
}

#[cfg(feature = "audio-device")]
async fn play(samples: Vec<f32>) -> Result<(), Box<dyn Error>> {
    use linkwise::audio::playback::{Playback, Progress};
    use std::time::{Duration, Instant};

    let name = std::env::var("LINKWISE_AUDIO_OUTPUT")?;
    let channel = match std::env::var("LINKWISE_AUDIO_OUTPUT_CHANNEL") {
        Ok(value) => value.parse()?,
        Err(std::env::VarError::NotPresent) => 0,
        Err(error) => return Err(error.into()),
    };
    let mut playback = Playback::start(&name, channel, samples)?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if playback.progress()? == Progress::EstimateElapsed {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("timed out waiting for audio presentation estimate".into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(not(feature = "audio-device"))]
async fn play(_: Vec<f32>) -> Result<(), Box<dyn Error>> {
    Err("transmit-frame requires an audio-device build".into())
}
