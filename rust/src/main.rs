use std::env;

const USAGE: &str = "linkwise — cross-platform modem research scaffold

Usage:
  linkwise --help
  linkwise --version

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
            println!("linkwise {}", env!("CARGO_PKG_VERSION"));
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}
