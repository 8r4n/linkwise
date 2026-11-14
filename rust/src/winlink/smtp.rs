//! Loopback SMTP submission for a single local Winlink station.

use std::{io, net::Ipv4Addr, path::PathBuf, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpListener,
    task::JoinSet,
    time::timeout,
};

use super::{Message, invalid, mailbox::DirectoryMailbox, mime};

const CLIENT_LIMIT: usize = 8;
const SESSION_TIMEOUT: Duration = Duration::from_secs(300);

/// Serve local submissions until the caller drops this future.
pub async fn serve(
    root: PathBuf,
    station: String,
    port: u16,
    limit: usize,
    tls: super::mail_tls::Transport,
) -> io::Result<()> {
    DirectoryMailbox::open(&root, limit)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            _ = clients.join_next(), if !clients.is_empty() => {}
            accepted = listener.accept(), if clients.len() < CLIENT_LIMIT => {
                let (socket, _) = accepted?;
                let root = root.clone();
                let station = station.clone();
                let tls = tls.clone();
                clients.spawn(async move {
                    let _ = timeout(SESSION_TIMEOUT, async {
                        session(tls.accept(socket).await?, root, &station, limit).await
                    }).await;
                });
            }
        }
    }
}

async fn session(
    socket: impl AsyncRead + AsyncWrite + Unpin,
    root: PathBuf,
    station: &str,
    limit: usize,
) -> io::Result<()> {
    let mut stream = BufReader::new(socket);
    let (mut greeted, mut sender, mut recipient) = (false, false, None);
    reply(&mut stream, "220 localhost Linkwise SMTP").await?;
    while let Some(line) = read_line(&mut stream, 512).await? {
        let command = std::str::from_utf8(&line[..line.len() - 2])
            .map_err(|_| invalid("invalid SMTP command"))?;
        let (verb, args) = command.split_once(' ').unwrap_or((command, ""));
        let response = match verb.to_ascii_uppercase().as_str() {
            "QUIT" => return reply(&mut stream, "221 Closing connection").await,
            "EHLO" | "HELO" if !args.is_empty() => {
                greeted = true;
                sender = false;
                recipient = None;
                "250 localhost"
            }
            "NOOP" => "250 OK",
            "RSET" => {
                sender = false;
                recipient = None;
                "250 OK"
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
