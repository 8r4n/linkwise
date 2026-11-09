use std::net::{Ipv4Addr, SocketAddr};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let port = match (args.next(), args.next(), args.next()) {
        (None, None, None) => 8787,
        (Some(flag), None, None) if flag == "--help" || flag == "-h" => {
            println!(
                "Usage: modem-demo [--port PORT]\nLocal Rust modem/SigPlot demo. Default: http://127.0.0.1:8787"
            );
            return;
        }
        (Some(flag), Some(port), None) if flag == "--port" => match port.parse::<u16>() {
            Ok(port) => port,
            Err(_) => {
                eprintln!("invalid port");
                std::process::exit(2);
            }
        },
        _ => {
            eprintln!("Usage: modem-demo [--port PORT]");
            std::process::exit(2);
        }
    };
    if let Err(error) = linkwise::demo::serve(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await {
        eprintln!("Modem demo failed: {error}");
        std::process::exit(1);
    }
}
