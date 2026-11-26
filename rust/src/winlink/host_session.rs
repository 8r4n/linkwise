//! Peer B2F forwarding over a modem's paired command and data streams.

use std::{
    future::Future,
    io::{self, BufRead, BufReader, Read, Write},
    time::Duration,
};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader as AsyncBufReader},
    net::{
        TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    runtime::Handle,
    sync::watch,
    time::timeout,
};

use super::{
    mailbox::DirectoryMailbox,
    session::{self, Role, Settings},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);
const IO_TIMEOUT: Duration = Duration::from_secs(120);
const CHUNK: usize = 1024;

/// Already paired host sockets. The caller owns endpoint selection and any authentication.
pub struct Connection {
    pub control: TcpStream,
    pub data: TcpStream,
}

/// Host attachment retained after the modem reports disconnect. An isolated
/// two-ended supervisor can release it after both clients finish teardown.
pub struct CompletedAttachment {
    _control: Control,
    _data: BufReader<DataSocket>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Progress {
    /// The listening modem accepted LISTEN ON.
    Listening,
    /// The modem reported a connection with the expected source and peer.
    ModemConnected,
    /// B2F exchange and modem teardown succeeded.
    Completed,
}

/// Run the host side of one B2F exchange. Set `stop` to true or close its sender
/// and await this future to join the mailbox worker before deleting its directory.
/// Dropping the future signals its worker but does not join it.
pub async fn exchange(
    connection: Connection,
    role: Role,
    settings: Settings<'_>,
    mailbox: DirectoryMailbox,
    stop: watch::Receiver<bool>,
    progress: impl FnMut(Progress),
) -> io::Result<()> {
    exchange_attached(connection, role, settings, mailbox, stop, progress)
        .await
        .map(drop)
}

/// Run an exchange and yield the paired host sockets after a clean teardown.
/// The cancellation and worker-join contract is the same as [`exchange`].
pub async fn exchange_attached(
    connection: Connection,
    role: Role,
    settings: Settings<'_>,
    mut mailbox: DirectoryMailbox,
    mut stop: watch::Receiver<bool>,
    mut progress: impl FnMut(Progress),
) -> io::Result<CompletedAttachment> {
    for call in [settings.local_call, settings.peer_call] {
        if call.is_empty()
            || call.len() > 12
            || !call
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid modem callsign",
            ));
        }
    }
    let mut control = Control::new(connection.control);
    let (local, peer) = (settings.local_call, settings.peer_call);
    let caller = matches!(role, Role::Caller);
    let (source, target) = if caller { (local, peer) } else { (peer, local) };
    bounded(&mut stop, CONNECT_TIMEOUT, async {
        control.command(&format!("MYCALL {local}")).await?;
        control.command("COMPRESSION OFF").await?;
        if caller {
            control.command("P2P SESSION").await?;
            control
                .command(&format!("CONNECT {source} {target}"))
                .await?;
        } else {
            control.command("LISTEN ON").await?;
            progress(Progress::Listening);
        }
        loop {
            let line = control.line().await?;
            let mut words = line.split_ascii_whitespace();
            match words.next() {
                Some("CONNECTED") => {
                    return if words.next() == Some(source) && words.next() == Some(target) {
                        Ok(())
                    } else {
                        Err(io::Error::other("unexpected modem peer"))
                    };
                }
                Some("DISCONNECTED") => {
                    return Err(io::Error::other("modem disconnected before B2F"));
                }
                _ => {}
            }
        }
    })
    .await?;
    progress(Progress::ModemConnected);

    let (local, peer) = (local.to_owned(), peer.to_owned());
    let (message_limit, compressed_limit) =
        (settings.max_message_bytes, settings.max_compressed_bytes);
    let runtime = Handle::current();
    let (cancel_worker, worker_stop) = watch::channel(false);
    let mut worker = tokio::task::spawn_blocking(move || {
        let mut stream = Data {
            socket: BufReader::new(DataSocket {
                data: connection.data,
                runtime,
                stop: worker_stop,
            }),
            pending: Vec::with_capacity(CHUNK),
            control,
            disconnected: false,
        };
        let result = session::exchange(
            &mut stream,
            role,
            Settings {
                local_call: &local,
                peer_call: &peer,
                max_message_bytes: message_limit,
                max_compressed_bytes: compressed_limit,
            },
            &mut mailbox,
        );
        (result, stream.control, stream.disconnected, stream.socket)
    });
    let (result, mut control, disconnected, data) = tokio::select! {
        biased;
        _ = stopped(&mut stop) => {
            cancel_worker.send_replace(true);
            drop(worker.await.map_err(io::Error::other)?);
            return Err(canceled());
        }
        done = &mut worker => done.map_err(io::Error::other)?,
    };
    if !disconnected {
        let teardown = bounded(&mut stop, CONNECT_TIMEOUT, async {
            if caller || result.is_err() {
                control.request("DISCONNECT").await?;
            }
            loop {
                match control.line().await?.as_str() {
                    "DISCONNECTED" => return Ok(()),
                    "WRONG" => return Err(io::Error::other("modem rejected disconnect")),
                    _ => {}
                }
            }
        })
        .await;
        if result.is_ok() {
            teardown?;
        }
    }
    result?;
    progress(Progress::Completed);
    Ok(CompletedAttachment {
        _control: control,
        _data: data,
    })
}

async fn stopped(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|&flag| flag).await;
}

fn canceled() -> io::Error {
    // Synchronous Read and Write retry Interrupted, so cancellation must be terminal.
    io::Error::new(
        io::ErrorKind::ConnectionAborted,
        "modem host session canceled",
    )
}

async fn bounded<T>(
    stop: &mut watch::Receiver<bool>,
    limit: Duration,
    run: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    tokio::select! {
        biased;
        _ = stopped(stop) => Err(canceled()),
        result = timeout(limit, run) => result?,
    }
}

struct DataSocket {
    data: TcpStream,
    runtime: Handle,
    stop: watch::Receiver<bool>,
}

impl Read for DataSocket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.runtime
            .block_on(bounded(&mut self.stop, IO_TIMEOUT, self.data.read(buf)))
    }
}

struct Data {
    socket: BufReader<DataSocket>,
    pending: Vec<u8>,
    control: Control,
    disconnected: bool,
}

impl Read for Data {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.socket.read(buf)
    }
}

impl BufRead for Data {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.socket.fill_buf()
    }

    fn consume(&mut self, count: usize) {
        self.socket.consume(count);
    }
}

impl Write for Data {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.pending.len() == CHUNK {
            self.flush()?;
        }
        let count = bytes.len().min(CHUNK - self.pending.len());
        self.pending.extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let socket = self.socket.get_mut();
        socket.runtime.block_on(bounded(
            &mut socket.stop,
            IO_TIMEOUT,
            socket.data.write_all(&self.pending),
        ))?;
        let result = socket.runtime.block_on(bounded(
            &mut socket.stop,
            IO_TIMEOUT,
            self.control.acknowledged(self.pending.len()),
        ));
        if result
            .as_ref()
            .is_err_and(|error| error.kind() == io::ErrorKind::ConnectionAborted)
        {
            self.disconnected = true;
        }
        result?;
        self.pending.clear();
        Ok(())
    }
}

/// Modem command channel reader and byte-acknowledgment tracker.
pub struct Control {
    read: AsyncBufReader<OwnedReadHalf>,
    write: OwnedWriteHalf,
    prefix: Vec<u8>,
}

impl Control {
    pub fn new(stream: TcpStream) -> Self {
        let (read, write) = stream.into_split();
        Self {
            read: AsyncBufReader::new(read),
            write,
            prefix: Vec::new(),
        }
    }

    pub async fn line(&mut self) -> io::Result<String> {
        loop {
            let bytes = self.read.fill_buf().await?;
            if bytes.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "modem command port closed",
                ));
            }
            let end = bytes.iter().position(|&byte| byte == b'\r');
            let count = end.unwrap_or(bytes.len());
            if self.prefix.len() + count > 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "oversized modem notice",
                ));
            }
            self.prefix.extend_from_slice(&bytes[..count]);
            self.read.consume(count + usize::from(end.is_some()));
            if end.is_some() {
                return String::from_utf8(std::mem::take(&mut self.prefix))
                    .map_err(io::Error::other);
            }
        }
    }

    pub async fn request(&mut self, command: &str) -> io::Result<()> {
        self.write
            .write_all(format!("{command}\r").as_bytes())
            .await
    }

    pub async fn command(&mut self, command: &str) -> io::Result<()> {
        self.request(command).await?;
        loop {
            match self.line().await?.as_str() {
                "OK" => return Ok(()),
                "WRONG" => return Err(io::Error::other(format!("modem rejected {command}"))),
                "IAMALIVE" => continue,
                other => {
                    return Err(io::Error::other(format!(
                        "unexpected modem response: {other}"
                    )));
                }
            }
        }
    }

    pub async fn acknowledged(&mut self, size: usize) -> io::Result<()> {
        let mut pending = 0usize;
        let mut acknowledged = 0usize;
        loop {
            let line = self.line().await?;
            if line == "DISCONNECTED" {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "modem disconnected with unconfirmed bytes",
                ));
            }
            if let Some(value) = line.strip_prefix("BUFFER ") {
                let current: usize = value.parse().map_err(io::Error::other)?;
                acknowledged += pending.saturating_sub(current);
                pending = current;
                if pending == 0 && acknowledged >= size {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn sockets() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, peer) = tokio::join!(
            TcpStream::connect(listener.local_addr().unwrap()),
            listener.accept(),
        );
        (client.unwrap(), peer.unwrap().0)
    }

    #[tokio::test]
    async fn cancellation_joins_mailbox_worker_while_modem_acknowledgment_is_stalled() {
        let (control, peer_control) = sockets().await;
        let (data, mut peer_data) = sockets().await;
        let dir = tempfile::tempdir().unwrap();
        let mailbox = DirectoryMailbox::open(dir.path(), 4096).unwrap();
        let (cancel, stop) = watch::channel(false);
        let mut events = Vec::new();
        let exchange = exchange(
            Connection { control, data },
            Role::Listener,
            Settings {
                local_call: "W1AW",
                peer_call: "K1ABC",
                max_message_bytes: 4096,
                max_compressed_bytes: 8192,
            },
            mailbox,
            stop,
            |status| events.push(status),
        );
        let peer = async {
            let mut control = AsyncBufReader::new(peer_control);
            for command in ["MYCALL W1AW\r", "COMPRESSION OFF\r", "LISTEN ON\r"] {
                let mut request = Vec::new();
                control.read_until(b'\r', &mut request).await.unwrap();
                assert_eq!(request, command.as_bytes());
                control
                    .get_mut()
                    .write_all(b"IAMALIVE\rOK\r")
                    .await
                    .unwrap();
            }
            control
                .get_mut()
                .write_all(b"CONNECTED K1ABC W1AW 500\r")
                .await
                .unwrap();
            let mut byte = [0];
            // The forwarding worker sent its greeting and is waiting for BUFFER evidence.
            assert_eq!(peer_data.read(&mut byte).await.unwrap(), 1);
            cancel.send_replace(true);
            assert_eq!(control.read(&mut byte).await.unwrap(), 0);
            peer_data.read_to_end(&mut Vec::new()).await.unwrap();
        };
        let (result, ()) = timeout(Duration::from_secs(5), async {
            tokio::join!(exchange, peer)
        })
        .await
        .expect("canceling a stalled exchange should close both sockets");
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::ConnectionAborted);
        assert_eq!(events, [Progress::Listening, Progress::ModemConnected]);
        assert!(
            std::fs::read_dir(dir.path().join("inbox"))
                .unwrap()
                .next()
                .is_none()
        );
    }
}
