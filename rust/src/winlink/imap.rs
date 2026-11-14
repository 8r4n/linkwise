//! Read-only IMAP access to the local B2F inbox.

use std::{
    collections::HashSet,
    fs::{self, File},
    io,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpListener,
    task::JoinSet,
    time::timeout,
};

use super::{invalid, mailbox::DirectoryMailbox, mime};

struct InboxMessage {
    mime: Vec<u8>,
    internal_date: String,
}

struct Inbox {
    validity: u32,
    messages: Vec<Arc<InboxMessage>>,
    ids: HashSet<String>,
    total_bytes: usize,
}

impl Inbox {
    fn load(root: &Path, limit: usize) -> io::Result<Self> {
        DirectoryMailbox::open(root, limit)?;
        let mut inbox = Self {
            validity: next_validity(root)?,
            messages: Vec::new(),
            ids: HashSet::new(),
            total_bytes: 0,
        };
        inbox.refresh(root, limit)?;
        Ok(inbox)
    }

    fn refresh(&mut self, root: &Path, limit: usize) -> io::Result<()> {
        let mut ids = Vec::new();
        for entry in fs::read_dir(root.join("inbox"))? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(stem) = name.to_str().and_then(|name| name.strip_suffix(".b2f")) else {
                continue;
            };
            let Ok(bytes) = (0..stem.len())
                .step_by(2)
                .map(|i| {
                    stem.get(i..i + 2)
                        .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                        .ok_or(())
                })
                .collect::<Result<Vec<_>, _>>()
            else {
                continue;
            };
            let Ok(id) = String::from_utf8(bytes) else {
                continue;
            };
            if super::valid_mid(&id)
                && !self.ids.contains(&id)
                && stem.bytes().all(|b| !b.is_ascii_uppercase())
            {
                ids.push(id);
            }
        }
        ids.sort_unstable();
        if ids.is_empty() {
            return Ok(());
        }
        let mailbox = DirectoryMailbox::open(root, limit)?;
        let mut messages = Vec::new();
        let mut unrenderable = Vec::new();
        let mut total = self.total_bytes;
        for id in ids {
            let wire = mailbox.incoming(&id)?;
            let message = match mime::export(&wire, limit) {
                Ok(message) => message,
                Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                    unrenderable.push(id);
                    continue;
                }
                Err(error) => return Err(error),
            };
            total = total.saturating_add(message.len());
            if total > 64 * 1024 * 1024 {
                return Err(invalid("IMAP inbox exceeds 64 MiB"));
            }
            let envelope = super::Message::decode(&wire, limit)?;
            let date = chrono::NaiveDateTime::parse_from_str(envelope.date, "%Y/%m/%d %H:%M")
                .map_err(|_| invalid("invalid B2F message date"))?;
            messages.push((
                id,
                Arc::new(InboxMessage {
                    mime: message,
                    internal_date: date.format("%e-%b-%Y %H:%M:%S +0000").to_string(),
                }),
            ));
        }
        for (id, message) in messages {
            self.ids.insert(id);
            self.messages.push(message);
        }
        for id in unrenderable {
            eprintln!("IMAP skipped a B2F message that cannot be represented as MIME");
            self.ids.insert(id);
        }
        self.total_bytes = total;
        Ok(())
    }
}

// Reserve a new generation before advertising it. A restart can reorder
// messages, so clients must discard UID mappings from an earlier snapshot.
fn next_validity(root: &Path) -> io::Result<u32> {
    let generations = root.join("imap-generations");
    fs::create_dir_all(&generations)?;
    let mut latest = 0u32;
    for entry in fs::read_dir(&generations)? {
        if let Ok(value) = entry?.file_name().to_string_lossy().parse::<u32>() {
            latest = latest.max(value);
        }
    }
    loop {
        latest = latest
            .checked_add(1)
            .ok_or_else(|| invalid("IMAP UIDVALIDITY exhausted"))?;
        match File::create_new(generations.join(latest.to_string())) {
            Ok(file) => {
                file.sync_all()?;
                #[cfg(unix)]
                {
                    File::open(&generations)?.sync_all()?;
                    File::open(root)?.sync_all()?;
                }
                return Ok(latest);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

pub struct Login {
    username: String,
    password: String,
}

impl Login {
    pub fn new(username: String, password: String) -> io::Result<Self> {
        // The bench supports ASCII credentials that fit in IMAP atoms.
        if [&username, &password].into_iter().any(|value| {
            value.is_empty()
                || value.len() > 256
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"@._-+".contains(&byte))
        }) {
            return Err(invalid(
                "mailbox credentials must be ASCII letters, digits, or @._-+",
            ));
        }
        Ok(Self { username, password })
    }

    fn matches(&self, username: &str, password: &str) -> bool {
        self.username == username && self.password == password
    }
}

pub async fn serve(
    root: PathBuf,
    port: u16,
    limit: usize,
    tls: super::mail_tls::Transport,
    login: Option<Login>,
) -> io::Result<()> {
    if login.is_some() && matches!(tls, super::mail_tls::Transport::Plain) {
        return Err(invalid("IMAP login requires TLS"));
    }
    let login = login.map(Arc::new);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    let inbox = Arc::new(Mutex::new(
        tokio::task::spawn_blocking({
            let root = root.clone();
            move || Inbox::load(&root, limit)
        })
        .await??,
    ));
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            _ = clients.join_next(), if !clients.is_empty() => {}
            accepted = listener.accept(), if clients.len() < 8 => {
                let (socket, _) = accepted?;
                let inbox = Arc::clone(&inbox);
                let root = root.clone();
                let tls = tls.clone();
                let login = login.clone();
                clients.spawn(async move {
                    let _ = timeout(Duration::from_secs(300), async {
                        session(tls.accept(socket).await?, &inbox, &root, limit, login.as_deref()).await
                    }).await;
                });
            }
        }
    }
}

async fn session(
    socket: impl AsyncRead + AsyncWrite + Unpin,
    inbox: &Arc<Mutex<Inbox>>,
    root: &Path,
    limit: usize,
    login: Option<&Login>,
) -> io::Result<()> {
    let mut authenticated = login.is_none();
    let mut stream = BufReader::new(socket);
    send(
        &mut stream,
        if authenticated {
            "* PREAUTH [CAPABILITY IMAP4rev1] Linkwise local inbox\r\n"
        } else {
            "* OK [CAPABILITY IMAP4rev1] Linkwise local inbox\r\n"
        },
    )
    .await?;
    let mut selected = false;
    let mut visible_count = 0;
    'commands: loop {
        let mut raw = Vec::new();
        (&mut stream).take(4096).read_until(b'\n', &mut raw).await?;
        if raw.is_empty() {
            return Ok(());
        }
        let line = std::str::from_utf8(&raw)
            .ok()
            .and_then(|line| line.strip_suffix("\r\n"))
            .ok_or_else(|| invalid("invalid IMAP command line"))?;
        let mut words = line.split_ascii_whitespace();
        let tag = words
            .next()
            .filter(|tag| {
                tag.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
            .ok_or_else(|| invalid("invalid IMAP command tag"))?;
        let mut command = words.next().unwrap_or_default().to_ascii_uppercase();
        let uid = command == "UID";
        if uid {
            command = words.next().unwrap_or_default().to_ascii_uppercase();
        }
        let args: Vec<_> = words.collect();
        if uid && (!authenticated || !selected || !matches!(command.as_str(), "SEARCH" | "FETCH")) {
            send(
                &mut stream,
                &format!("{tag} BAD Unsupported UID command or state\r\n"),
            )
            .await?;
            continue;
        }
        if !authenticated {
            match (command.as_str(), args.as_slice()) {
                ("LOGIN", [username, password]) => {
                    let username = username
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or(username);
                    let password = password
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .unwrap_or(password);
                    authenticated = login.is_some_and(|login| login.matches(username, password));
                    let response = if authenticated {
                        "OK LOGIN completed"
                    } else {
                        "NO [AUTHENTICATIONFAILED] Invalid credentials"
                    };
                    send(&mut stream, &format!("{tag} {response}\r\n")).await?;
                    continue;
                }
                ("CAPABILITY" | "NOOP" | "LOGOUT", []) => {}
                _ => {
                    send(
                        &mut stream,
                        &format!("{tag} BAD Authenticate with LOGIN first\r\n"),
                    )
                    .await?;
                    continue;
                }
            }
        }
        let selecting = matches!(command.as_str(), "SELECT" | "EXAMINE")
            && matches!(args.as_slice(), [name] if is_inbox(name));
        let (validity, messages) = if selecting || (selected && command != "LOGOUT") {
            let inbox = Arc::clone(inbox);
            let root = root.to_owned();
            let refreshed = tokio::task::spawn_blocking(move || {
                let mut inbox = inbox
                    .lock()
                    .map_err(|_| io::Error::other("IMAP inbox lock poisoned"))?;
                inbox.refresh(&root, limit)?;
                Ok::<_, io::Error>((inbox.validity, inbox.messages.clone()))
            })
            .await?;
            match refreshed {
                Ok(snapshot) => snapshot,
                Err(_) => {
                    send(&mut stream, &format!("{tag} NO Inbox refresh failed\r\n")).await?;
                    continue;
                }
            }
        } else {
            (0, Vec::new())
        };
        let count = messages.len();
        if selected && !selecting && command != "LOGOUT" && count != visible_count {
            send(&mut stream, &format!("* {count} EXISTS\r\n")).await?;
            visible_count = count;
        }
        let response = match (command.as_str(), args.as_slice()) {
            ("CAPABILITY", []) => {
                send(&mut stream, "* CAPABILITY IMAP4rev1\r\n").await?;
                "OK CAPABILITY completed"
            }
            ("LOGOUT", []) => {
                send(
                    &mut stream,
                    &format!("* BYE Logging out\r\n{tag} OK LOGOUT completed\r\n"),
                )
                .await?;
                return Ok(());
            }
            ("NOOP", []) => "OK NOOP completed",
            ("LIST" | "LSUB", ["\"\"", pattern])
                if is_inbox(pattern) || matches!(*pattern, "*" | "%" | "\"*\"" | "\"%\"") =>
            {
                send(
                    &mut stream,
                    &format!("* {command} (\\Noinferiors) NIL INBOX\r\n"),
                )
                .await?;
                "OK List completed"
            }
            ("LIST" | "LSUB", [_, _]) => "OK List completed",
            ("SELECT" | "EXAMINE", [name]) if is_inbox(name) => {
                selected = true;
                visible_count = count;
                send(&mut stream, &format!(
                    "* FLAGS ()\r\n* {count} EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY {}] Snapshot generation\r\n* OK [UIDNEXT {}] Next UID\r\n* OK [PERMANENTFLAGS ()] Read-only flags\r\n",
                    validity, count + 1
                )).await?;
                "OK [READ-ONLY] INBOX selected"
            }
            ("SELECT" | "EXAMINE", [_]) => {
                selected = false;
                "NO Mailbox does not exist"
            }
            ("CLOSE", []) if selected => {
                selected = false;
                "OK CLOSE completed"
            }
            ("SEARCH", _) if selected => {
                let mut filters = Vec::new();
                let mut keys = args.iter().peekable();
                while let Some(key) = keys.next() {
                    if key.eq_ignore_ascii_case("NOT")
                        && keys
                            .peek()
                            .is_some_and(|key| key.eq_ignore_ascii_case("DELETED"))
                    {
                        keys.next();
                        continue;
                    }
                    if key.eq_ignore_ascii_case("ALL") || key.eq_ignore_ascii_case("UNDELETED") {
                        continue;
                    }
                    let set = if key.eq_ignore_ascii_case("UID") {
                        keys.next()
                    } else {
                        Some(key)
                    };
                    let Some(ranges) = set.and_then(|set| message_ranges(set, count)) else {
                        send(
                            &mut stream,
                            &format!("{tag} BAD Unsupported SEARCH criteria\r\n"),
                        )
                        .await?;
                        continue 'commands;
                    };
                    filters.push(ranges);
                }
                let mut result = String::from("* SEARCH");
                for index in 1..=count {
                    if filters
                        .iter()
                        .all(|ranges| ranges.iter().any(|range| range.contains(&index)))
                    {
                        use std::fmt::Write;
                        write!(result, " {index}").expect("writing to String");
                    }
                }
                result.push_str("\r\n");
                send(&mut stream, &result).await?;
                "OK SEARCH completed"
            }
            ("FETCH", [set, items @ ..]) if selected => {
                fetch(&mut stream, &messages, set, items, uid).await?
            }
            _ => "BAD Unsupported command or state",
        };
        send(&mut stream, &format!("{tag} {response}\r\n")).await?;
    }
}

fn is_inbox(name: &str) -> bool {
    name.strip_prefix('"')
        .and_then(|name| name.strip_suffix('"'))
        .unwrap_or(name)
        .eq_ignore_ascii_case("INBOX")
}

fn message_ranges(set: &str, count: usize) -> Option<Vec<std::ops::RangeInclusive<usize>>> {
    let number = |text: &str| {
        if text == "*" {
            Some(count.max(1))
        } else {
            text.parse::<u32>()
                .ok()
                .filter(|n| *n > 0)
                .map(|n| n as usize)
        }
    };
    let mut ranges = Vec::new();
    for range in set.split(',') {
        let (first, last) = range.split_once(':').unwrap_or((range, range));
        let (Some(first), Some(last)) = (number(first), number(last)) else {
            return None;
        };
        ranges.push(first.min(last)..=first.max(last));
    }
    Some(ranges)
}

async fn fetch(
    stream: &mut BufReader<impl AsyncRead + AsyncWrite + Unpin>,
    messages: &[Arc<InboxMessage>],
    set: &str,
    items: &[&str],
    uid: bool,
) -> io::Result<&'static str> {
    let Some(ranges) = message_ranges(set, messages.len()) else {
        return Ok("BAD Invalid message set");
    };
    let items = items.join(" ").to_ascii_uppercase();
    let items = items
        .strip_prefix('(')
        .and_then(|items| items.strip_suffix(')'))
        .unwrap_or(&items);
    let mut attributes = Vec::new();
    let mut remaining = items.trim();
    while !remaining.is_empty() {
        let end = if remaining.starts_with("BODY.PEEK[HEADER.FIELDS (")
            || remaining.starts_with("BODY[HEADER.FIELDS (")
        {
            let Some(end) = remaining.find(")]") else {
                return Ok("BAD Invalid header field list");
            };
            end + 2
        } else {
            remaining
                .find(char::is_whitespace)
                .unwrap_or(remaining.len())
        };
        attributes.push(&remaining[..end]);
        remaining = remaining[end..].trim_start();
    }
    if attributes.is_empty()
        || attributes.iter().any(|item| {
            !matches!(
                *item,
                "UID"
                    | "FLAGS"
                    | "RFC822.SIZE"
                    | "RFC822"
                    | "BODY[]"
                    | "BODY.PEEK[]"
                    | "INTERNALDATE"
            ) && header_fields(item).is_none()
        })
    {
        return Ok("BAD Unsupported FETCH attributes");
    }
    if uid && !attributes.contains(&"UID") {
        attributes.insert(0, "UID");
    }
    for (index, message) in messages.iter().enumerate() {
        let index = index + 1;
        if !ranges.iter().any(|range| range.contains(&index)) {
            continue;
        }
        send(stream, &format!("* {index} FETCH (")).await?;
        for (position, item) in attributes.iter().enumerate() {
            if position != 0 {
                send(stream, " ").await?;
            }
            let message = message.as_ref();
            match *item {
                "INTERNALDATE" => {
                    send(
                        stream,
                        &format!("INTERNALDATE \"{}\"", message.internal_date),
                    )
                    .await?
                }
                section if header_fields(section).is_some() => {
                    let fields = header_fields(section).expect("header section");
                    let subset = selected_headers(&message.mime, fields);
                    let section = section.strip_prefix("BODY.PEEK").unwrap_or(&section[4..]);
                    send(stream, &format!("BODY{section} {{{}}}\r\n", subset.len())).await?;
                    stream.write_all(&subset).await?;
                }
                "UID" => send(stream, &format!("UID {index}")).await?,
                "FLAGS" => send(stream, "FLAGS ()").await?,
                "RFC822.SIZE" => {
                    send(stream, &format!("RFC822.SIZE {}", message.mime.len())).await?
                }
                literal => {
                    let name = if literal == "RFC822" {
                        literal
                    } else {
                        "BODY[]"
                    };
                    send(stream, &format!("{name} {{{}}}\r\n", message.mime.len())).await?;
                    stream.write_all(&message.mime).await?;
                }
            }
        }
        send(stream, ")\r\n").await?;
    }
    Ok("OK FETCH completed")
}

fn header_fields(section: &str) -> Option<&str> {
    section
        .strip_prefix("BODY.PEEK[HEADER.FIELDS (")
        .or_else(|| section.strip_prefix("BODY[HEADER.FIELDS ("))?
        .strip_suffix(")]")
}

fn selected_headers(mime: &[u8], fields: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let mut include = false;
    for line in mime.split_inclusive(|byte| *byte == b'\n') {
        if line == b"\r\n" {
            break;
        }
        if !line.starts_with(b" ") && !line.starts_with(b"\t") {
            include = line
                .iter()
                .position(|byte| *byte == b':')
                .is_some_and(|colon| {
                    fields
                        .split_ascii_whitespace()
                        .any(|field| line[..colon].eq_ignore_ascii_case(field.as_bytes()))
                });
        }
        if include {
            result.extend_from_slice(line);
        }
    }
    result.extend_from_slice(b"\r\n");
    result
}

async fn send(
    stream: &mut BufReader<impl AsyncRead + AsyncWrite + Unpin>,
    response: &str,
) -> io::Result<()> {
    stream.write_all(response.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_date_does_not_hide_valid_mail_on_load_or_refresh() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("linkwise-imap-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            let limit = 4096;
            DirectoryMailbox::open(&root, limit)?;
            let good = super::super::tests::FIXTURE;
            let bad = good
                .split_at(b"Mid: TEST123\r\nDate: 2026/11/13 12:00".len())
                .1;
            let mut wire = b"Mid: BADDATE\r\nDate: yesterday".to_vec();
            wire.extend_from_slice(bad);
            fs::write(root.join("inbox/42414444415445.b2f"), wire)?;
            let mut inbox = Inbox::load(&root, limit)?;
            assert!(inbox.messages.is_empty());
            fs::write(root.join("inbox/54455354313233.b2f"), good)?;
            inbox.refresh(&root, limit)?;
            assert_eq!(inbox.messages.len(), 1);
            assert_eq!(Inbox::load(&root, limit)?.messages.len(), 1);
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }
}
