//! Loopback SMTP submission for a single local Winlink station.

use std::{io, net::SocketAddr, path::PathBuf, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpListener,
    task::JoinSet,
    time::timeout,
};

use super::{
    Message,
    imap::Login,
    invalid,
    mail_tls::{MailStream, Transport},
    mailbox::DirectoryMailbox,
    mime,
};

const CLIENT_LIMIT: usize = 8;
const SESSION_TIMEOUT: Duration = Duration::from_secs(300);

/// Serve local submissions until the caller drops this future.
pub async fn serve(
    root: PathBuf,
    station: String,
    address: SocketAddr,
    limit: usize,
    tls: Transport,
    starttls: bool,
    login: Option<Login>,
) -> io::Result<()> {
    if (starttls || login.is_some()) && matches!(tls, Transport::Plain) {
        return Err(invalid(
            "SMTP authentication and STARTTLS require a TLS configuration",
        ));
    }
    if !address.ip().is_loopback()
        && (login.is_none() || matches!(tls, super::mail_tls::Transport::Plain))
    {
        return Err(invalid(
            "Non-loopback mail listeners require TLS and authentication",
        ));
    }
    let login = login.map(std::sync::Arc::new);
    DirectoryMailbox::open(&root, limit)?;
    let listener = TcpListener::bind(address).await?;
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            _ = clients.join_next(), if !clients.is_empty() => {}
            accepted = listener.accept(), if clients.len() < CLIENT_LIMIT => {
                let (socket, _) = accepted?;
                let root = root.clone();
                let station = station.clone();
                let tls = tls.clone();
                let login = login.clone();
                clients.spawn(async move {
                    let result = timeout(SESSION_TIMEOUT, async {
                        let encrypted = !starttls && !matches!(tls, Transport::Plain);
                        let socket = if starttls { Transport::Plain.accept(socket).await? } else { tls.accept(socket).await? };
                        session(socket, root, &station, limit, starttls.then_some(tls), encrypted, login.as_deref()).await
                    }).await;
                    if let Ok(Err(error)) = result {
                        eprintln!("Linkwise SMTP session ended: {error}");
                    }
                });
            }
        }
    }
}

async fn session(
    socket: Box<dyn MailStream>,
    root: PathBuf,
    station: &str,
    limit: usize,
    mut upgrade: Option<Transport>,
    mut encrypted: bool,
    login: Option<&Login>,
) -> io::Result<()> {
    let mut authenticated = login.is_none();
    let mut stream = BufReader::new(socket);
    let (mut greeted, mut sender, mut recipient) = (false, false, None);
    reply(&mut stream, "220 localhost Linkwise SMTP").await?;
    while let Some(line) = read_line(&mut stream, 512).await? {
        let command = std::str::from_utf8(&line[..line.len() - 2])
            .map_err(|_| invalid("invalid SMTP command"))?;
        let (verb, args) = command.split_once(' ').unwrap_or((command, ""));
        let (mechanism, initial) = args.split_once(' ').unwrap_or((args, ""));
        let response = match verb.to_ascii_uppercase().as_str() {
            "QUIT" => return reply(&mut stream, "221 Closing connection").await,
            "EHLO" | "HELO" if !args.is_empty() => {
                greeted = true;
                sender = false;
                recipient = None;
                match (
                    verb.eq_ignore_ascii_case("EHLO"),
                    upgrade.is_some(),
                    encrypted && login.is_some(),
                ) {
                    (true, true, _) => "250-localhost\r\n250 STARTTLS",
                    (true, false, true) => "250-localhost\r\n250 AUTH PLAIN LOGIN",
                    _ => "250 localhost",
                }
            }
            "NOOP" => "250 OK",
            "RSET" => {
                sender = false;
                recipient = None;
                "250 OK"
            }
            "STARTTLS" if greeted && args.is_empty() && upgrade.is_some() => {
                if !stream.buffer().is_empty() {
                    return reply(&mut stream, "554 STARTTLS cannot be pipelined").await;
                }
                reply(&mut stream, "220 Ready to start TLS").await?;
                stream = BufReader::new(
                    upgrade
                        .take()
                        .expect("upgrade checked")
                        .accept(stream.into_inner())
                        .await?,
                );
                encrypted = true;
                greeted = false;
                sender = false;
                recipient = None;
                continue;
            }
            "STARTTLS" => "503 STARTTLS unavailable in this session state",
            "AUTH" if encrypted && !authenticated && mechanism.eq_ignore_ascii_case("LOGIN") => {
                // The pinned Thunderbird client starts AUTH immediately after STARTTLS.
                authenticated =
                    authenticate(&mut stream, login.expect("login required"), initial).await?;
                if authenticated {
                    greeted = true;
                    "235 2.7.0 Authentication successful"
                } else {
                    "535 5.7.8 Invalid credentials"
                }
            }
            "AUTH" if encrypted && !authenticated && mechanism.eq_ignore_ascii_case("PLAIN") => {
                authenticated =
                    authenticate_plain(&mut stream, login.expect("login required"), initial)
                        .await?;
                if authenticated {
                    greeted = true;
                    "235 2.7.0 Authentication successful"
                } else {
                    "535 5.7.8 Invalid credentials"
                }
            }
            "AUTH" if !encrypted => "538 5.7.11 Encryption required",
            "AUTH" if login.is_some() && authenticated => "503 Already authenticated",
            "AUTH" => "504 Unsupported authentication mechanism",
            "MAIL" if !authenticated || upgrade.is_some() => {
                "530 5.7.0 Authentication or STARTTLS required"
            }
            "MAIL" if greeted => {
                sender = false;
                recipient = None;
                if path_call(args, "FROM:").is_some_and(|call| call.eq_ignore_ascii_case(station)) {
                    sender = true;
                    "250 Sender accepted"
                } else {
                    "553 Sender must match the local Winlink station"
                }
            }
            "RCPT" if sender && recipient.is_none() => {
                recipient = path_call(args, "TO:").map(str::to_ascii_uppercase);
                if recipient.is_some() {
                    "250 Recipient accepted"
                } else {
                    "550 Use callsign@winlink.org for P2P delivery"
                }
            }
            "RCPT" if recipient.is_some() => "452 Only one P2P recipient is supported",
            "DATA" if args.is_empty() && recipient.is_some() => {
                let target = recipient.take().expect("recipient checked");
                sender = false;
                reply(&mut stream, "354 End data with <CRLF>.<CRLF>").await?;
                let Some(raw) = receive_data(&mut stream, limit.saturating_mul(2)).await? else {
                    return reply(&mut stream, "552 Message exceeds size limit").await;
                };
                let root = root.clone();
                let station = station.to_owned();
                match tokio::task::spawn_blocking(move || {
                    queue(root, &station, &target, &raw, limit)
                })
                .await
                {
                    Ok(Ok(())) => "250 Queued for Winlink P2P",
                    Ok(Err(error)) if error.kind() == io::ErrorKind::InvalidData => {
                        "554 Unsupported message or SMTP envelope differs from message headers"
                    }
                    _ => "451 Could not store message",
                }
            }
            "MAIL" | "RCPT" | "DATA" => "503 Bad sequence of commands",
            _ => "502 Command not implemented",
        };
        reply(&mut stream, response).await?;
    }
    Ok(())
}

async fn authenticate(
    stream: &mut BufReader<Box<dyn MailStream>>,
    login: &Login,
    initial: &str,
) -> io::Result<bool> {
    let username = if initial.is_empty() {
        reply(stream, "334 VXNlcm5hbWU6").await?;
        let Some(line) = read_line(stream, 512).await? else {
            return Ok(false);
        };
        line[..line.len() - 2].to_vec()
    } else {
        initial.as_bytes().to_vec()
    };
    if username == b"*" {
        return Ok(false);
    }
    reply(stream, "334 UGFzc3dvcmQ6").await?;
    let Some(password) = read_line(stream, 512).await? else {
        return Ok(false);
    };
    let decode = |encoded: &[u8]| {
        STANDARD
            .decode(encoded)
            .ok()
            .and_then(|value| String::from_utf8(value).ok())
    };
    Ok(
        match (decode(&username), decode(&password[..password.len() - 2])) {
            (Some(username), Some(password)) => login.matches(&username, &password),
            _ => false,
        },
    )
}

async fn authenticate_plain(
    stream: &mut BufReader<Box<dyn MailStream>>,
    login: &Login,
    initial: &str,
) -> io::Result<bool> {
    let response = if initial.is_empty() {
        reply(stream, "334 ").await?;
        let Some(line) = read_line(stream, 512).await? else {
            return Ok(false);
        };
        line[..line.len() - 2].to_vec()
    } else {
        initial.as_bytes().to_vec()
    };
    let Ok(decoded) = STANDARD.decode(response) else {
        return Ok(false);
    };
    let Ok(credentials) = std::str::from_utf8(&decoded) else {
        return Ok(false);
    };
    let mut fields = credentials.split('\0');
    let (Some(identity), Some(username), Some(password), None) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return Ok(false);
    };
    Ok((identity.is_empty() || identity == username) && login.matches(username, password))
}

fn queue(root: PathBuf, station: &str, target: &str, raw: &[u8], limit: usize) -> io::Result<()> {
    let wire = mime::import(raw, limit)?;
    let message = Message::decode(&wire, limit)?;
    if !message.from.eq_ignore_ascii_case(station) || message.to != [target] {
        return Err(invalid("SMTP envelope differs from MIME headers"));
    }
    DirectoryMailbox::open(root, limit)?.enqueue(&wire)
}

fn path_call<'a>(args: &'a str, prefix: &str) -> Option<&'a str> {
    if !args.get(..prefix.len())?.eq_ignore_ascii_case(prefix) {
        return None;
    }
    let address = args[prefix.len()..]
        .trim()
        .strip_prefix('<')?
        .strip_suffix('>')?;
    let (call, domain) = address.split_once('@')?;
    (domain.eq_ignore_ascii_case("winlink.org") && mime::valid_call(call)).then_some(call)
}

async fn receive_data(
    stream: &mut BufReader<impl AsyncRead + AsyncWrite + Unpin>,
    limit: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut raw = Vec::new();
    while let Some(line) = read_line(stream, 1001).await? {
        if line == b".\r\n" {
            return Ok(Some(raw));
        }
        let unstuffed = line.strip_prefix(b".").unwrap_or(&line);
        if unstuffed.len() > limit.saturating_sub(raw.len()) {
            return Ok(None);
        }
        raw.extend_from_slice(unstuffed);
    }
    Err(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "SMTP DATA interrupted",
    ))
}

async fn read_line(
    stream: &mut BufReader<impl AsyncRead + AsyncWrite + Unpin>,
    limit: u64,
) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    stream.take(limit).read_until(b'\n', &mut line).await?;
    if line.is_empty() {
        return Ok(None);
    }
    if !line.ends_with(b"\r\n") {
        return Err(invalid("invalid or oversized SMTP line"));
    }
    Ok(Some(line))
}

async fn reply(
    stream: &mut BufReader<impl AsyncRead + AsyncWrite + Unpin>,
    text: &str,
) -> io::Result<()> {
    stream.write_all(text.as_bytes()).await?;
    stream.write_all(b"\r\n").await
}
