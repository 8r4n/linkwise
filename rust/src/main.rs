use std::{env, path::Path, sync::Arc};

mod wav_analysis;
mod wav_synthesis;

use linkwise::{
    mcp,
    radio::{RadioControlPort, RigctldRadio, SimulatedRadio},
};

const USAGE: &str = "linkwise — radio-control and modem research service

Usage:
  linkwise                 Start the MCP server on stdio
  linkwise analyze-wav PATH  Report bench burst candidates as JSON
  linkwise decode-wav PATH   Decode BW500 DATA (level 4 requires measurement profile) from a 48 kHz mono WAV
  linkwise audio-inputs      List input devices (requires audio-device feature)
  linkwise audio-outputs     List output devices (requires audio-device feature)
  linkwise receive-audio SECONDS  Receive BW500 DATA as JSON lines
  linkwise render-wav REQUEST_JSON OUTPUT_WAV  Render a file-only BW500 handshake stimulus
  linkwise --help
  linkwise --version

Environment:
  LINKWISE_RADIO_BACKEND=simulated|rigctld (default: simulated)
  LINKWISE_RIGCTLD_ADDR=host:port (default: hamlib:4532)
  LINKWISE_RADIO_WRITES=true (disabled by default; requires station limits)
  LINKWISE_AUDIO_INPUT=exact device name (default: system input)
  LINKWISE_AUDIO_CHANNEL=zero-based channel (default: 0)
  LINKWISE_BW500_PROFILE=path to verified level-4 measurement JSON (optional)

Receive commands report CRC-valid DATA frames, not established sessions.";

#[tokio::main]
async fn main() {
    let mut args = env::args_os().skip(1);
    let arguments = (args.next(), args.next(), args.next());
    if args.next().is_some() {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    match arguments {
        (None, None, None) => {
            let backend = env::var("LINKWISE_RADIO_BACKEND").unwrap_or_else(|_| "simulated".into());
            let radio: Arc<dyn RadioControlPort> = match backend.as_str() {
                "simulated" => Arc::new(SimulatedRadio::from_env()),
                "rigctld" => Arc::new(RigctldRadio::from_env()),
                other => {
                    eprintln!("unsupported LINKWISE_RADIO_BACKEND: {other}");
                    std::process::exit(2);
                }
            };
            if let Err(error) = mcp::serve(radio).await {
                eprintln!("MCP server failed: {error}");
                std::process::exit(1);
            }
        }
        (Some(arg), None, None) if arg == "--help" || arg == "-h" => {
            println!("{USAGE}");
        }
        (Some(arg), None, None) if arg == "--version" || arg == "-V" => {
            println!("linkwise {}", env!("CARGO_PKG_VERSION"));
        }
        #[cfg(feature = "audio-device")]
        (Some(arg), None, None) if arg == "audio-outputs" => {
            match linkwise::audio::playback::output_devices() {
                Ok(devices) => println!("{}", serde_json::to_string_pretty(&devices).unwrap()),
                Err(error) => {
                    eprintln!("Audio enumeration failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        #[cfg(feature = "audio-device")]
        (Some(arg), None, None) if arg == "audio-inputs" => {
            match linkwise::audio::device::input_devices() {
                Ok(devices) => println!("{}", serde_json::to_string_pretty(&devices).unwrap()),
                Err(error) => {
                    eprintln!("Audio enumeration failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        #[cfg(feature = "audio-device")]
        (Some(arg), Some(seconds), None) if arg == "receive-audio" => {
            if let Err(error) = receive_audio(&seconds.to_string_lossy()) {
                eprintln!("Audio receive failed: {error}");
                std::process::exit(1);
            }
        }
        (Some(arg), Some(path), None) if arg == "analyze-wav" || arg == "decode-wav" => {
            match wav_analysis::analyze(Path::new(&path), arg == "decode-wav") {
                Ok(report) => println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serialize WAV report")
                ),
                Err(error) => {
                    eprintln!("WAV analysis failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        (Some(arg), Some(input), Some(output)) if arg == "render-wav" => {
            match wav_synthesis::render(Path::new(&input), Path::new(&output)) {
                Ok(count) => println!(
                    "{}",
                    serde_json::json!({"sample_rate_hz":48000, "samples":count})
                ),
                Err(error) => {
                    eprintln!("WAV synthesis failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

#[cfg(feature = "audio-device")]
fn receive_audio(seconds: &str) -> Result<(), Box<dyn std::error::Error>> {
    use linkwise::audio::device::Capture;
    use std::{
        io::Write,
        time::{Duration, Instant},
    };

    let seconds: u64 = seconds.parse()?;
    if !(1..=3600).contains(&seconds) {
        return Err("receive duration must be 1–3600 seconds".into());
    }
    let name = env::var("LINKWISE_AUDIO_INPUT").ok();
    let channel: u16 = env::var("LINKWISE_AUDIO_CHANNEL")
        .unwrap_or_else(|_| "0".into())
        .parse()?;
    let capture = Capture::start(name.as_deref(), channel)?;
    let mut receiver = wav_analysis::configured_receiver()?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut output = std::io::BufWriter::new(std::io::stdout().lock());
    let mut output_error = None;
    let failed = std::cell::Cell::new(false);
    let mut emit = |event: linkwise::modem::receiver::Reception| {
        if output_error.is_some() {
            return;
        }
        let frames: Vec<_> = event.frames.into_iter().map(|frame| serde_json::json!({
            "level": frame.level, "samples": frame.samples,
            "control": frame.frame.control,
            "payload_hex": frame.frame.payload.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        })).collect();
        let event = serde_json::json!({"burst_candidate": event.candidate,
            "truncated": event.truncated, "bw500_data_frames": frames});
        if let Err(error) = writeln!(output, "{event}").and_then(|_| output.flush()) {
            output_error = Some(error);
            failed.set(true);
        }
    };
    while Instant::now() < deadline && !failed.get() {
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(250));
        if let Some(block) = capture.read(wait)? {
            receiver.push(&block.samples, &mut emit)?;
        }
    }
    if !failed.get() {
        receiver.finish(&mut emit);
    }
    if let Some(error) = output_error {
        return Err(error.into());
    }
    Ok(())
}
