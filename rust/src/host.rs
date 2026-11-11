//! Host byte-stream transports. Session execution is supplied by a modem port;
//! this layer never manufactures CONNECTED, ACKs, or BUFFER advancement.

pub mod tcp;

use crate::api::{Command, CommandDecoder};
use async_trait::async_trait;
use std::{io, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};

#[derive(Clone, Copy)]
pub enum Channel {
    Control,
    Data,
}

pub enum Reply {
    Ok,
    Wrong,
    Version(String),
}

impl Reply {
    fn bytes(self) -> io::Result<Vec<u8>> {
        let line = match self {
            Self::Ok => "OK".into(),
            Self::Wrong => "WRONG".into(),
            Self::Version(version)
                if version.bytes().all(|b| b.is_ascii_graphic() || b == b' ') =>
            {
                format!("VERSION {version}")
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid host reply",
                ));
            }
        };
        Ok(format!("{line}\r").into_bytes())
    }
}

/// One engine-owned client session. The owner pairs its command/data sockets
/// and serializes access before passing them to these stream handlers.
#[async_trait]
pub trait ModemPort: Send + Sync {
    async fn command(&self, command: Command) -> Reply;
    /// Accept all bytes with backpressure, or fail; never silently drop a suffix.
    async fn data(&self, bytes: &[u8]) -> io::Result<()>;
    /// The owner must abort/clean up session and PTT authority on channel loss.
    async fn closed(&self, channel: Channel);
}

/// Works with TcpStream or any other duplex byte stream. Notifications must
/// already be validated ASCII CR-delimited engine events, at most 1024 bytes.
/// A closed notification queue does not fabricate a disconnected event.
pub async fn control<S>(
    stream: S,
    port: &dyn ModemPort,
    notices: mpsc::Receiver<Vec<u8>>,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let result = control_stream(stream, port, notices, None).await;
    port.closed(Channel::Control).await;
    result
}

async fn control_stream<S>(
    stream: S,
    port: &dyn ModemPort,
    mut notices: mpsc::Receiver<Vec<u8>>,
    write_timeout: Option<Duration>,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut read, mut write) = tokio::io::split(stream);
    let mut decoder = CommandDecoder::default();
    let mut input = [0; 1024];
    let mut notifications_open = true;
    loop {
        tokio::select! {
            count = read.read(&mut input) => {
                let count = count?;
                if count == 0 { return Ok(()); }
                let mut commands = Vec::new();
                decoder.push(&input[..count], |command| commands.push(command));
                for command in commands {
                    let reply = match command {
                        Ok(command) => port.command(command).await,
                        Err(_) => Reply::Wrong,
                    };
                    write_bytes(&mut write, &reply.bytes()?, write_timeout).await?;
                }
            }
            notice = notices.recv(), if notifications_open => {
                match notice {
                    None => notifications_open = false,
                    Some(notice) => {
                        if notice.len() > 1024 || notice.last() != Some(&b'\r')
                            || !notice[..notice.len()-1].iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
                            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid modem notification"));
                        }
                        write_bytes(&mut write, &notice, write_timeout).await?;
                    }
                }
            }
        }
    }
}

/// Raw binary transport, without line splitting, KISS wrapping or text matching.
/// The engine owns bounded receive queues and reports send progress separately.
pub async fn data<S>(
    stream: S,
    port: &dyn ModemPort,
    received: mpsc::Receiver<Vec<u8>>,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let result = data_stream(stream, port, received, None).await;
    port.closed(Channel::Data).await;
    result
}

async fn data_stream<S>(
    stream: S,
    port: &dyn ModemPort,
    mut received: mpsc::Receiver<Vec<u8>>,
    write_timeout: Option<Duration>,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut read, mut write) = tokio::io::split(stream);
    let mut input = [0; 4096];
    let mut receiving = true;
    loop {
        tokio::select! {
            // Drain queued output before accepting more input from a backend
            // that reserves bounded delivery space while handling each block.
            biased;
            bytes = received.recv(), if receiving => {
                match bytes {
                    Some(bytes) => write_bytes(&mut write, &bytes, write_timeout).await?,
                    None => receiving = false,
                }
            }
            count = read.read(&mut input) => {
                let count = count?;
                if count == 0 { return Ok(()); }
                port.data(&input[..count]).await?;
            }
        }
    }
}

async fn write_bytes(
    writer: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
    deadline: Option<Duration>,
) -> io::Result<()> {
    match deadline {
        Some(duration) => tokio::time::timeout(duration, writer.write_all(bytes))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "host write stalled"))?,
        None => writer.write_all(bytes).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[tokio::test(start_paused = true)]
    async fn bounded_writer_fails_when_the_peer_stops_reading() {
        let (mut writer, _peer) = tokio::io::duplex(1);
        let error = write_bytes(&mut writer, b"blocked", Some(Duration::from_secs(5)))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[derive(Default)]
    struct Port {
        bytes: Mutex<Vec<u8>>,
        closed: Mutex<usize>,
    }
    #[async_trait]
    impl ModemPort for Port {
        async fn command(&self, command: Command) -> Reply {
            match command {
                Command::Version => Reply::Version("Linkwise bench".into()),
                _ => Reply::Ok,
            }
        }
        async fn data(&self, bytes: &[u8]) -> io::Result<()> {
            self.bytes.lock().unwrap().extend(bytes);
            Ok(())
        }
        async fn closed(&self, _: Channel) {
            *self.closed.lock().unwrap() += 1;
        }
    }

    #[tokio::test]
    async fn command_transport_handles_fragmented_input_and_calls_cleanup_on_eof() {
        let port = Arc::new(Port::default());
        let (mut client, server) = tokio::io::duplex(256);
        let (_, notices) = mpsc::channel(4);
        let backend = port.clone();
        let task = tokio::spawn(async move { control(server, &*backend, notices).await });
        client.write_all(b"VER").await.unwrap();
        client
            .write_all(b"SION\rBW500\rNOTSUPPORTED\r")
            .await
            .unwrap();
        let expected = b"VERSION Linkwise bench\rOK\rWRONG\r";
        let mut response = vec![0; expected.len()];
        client.read_exact(&mut response).await.unwrap();
        assert_eq!(response, expected);
        drop(client);
        task.await.unwrap().unwrap();
        assert_eq!(*port.closed.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn data_transport_preserves_binary_bytes_in_both_directions() {
        let port = Arc::new(Port::default());
        let (mut client, server) = tokio::io::duplex(512);
        let (send, received) = mpsc::channel(4);
        let backend = port.clone();
        let task = tokio::spawn(async move { data(server, &*backend, received).await });
        let bytes: Vec<_> = (0..=255).collect();
        client.write_all(&bytes).await.unwrap();
        send.send(bytes.clone()).await.unwrap();
        let mut output = [0; 256];
        client.read_exact(&mut output).await.unwrap();
        assert_eq!(output.as_slice(), bytes);
        drop(client);
        task.await.unwrap().unwrap();
        assert_eq!(*port.bytes.lock().unwrap(), bytes);
        assert_eq!(*port.closed.lock().unwrap(), 1);
    }
}
