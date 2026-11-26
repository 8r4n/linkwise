//! One B2F exchange through the local modem's paired host ports.

use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use linkwise::winlink::{
    host_session::{self, Connection},
    mailbox::DirectoryMailbox,
    session::{Role, Settings},
};
use tokio::{net::TcpStream, sync::watch, time::timeout};

use crate::frame_cli::Failure;

const MESSAGE_LIMIT: usize = 1024 * 1024;
const COMPRESSED_LIMIT: usize = 2 * MESSAGE_LIMIT;

pub(crate) async fn run(args: &[OsString]) -> Result<(), Failure> {
    if args.get(1).is_some_and(|arg| arg == "--pki") {
        #[cfg(feature = "secure-transfer")]
        return crate::pki_mail::run(args);
        #[cfg(not(feature = "secure-transfer"))]
        return Err(Failure::usage("PKI support is unavailable in this build"));
    }
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
            [value, flag, certificate, key] if flag == "--tls" || flag == "--starttls" => {
                let port = port(value)?;
                #[cfg(any(
                    feature = "mail-tls",
                    all(target_os = "linux", feature = "rocky-mail-tls")
                ))]
                {
                    let tls = Transport::from_pem(
                        certificate.as_os_str().as_ref(),
                        key.as_os_str().as_ref(),
                    )
                    .map_err(Failure::operation)?;
                    Ok((port, tls))
                }
                #[cfg(not(any(
                    feature = "mail-tls",
                    all(target_os = "linux", feature = "rocky-mail-tls")
                )))]
                {
                    let _ = (port, certificate, key);
                    Err(Failure::usage(
                        "TLS requires a build with mail-tls or rocky-mail-tls",
                    ))
                }
            }
            _ => Err(Failure::usage(
                "expected [PORT [--tls|--starttls CERT.pem KEY.pem]]",
            )),
        }
    };
    let mail_address = |variable, port| {
        let address = match std::env::var(variable) {
            Ok(value) => value.parse::<IpAddr>().map_err(|_| {
                Failure::usage("LINKWISE_IMAP_BIND and LINKWISE_SMTP_BIND must be IP addresses")
            })?,
            Err(std::env::VarError::NotPresent) => Ipv4Addr::LOCALHOST.into(),
            Err(_) => return Err(Failure::usage("invalid mail bind address")),
        };
        Ok(SocketAddr::new(address, port))
    };
    if let [command, root, ports @ ..] = args
        && command == "imap"
    {
        if ports.get(1).is_some_and(|flag| flag == "--starttls") {
            return Err(Failure::usage("IMAP supports implicit TLS with --tls"));
        }
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
        return linkwise::winlink::imap::serve(
            root.into(),
            mail_address("LINKWISE_IMAP_BIND", port)?,
            MESSAGE_LIMIT,
            tls,
            login,
        )
        .await
        .map_err(Failure::operation);
    }
    if let [command, root, station, ports @ ..] = args
        && command == "smtp"
    {
        let (port, tls) = transport(ports, 2525)?;
        let starttls = ports.get(1).is_some_and(|flag| flag == "--starttls");
        let login = match (
            std::env::var_os("LINKWISE_SMTP_USER"),
            std::env::var_os("LINKWISE_SMTP_PASSWORD"),
        ) {
            (None, None) => None,
            (Some(user), Some(password)) => Some(
                linkwise::winlink::imap::Login::new(
                    user.into_string()
                        .map_err(|_| Failure::usage("invalid SMTP username"))?,
                    password
                        .into_string()
                        .map_err(|_| Failure::usage("invalid SMTP password"))?,
                )
                .map_err(Failure::operation)?,
            ),
            _ => {
                return Err(Failure::usage(
                    "set both LINKWISE_SMTP_USER and LINKWISE_SMTP_PASSWORD",
                ));
            }
        };
        return linkwise::winlink::smtp::serve(
            root.into(),
            call(station)?,
            mail_address("LINKWISE_SMTP_BIND", port)?,
            MESSAGE_LIMIT,
            tls,
            starttls,
            login,
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
    mailbox: DirectoryMailbox,
) -> io::Result<()> {
    let (control, data) = timeout(Duration::from_secs(5), async {
        tokio::try_join!(
            TcpStream::connect((Ipv4Addr::LOCALHOST, command_port)),
            TcpStream::connect((Ipv4Addr::LOCALHOST, data_port)),
        )
    })
    .await??;
    let (_continue_running, stop) = watch::channel(false);
    host_session::exchange(
        Connection { control, data },
        if caller { Role::Caller } else { Role::Listener },
        Settings {
            local_call: &local,
            peer_call: &peer,
            max_message_bytes: MESSAGE_LIMIT,
            max_compressed_bytes: COMPRESSED_LIMIT,
        },
        mailbox,
        stop,
        |_| {},
    )
    .await
}
