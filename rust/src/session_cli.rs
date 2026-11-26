//! Send bounded chunks through the local VARA-compatible modem host.
use std::{
    error::Error,
    ffi::OsString,
    fs::File,
    io::{self, Read},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
    time::timeout,
};

use crate::frame_cli::Failure;
pub(crate) use linkwise::winlink::host_session::Control;

const CHUNK: usize = 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) async fn run(args: &[OsString]) -> Result<(), Failure> {
    let ([source, destination, input, ..], ports) = (args, args.get(3..).unwrap_or_default())
    else {
        return Err(Failure::usage(
            "send requires SOURCE DESTINATION INPUT [COMMAND_PORT DATA_PORT]",
        ));
    };
    let source = callsign(source)?;
    let destination = callsign(destination)?;
    let (command_port, data_port) = parse_ports(ports)?;
    let (sender, mut receiver) = mpsc::channel(1);
    let path = input.clone();
    // Stdin may block indefinitely. Keep socket and signal processing on the
    // async runtime and stop the producer when its receiver goes away.
    std::thread::spawn(move || {
        if let Err(error) = read_input(&path, &sender) {
            let _ = sender.blocking_send(Err(error));
        }
    });
    tokio::select! {
        biased;
        event = linkwise::signals::stop() => Err(crate::frame_cli::interruption(
            event, "transmission interrupted; delivery may be unconfirmed",
        )),
        result = send(source, destination, command_port, data_port, &mut receiver) => {
            result.map_err(Failure::operation)
        }
    }
}

pub(crate) fn callsign(value: &std::ffi::OsStr) -> Result<String, Failure> {
    value
        .to_str()
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 12
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        .map(str::to_ascii_uppercase)
        .ok_or_else(|| Failure::usage("invalid callsign"))
}

pub(crate) fn parse_ports(ports: &[OsString]) -> Result<(u16, u16), Failure> {
    let port = |value: &OsString| {
        value
            .to_str()
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|&value| value != 0)
            .ok_or_else(|| Failure::usage("ports must be integers from 1 to 65535"))
    };
    let ports = match ports {
        [] => (8300, 8301),
        [command, data] => (port(command)?, port(data)?),
        _ => return Err(Failure::usage("supply both command and data ports")),
    };
    if ports.0 == ports.1 {
        return Err(Failure::usage("command and data ports must differ"));
    }
    Ok(ports)
}

fn read_input(path: &OsString, output: &mpsc::Sender<io::Result<Vec<u8>>>) -> io::Result<()> {
    let mut input: Box<dyn Read> = if path == "-" {
        Box::new(io::stdin().lock())
    } else {
        Box::new(File::open(path)?)
    };
    loop {
        let mut chunk = Vec::with_capacity(CHUNK);
        input.by_ref().take(CHUNK as u64).read_to_end(&mut chunk)?;
        if chunk.is_empty() || output.blocking_send(Ok(chunk)).is_err() {
            return Ok(());
        }
    }
}

async fn send(
    source: String,
    destination: String,
    command_port: u16,
    data_port: u16,
    input: &mut mpsc::Receiver<io::Result<Vec<u8>>>,
) -> Result<(), Box<dyn Error>> {
    let Some(first) = input.recv().await.transpose()? else {
        return Ok(());
    };
    let (control, data) = timeout(Duration::from_secs(5), async {
        tokio::try_join!(
            TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, command_port)),
            TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, data_port)),
        )
    })
    .await??;
    let mut control = Control::new(control);
    let (mut inbound, mut outbound) = data.into_split();
    let transfer = async {
        timeout(CONNECT_TIMEOUT, async {
            control.command(&format!("MYCALL {source}")).await?;
            control.command("P2P SESSION").await?;
            control
                .command(&format!("CONNECT {source} {destination}"))
                .await?;
            loop {
                let line = control.line().await?;
                if line.starts_with(&format!("CONNECTED {source} {destination} ")) {
                    return Ok(());
                }
                if line == "DISCONNECTED" {
                    return Err(io::Error::other("modem disconnected before connecting"));
                }
            }
        })
        .await??;
        let mut next = Some(first);
        while let Some(chunk) = next {
            timeout(TRANSFER_TIMEOUT, async {
                outbound.write_all(&chunk).await?;
                control.acknowledged(chunk.len()).await
            })
            .await??;
            next = loop {
                tokio::select! {
                    biased;
                    line = control.line() => {
                        if line? == "DISCONNECTED" {
                            return Err("modem disconnected before input ended".into());
                        }
                    }
                    chunk = input.recv() => break chunk.transpose()?,
                }
            };
        }
        timeout(CONNECT_TIMEOUT, async {
            control.command("DISCONNECT").await?;
            while control.line().await? != "DISCONNECTED" {}
            Ok::<_, io::Error>(())
        })
        .await??;
        Ok(())
    };
    let discard_inbound = async {
        let mut bytes = [0; CHUNK];
        while inbound.read(&mut bytes).await? != 0 {}
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "modem data port closed",
        ))
    };
    tokio::select! {
        biased;
        result = transfer => result,
        result = discard_inbound => result.map_err(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn send_times_out_when_acknowledgment_stalls() {
        let command_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let data_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (sender, mut input) = mpsc::channel(1);
        sender.send(Ok(vec![0, 255, 1])).await.unwrap();
        drop(sender);

        let transfer = send(
            "W1AW".into(),
            "K1ABC".into(),
            command_listener.local_addr().unwrap().port(),
            data_listener.local_addr().unwrap().port(),
            &mut input,
        );
        tokio::pin!(transfer);

        let peer = async {
            let (mut command, _) = command_listener.accept().await.unwrap();
            let (mut data, _) = data_listener.accept().await.unwrap();
            for request in ["MYCALL W1AW\r", "P2P SESSION\r", "CONNECT W1AW K1ABC\r"] {
                let mut bytes = vec![0; request.len()];
                command.read_exact(&mut bytes).await.unwrap();
                assert_eq!(bytes, request.as_bytes());
                command.write_all(b"OK\r").await.unwrap();
            }
            command
                .write_all(b"CONNECTED W1AW K1ABC 500\r")
                .await
                .unwrap();
            let mut bytes = [0; 3];
            data.read_exact(&mut bytes).await.unwrap();
            assert_eq!(bytes, [0, 255, 1]);
            command.write_all(b"BUFFER 3\r").await.unwrap();
            (command, data)
        };

        let (mut command, mut data) = timeout(Duration::from_secs(5), async {
            tokio::select! {
                result = &mut transfer => panic!("send completed before acknowledgment: {result:?}"),
                sockets = peer => sockets,
            }
        })
        .await
        .expect("test peer must receive the first chunk");

        // Both sockets stay open, so only the production deadline can end the send.
        tokio::time::pause();
        tokio::time::advance(TRANSFER_TIMEOUT).await;
        let error = transfer.await.unwrap_err();
        assert!(
            error
                .downcast_ref::<tokio::time::error::Elapsed>()
                .is_some(),
            "{error}"
        );
        let mut byte = [0];
        assert_eq!(command.read(&mut byte).await.unwrap(), 0);
        assert_eq!(data.read(&mut byte).await.unwrap(), 0);
    }
}
