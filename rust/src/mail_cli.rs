//! One B2F exchange through the local modem's paired host ports.

use std::{
    ffi::OsString,
    fs::File,
    io::{self, BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpStream as BlockingTcp},
    time::Duration,
};

use linkwise::winlink::{
    mailbox::DirectoryMailbox,
    session::{self, Role, Settings},
};
use tokio::{net::TcpStream, runtime::Handle, time::timeout};

use crate::{frame_cli::Failure, session_cli::Control};

const MESSAGE_LIMIT: usize = 1024 * 1024;
const COMPRESSED_LIMIT: usize = 2 * MESSAGE_LIMIT;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);
const IO_TIMEOUT: Duration = Duration::from_secs(120);
const CHUNK: usize = 1024;

pub(crate) async fn run(args: &[OsString]) -> Result<(), Failure> {
    if let [command, root, input] = args
        && (command == "queue" || command == "import")
    {
        let mut envelope = Vec::new();
        File::open(input)
            .and_then(|file| {
                file.take(if command == "import" {
                    2 * MESSAGE_LIMIT as u64 + 1
                } else {
                    MESSAGE_LIMIT as u64 + 1
                })
                .read_to_end(&mut envelope)
            })
            .map_err(Failure::operation)?;
        if command == "import" {
            envelope = linkwise::winlink::mime::import(&envelope, MESSAGE_LIMIT)
                .map_err(Failure::operation)?;
        }
        return DirectoryMailbox::open(root, MESSAGE_LIMIT)
            .and_then(|mailbox| mailbox.enqueue(&envelope))
            .map_err(Failure::operation);
    }
    if let [command, root, id] = args
        && command == "export"
    {
        let id = id
            .to_str()
            .ok_or_else(|| Failure::usage("invalid message ID"))?;
        let envelope = DirectoryMailbox::open(root, MESSAGE_LIMIT)
            .and_then(|mailbox| mailbox.incoming(id))
            .map_err(Failure::operation)?;
        let mail = linkwise::winlink::mime::export(&envelope, MESSAGE_LIMIT)
            .map_err(Failure::operation)?;
        return io::stdout()
            .lock()
            .write_all(&mail)
            .map_err(Failure::operation);
    }
    let call = |value: &OsString| {
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
    };
    let port = |value: &OsString| {
        value
            .to_str()
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|&port| port != 0)
            .ok_or_else(|| Failure::usage("invalid modem port"))
    };
    let transport = |options: &[OsString], default_port| {
        use linkwise::winlink::mail_tls::Transport;

        match options {
            [] => Ok((default_port, Transport::Plain)),
            [value] => Ok((port(value)?, Transport::Plain)),
            [value, flag, certificate, key] if flag == "--tls" => {
                let port = port(value)?;
                #[cfg(feature = "mail-tls")]
                {
                    let tls = Transport::from_pem(
                        certificate.as_os_str().as_ref(),
                        key.as_os_str().as_ref(),
                    )
                    .map_err(Failure::operation)?;
                    Ok((port, tls))
                }
                #[cfg(not(feature = "mail-tls"))]
                {
                    let _ = (port, certificate, key);
                    Err(Failure::usage(
                        "--tls requires a build with the mail-tls feature",
                    ))
                }
            }
            _ => Err(Failure::usage("expected [PORT [--tls CERT.pem KEY.pem]]")),
        }
    };
    if let [command, root, ports @ ..] = args
        && command == "imap"
    {
        let (port, tls) = transport(ports, 1143)?;
        let login = match (
            std::env::var_os("LINKWISE_IMAP_USER"),
            std::env::var_os("LINKWISE_IMAP_PASSWORD"),
        ) {
            (None, None) => None,
            (Some(user), Some(password)) => Some(
                linkwise::winlink::imap::Login::new(
                    user.into_string()
                        .map_err(|_| Failure::usage("invalid IMAP username"))?,
                    password
                        .into_string()
                        .map_err(|_| Failure::usage("invalid IMAP password"))?,
                )
                .map_err(Failure::operation)?,
            ),
            _ => {
                return Err(Failure::usage(
                    "set both LINKWISE_IMAP_USER and LINKWISE_IMAP_PASSWORD",
                ));
            }
        };
        return linkwise::winlink::imap::serve(root.into(), port, MESSAGE_LIMIT, tls, login)
            .await
            .map_err(Failure::operation);
    }
    if let [command, root, station, ports @ ..] = args
        && command == "smtp"
    {
        let (port, tls) = transport(ports, 2525)?;
        return linkwise::winlink::smtp::serve(
            root.into(),
            call(station)?,
            port,
            MESSAGE_LIMIT,
            tls,
        )
        .await
        .map_err(Failure::operation);
    }
    let [command, root, local, peer, ports @ ..] = args else {
        return Err(Failure::usage(
            "mail requires queue, import, export, smtp, imap, exchange, or listen",
        ));
    };
    if command != "exchange" && command != "listen" {
        return Err(Failure::usage(
            "mail requires queue, import, export, smtp, imap, exchange, or listen",
        ));
    }
    let caller = command == "exchange";
    let local = call(local)?;
    let peer = call(peer)?;
    let (command_port, data_port) = match ports {
        [] => (8300, 8301),
        [command, data] => (port(command)?, port(data)?),
        _ => return Err(Failure::usage("supply both modem ports")),
    };
    if command_port == data_port {
        return Err(Failure::usage("modem ports must differ"));
    }
    let mailbox = DirectoryMailbox::open(root, MESSAGE_LIMIT).map_err(Failure::operation)?;
    exchange(caller, local, peer, command_port, data_port, mailbox)
        .await
        .map_err(Failure::operation)
}

async fn exchange(
    caller: bool,
    local: String,
    peer: String,
    command_port: u16,
    data_port: u16,
    mut mailbox: DirectoryMailbox,
) -> io::Result<()> {
    let (control, data) = timeout(Duration::from_secs(5), async {
        tokio::try_join!(
            TcpStream::connect((Ipv4Addr::LOCALHOST, command_port)),
            TcpStream::connect((Ipv4Addr::LOCALHOST, data_port)),
        )
    })
    .await??;
    let mut control = Control::new(control);
    let (source, target) = if caller {
        (&local, &peer)
    } else {
        (&peer, &local)
    };
    timeout(CONNECT_TIMEOUT, async {
        control.command(&format!("MYCALL {local}")).await?;
        control.command("COMPRESSION OFF").await?;
        if caller {
            control.command("P2P SESSION").await?;
            control
                .command(&format!("CONNECT {source} {target}"))
                .await?;
        } else {
            control.command("LISTEN ON").await?;
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
    .await??;

    let data = data.into_std()?;
    data.set_nonblocking(false)?;
    data.set_read_timeout(Some(IO_TIMEOUT))?;
    data.set_write_timeout(Some(IO_TIMEOUT))?;
    let runtime = Handle::current();
    let (result, mut control, disconnected, _data) = tokio::task::spawn_blocking(move || {
        let mut stream = Data {
            socket: BufReader::new(data),
            pending: Vec::with_capacity(CHUNK),
            control,
            runtime,
            disconnected: false,
        };
        let role = if caller { Role::Caller } else { Role::Listener };
        let result = session::exchange(
            &mut stream,
            role,
            Settings {
                local_call: &local,
                peer_call: &peer,
                max_message_bytes: MESSAGE_LIMIT,
                max_compressed_bytes: COMPRESSED_LIMIT,
            },
            &mut mailbox,
        );
        (result, stream.control, stream.disconnected, stream.socket)
    })
    .await
    .map_err(io::Error::other)?;
    if !disconnected {
        let teardown = timeout(CONNECT_TIMEOUT, async {
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
            teardown??;
        }
    }
    result
}

struct Data {
    socket: BufReader<BlockingTcp>,
    pending: Vec<u8>,
    control: Control,
    runtime: Handle,
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
        self.socket.get_mut().write_all(&self.pending)?;
        let count = self.pending.len();
        let result = self
            .runtime
            .block_on(timeout_in_runtime(&mut self.control, count));
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

async fn timeout_in_runtime(control: &mut Control, count: usize) -> io::Result<()> {
    timeout(IO_TIMEOUT, control.acknowledged(count)).await?
}
