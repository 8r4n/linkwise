use std::{env, path::Path, sync::Arc};

mod wav_analysis;

use linkwise::{
    mcp,
    radio::{RadioControlPort, RigctldRadio, SimulatedRadio},
};

const USAGE: &str = "linkwise — radio-control and modem research service

Usage:
  linkwise                 Start the MCP server on stdio
  linkwise analyze-wav PATH  Report bench burst candidates as JSON
  linkwise decode-wav PATH   Decode BW500 level 1–3 DATA from a 48 kHz mono WAV
  linkwise --help
  linkwise --version

Environment:
  LINKWISE_RADIO_BACKEND=simulated|rigctld (default: simulated)
  LINKWISE_RIGCTLD_ADDR=host:port (default: hamlib:4532)
  LINKWISE_RADIO_WRITES=true (disabled by default; requires station limits)

WAV decoding is an offline research tool.";

#[tokio::main]
async fn main() {
    let mut args = env::args_os().skip(1);
    match (args.next(), args.next(), args.next()) {
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
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}
