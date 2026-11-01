use std::env;

const USAGE: &str = "openvara — cross-platform modem research scaffold

Usage:
  openvara --help
  openvara --version

Modem operation is not implemented yet.";

fn main() {
    let mut args = env::args_os().skip(1);
    match (args.next(), args.next()) {
        (None, None) => {
            println!("{USAGE}");
        }
        (Some(arg), None) if arg == "--help" || arg == "-h" => {
            println!("{USAGE}");
        }
        (Some(arg), None) if arg == "--version" || arg == "-V" => {
            println!("openvara {}", env!("CARGO_PKG_VERSION"));
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}
