use std::{env, path::Path, sync::Arc};

mod frame_cli;
mod mail_cli;
mod session_cli;
#[cfg(feature = "secure-transfer")]
mod secure_cli;
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
  linkwise encode-frame LEVEL CONTROL INPUT OUTPUT [wav|pcm-f32le]
  linkwise decode-frame INPUT  Write one decoded payload as raw bytes
  linkwise transmit-frame LEVEL CONTROL INPUT  Play one BW500 frame
  linkwise send SOURCE DESTINATION INPUT [COMMAND_PORT DATA_PORT]  Send via local P2P modem
  linkwise mail queue MAILBOX ENVELOPE
  linkwise mail import MAILBOX MESSAGE.eml
  linkwise mail export MAILBOX MESSAGE_ID
  linkwise mail smtp MAILBOX CALLSIGN [PORT [--tls CERT.pem KEY.pem]]
  linkwise mail imap MAILBOX [PORT [--tls CERT.pem KEY.pem]]
  linkwise mail exchange|listen MAILBOX LOCAL PEER [COMMAND_PORT DATA_PORT]
  linkwise secure send|receive CONFIG FILE  Encrypted P2P transfer (requires secure-transfer feature)
  linkwise --help
  linkwise --version

Environment:
  LINKWISE_RADIO_BACKEND=simulated|rigctld (default: simulated)
  LINKWISE_RIGCTLD_ADDR=host:port (default: hamlib:4532)
  LINKWISE_RADIO_WRITES=true (disabled by default; requires station limits)
  LINKWISE_AUDIO_INPUT=exact device name (default: system input)
  LINKWISE_AUDIO_CHANNEL=zero-based channel (default: 0)
  LINKWISE_BW500_PROFILE=path to verified level-4 measurement JSON (optional)

Frame commands accept '-' for stdin or stdout where applicable.
LEVEL is 1, 2, or 3. CONTROL is a decimal or 0x-prefixed byte.
INPUT to encode-frame or transmit-frame must be exactly 9, 22, or 34 bytes.
LINKWISE_AUDIO_OUTPUT names the playback device; LINKWISE_AUDIO_OUTPUT_CHANNEL defaults to 0.
Use -- before operands to end option parsing.

Receive commands report CRC-valid DATA frames, not established sessions.";

#[tokio::main]
async fn main() {
    use std::io::Write;

    let mut args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "secure") {
        #[cfg(feature = "secure-transfer")]
        {
            if let Err(error) = secure_cli::run(&args[1..]).await {
                fail(error.status, error.source);
            }
            return;
        }
        #[cfg(not(feature = "secure-transfer"))]
        fail(2, "secure transfer support is unavailable in this build".into());
    }
    if matches!(
        args.first().and_then(|arg| arg.to_str()),
        Some("encode-frame" | "decode-frame" | "transmit-frame" | "send" | "mail")
    ) {
        if args.get(1).is_some_and(|arg| arg == "--") {
            args.remove(1);
        }
        let result = if args.len() == 2 && matches!(args[1].to_str(), Some("--help" | "-h")) {
            writeln!(std::io::stdout().lock(), "{USAGE}").map_err(frame_cli::Failure::operation)
        } else if args[0] == "mail" {
            mail_cli::run(&args[1..]).await
        } else if args[0] == "send" {
            session_cli::run(&args[1..]).await
        } else {
            frame_cli::run(&args).await
        };
        if let Err(error) = result {
            fail(error.status, error.source);
        }
        return;
    }
    if args.len() >= 3 && args[1] == "--" {
        args.remove(1);
    }
    let mut args = args.into_iter();
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
            output(format_args!("{USAGE}"));
        }
        (Some(arg), None, None) if arg == "--version" || arg == "-V" => {
            output(format_args!("linkwise {}", env!("CARGO_PKG_VERSION")));
        }
        #[cfg(feature = "audio-device")]
        (Some(arg), None, None) if arg == "audio-outputs" => {
            match linkwise::audio::playback::output_devices() {
                Ok(devices) => output(format_args!(
                    "{}",
                    serde_json::to_string_pretty(&devices).unwrap()
                )),
                Err(error) => {
                    eprintln!("Audio enumeration failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        #[cfg(feature = "audio-device")]
        (Some(arg), None, None) if arg == "audio-inputs" => {
            match linkwise::audio::device::input_devices() {
                Ok(devices) => output(format_args!(
                    "{}",
                    serde_json::to_string_pretty(&devices).unwrap()
                )),
                Err(error) => {
                    eprintln!("Audio enumeration failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        #[cfg(feature = "audio-device")]
        (Some(arg), Some(seconds), None) if arg == "receive-audio" => {
            let seconds = seconds.to_string_lossy();
            let result = tokio::select! {
                biased;
                event = linkwise::signals::stop() => Err(frame_cli::interruption(event, "audio receive interrupted")),
                result = receive_audio(&seconds) => result.map_err(frame_cli::Failure::operation),
            };
            if let Err(error) = result {
                fail(error.status, error.source);
            }
        }
        (Some(arg), Some(path), None) if arg == "analyze-wav" || arg == "decode-wav" => {
            match wav_analysis::analyze(Path::new(&path), arg == "decode-wav") {
                Ok(report) => output(format_args!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serialize WAV report")
                )),
                Err(error) => {
                    eprintln!("WAV analysis failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        (Some(arg), Some(input), Some(destination)) if arg == "render-wav" => {
            match wav_synthesis::render(Path::new(&input), Path::new(&destination)) {
                Ok(count) => output(format_args!(
                    "{}",
                    serde_json::json!({"sample_rate_hz":48000, "samples":count})
                )),
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

fn output(args: std::fmt::Arguments<'_>) {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    if let Err(error) = writeln!(stdout, "{args}").and_then(|_| stdout.flush()) {
        fail(1, error.into());
    }
}

fn fail(status: i32, error: Box<dyn std::error::Error>) -> ! {
    if !error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::BrokenPipe)
    {
        eprintln!("linkwise: {error}");
    }
    std::process::exit(status);
}

#[cfg(feature = "audio-device")]
async fn receive_audio(seconds: &str) -> Result<(), Box<dyn std::error::Error>> {
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
        if let Some(block) = capture.read(Duration::ZERO)? {
            receiver.push(&block.samples, &mut emit)?;
            tokio::task::yield_now().await;
        } else {
            tokio::time::sleep(Duration::from_millis(10)).await;
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
