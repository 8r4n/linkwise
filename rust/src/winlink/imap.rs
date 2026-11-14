//! IMAP access to the local B2F inbox and archived Sent copies.

use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    fs::{self, File},
    io,
    net::SocketAddr,
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
    uid: usize,
    mime: Vec<u8>,
    internal_date: String,
}

struct Inbox {
    validity: u32,
    next_uid: usize,
    assigned: HashMap<String, usize>,
    messages: Vec<Arc<InboxMessage>>,
    ids: HashSet<String>,
    total_bytes: usize,
}

impl Inbox {
    fn load(root: &Path, limit: usize) -> io::Result<Self> {
        DirectoryMailbox::open(root, limit)?;
        let state = root.join("imap-inbox");
        fs::create_dir_all(&state)?;
        let generation = state.join("uidvalidity");
        if !generation.try_exists()? {
            let validity = next_validity(root)?;
            match super::mailbox::publish(&generation, validity.to_string().as_bytes()) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        let validity = fs::read_to_string(generation)?
            .parse::<u32>()
            .ok()
            .filter(|value| *value != 0)
            .ok_or_else(|| invalid("invalid Inbox UIDVALIDITY"))?;
        let mut assigned = HashMap::new();
        let mut next_uid = 1;
        loop {
            let id = match fs::read_to_string(state.join(next_uid.to_string())) {
                Ok(id) => id,
                Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                Err(error) => return Err(error),
            };
            if !super::valid_mid(&id) || assigned.insert(id, next_uid).is_some() {
                return Err(invalid("invalid Inbox UID mapping"));
            }
            next_uid += 1;
        }
        let mut inbox = Self {
            validity,
            next_uid,
            assigned,
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
        ids.sort_unstable_by(|a, b| {
            let order = |id: &String| self.assigned.get(id).copied().unwrap_or(usize::MAX);
            order(a).cmp(&order(b)).then_with(|| a.cmp(b))
        });
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
            let uid = if let Some(&uid) = self.assigned.get(&id) {
                uid
            } else {
                let uid = self.next_uid;
                if uid >= u32::MAX as usize {
                    return Err(invalid("Inbox UID space exhausted"));
                }
                let path = root.join("imap-inbox").join(uid.to_string());
                super::mailbox::publish(&path, id.as_bytes())?;
                self.assigned.insert(id.clone(), uid);
                self.next_uid += 1;
                uid
            };
            messages.push((
                id,
                Arc::new(InboxMessage {
                    uid,
                    mime: message,
                    internal_date: date.format("%e-%b-%Y %H:%M:%S +0000").to_string(),
                }),
            ));
        }
        for (id, message) in messages {
            self.ids.insert(id);
            let position = self
                .messages
                .partition_point(|existing| existing.uid < message.uid);
            self.messages.insert(position, message);
        }
        for id in unrenderable {
            eprintln!("IMAP skipped a B2F message that cannot be represented as MIME");
            self.ids.insert(id);
        }
        self.total_bytes = total;
        Ok(())
    }
}

// Sent UIDs are the filenames of immutable messages. The folder is append-only.
fn refresh_sent(root: &Path, sent: &mut Inbox, limit: usize) -> io::Result<()> {
    loop {
        let path = root.join((sent.messages.len() + 1).to_string());
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        use std::io::Read as _;
        let stored_at = chrono::DateTime::<chrono::Utc>::from(file.metadata()?.modified()?);
        let mut mime = Vec::new();
        file.take(2 * limit as u64 + 1).read_to_end(&mut mime)?;
        if mime.len() > 2 * limit || sent.total_bytes + mime.len() > 64 * 1024 * 1024 {
            return Err(invalid("Sent archive exceeds its size limit"));
        }
        sent.total_bytes += mime.len();
        let uid = sent.next_uid;
        sent.next_uid += 1;
        sent.messages.push(Arc::new(InboxMessage {
            uid,
            internal_date: stored_at.format("%e-%b-%Y %H:%M:%S +0000").to_string(),
            mime,
        }));
    }
}

// Reserve a generation newer than any legacy snapshot before publishing
// the persistent Inbox UID mapping.
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

    pub(crate) fn matches(&self, username: &str, password: &str) -> bool {
        self.username == username && self.password == password
    }
}

pub async fn serve(
    root: PathBuf,
    address: SocketAddr,
    limit: usize,
    tls: super::mail_tls::Transport,
    login: Option<Login>,
) -> io::Result<()> {
    if login.is_some() && matches!(tls, super::mail_tls::Transport::Plain) {
        return Err(invalid("IMAP login requires TLS"));
    }
    if !address.ip().is_loopback()
        && (login.is_none() || matches!(tls, super::mail_tls::Transport::Plain))
    {
        return Err(invalid(
            "Non-loopback mail listeners require TLS and authentication",
        ));
    }
    let login = login.map(Arc::new);
    let listener = TcpListener::bind(address).await?;
    let inbox = Arc::new(Mutex::new(
        tokio::task::spawn_blocking({
            let root = root.clone();
            move || Inbox::load(&root, limit)
        })
        .await??,
    ));
    let sent = Arc::new(Mutex::new(Inbox {
        validity: 1,
        next_uid: 1,
        assigned: HashMap::new(),
        messages: Vec::new(),
        ids: HashSet::new(),
        total_bytes: 0,
    }));
    let sent_root = root.join("imap-sent");
    tokio::task::spawn_blocking({
        let sent_root = sent_root.clone();
        move || fs::create_dir_all(sent_root)
    })
    .await??;
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            _ = clients.join_next(), if !clients.is_empty() => {}
            accepted = listener.accept(), if clients.len() < 8 => {
                let (socket, _) = accepted?;
                let inbox = Arc::clone(&inbox);
                let root = root.clone();
                let sent = Arc::clone(&sent);
                let sent_root = sent_root.clone();
                let tls = tls.clone();
                let login = login.clone();
                clients.spawn(async move {
                    let _ = timeout(Duration::from_secs(300), async {
                        session(tls.accept(socket).await?, (&inbox, &root), (&sent, &sent_root), limit, login.as_deref()).await
                    }).await;
                });
            }
        }
    }
}

const MAX_COMMAND_BYTES: u64 = 4096;

struct Command<'a> {
    tag: &'a str,
    name: String,
    uid: bool,
    arguments: Vec<&'a str>,
}

fn parse_command(line: &str) -> Result<Command<'_>, Option<&str>> {
    let mut words = line.splitn(2, ' ');
    let tag = words
        .next()
        .filter(|tag| {
            !tag.is_empty()
                && tag
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
        .ok_or(None)?;
    let bad = Some(tag);
    let mut rest = words.next().ok_or(bad)?.trim_start_matches(' ');
    let mut tokens = Vec::new();
    while !rest.is_empty() {
        let mut quoted = false;
        let mut escaped = false;
        let mut end = rest.len();
        for (offset, byte) in rest.bytes().enumerate() {
            if escaped {
                if !matches!(byte, b'"' | b'\\') {
                    return Err(bad);
                }
                escaped = false;
                continue;
            }
            match byte {
                b'\\' if quoted => escaped = true,
                b'"' => quoted = !quoted,
                b' ' if !quoted => {
                    end = offset;
                    break;
                }
                0..=31 | 127 => return Err(bad),
                _ => {}
            }
        }
        if quoted || escaped {
            return Err(bad);
        }
        tokens.push(&rest[..end]);
        rest = rest[end..].trim_start_matches(' ');
    }
    let mut tokens = tokens.into_iter();
    let name = tokens.next().ok_or(bad)?;
    let uid = name.eq_ignore_ascii_case("UID");
    let name = if uid { tokens.next().ok_or(bad)? } else { name };
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(bad);
    }
    Ok(Command {
        tag,
        name: name.to_ascii_uppercase(),
        uid,
        arguments: tokens.collect(),
    })
}

// Persist read state separately from the immutable message and radio queue.
fn seen_state(root: &Path, folder: &str, uid: usize) -> PathBuf {
    let directory = if folder == "Sent" {
        "imap-sent-seen"
    } else {
        "imap-inbox-seen"
    };
    root.join(directory).join(uid.to_string())
}

fn set_seen(path: &Path, seen: bool) -> io::Result<()> {
    let directory = path.parent().expect("flag directory");
    if seen {
        fs::create_dir_all(directory)?;
        match super::mailbox::publish(path, b"") {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            result => result?,
        }
        #[cfg(unix)]
        File::open(directory.parent().expect("mailbox root"))?.sync_all()?;
    } else {
        match fs::remove_file(path) {
            Ok(()) => {
                #[cfg(unix)]
                File::open(directory)?.sync_all()?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn flags(seen: bool) -> &'static str {
    if seen { "(\\Seen)" } else { "()" }
}

async fn session(
    socket: impl AsyncRead + AsyncWrite + Unpin,
    inbox: (&Arc<Mutex<Inbox>>, &Path),
    sent: (&Arc<Mutex<Inbox>>, &Path),
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
    let mut selected = None;
    let mut read_only = true;
    let mut visible_count = 0;
    'commands: loop {
        let mut raw = Vec::new();
        (&mut stream)
            .take(MAX_COMMAND_BYTES + 1)
            .read_until(b'\n', &mut raw)
            .await?;
        if raw.is_empty() {
            return Ok(());
        }
        if raw.len() as u64 > MAX_COMMAND_BYTES || !raw.ends_with(b"\n") {
            send(&mut stream, "* BYE Invalid or oversized command line\r\n").await?;
            return Ok(());
        }
        let parsed = std::str::from_utf8(&raw)
            .ok()
            .and_then(|line| line.strip_suffix("\r\n"))
            .ok_or(None)
            .and_then(parse_command);
        let Command {
            tag,
            name: command,
            uid,
            arguments: args,
        } = match parsed {
            Ok(command) => command,
            Err(tag) => {
                let tag = tag.unwrap_or("*");
                send(
                    &mut stream,
                    &format!("{tag} BAD Invalid command syntax\r\n"),
                )
                .await?;
                continue;
            }
        };
        if uid
            && (!authenticated
                || selected.is_none()
                || !matches!(command.as_str(), "SEARCH" | "FETCH" | "STORE"))
        {
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
        let requested = args.first().and_then(|name| folder(name));
        let selecting = matches!(command.as_str(), "SELECT" | "EXAMINE") && args.len() == 1;
        let snapshot = if selecting || command == "STATUS" {
            requested
        } else if command != "LOGOUT" {
            selected
        } else {
            None
        };
        let (validity, next_uid, messages, seen) = if let Some(folder) = snapshot {
            let (mailbox, root) = if folder == "Sent" { sent } else { inbox };
            let mailbox = Arc::clone(mailbox);
            let root = root.to_owned();
            let refreshed = tokio::task::spawn_blocking(move || {
                let mut inbox = mailbox
                    .lock()
                    .map_err(|_| io::Error::other("IMAP inbox lock poisoned"))?;
                if folder == "Sent" {
                    refresh_sent(&root, &mut inbox, limit)?;
                } else {
                    inbox.refresh(&root, limit)?;
                }
                let seen = inbox
                    .messages
                    .iter()
                    .map(|message| seen_state(&root, folder, message.uid).try_exists())
                    .collect::<io::Result<Vec<_>>>()?;
                Ok::<_, io::Error>((inbox.validity, inbox.next_uid, inbox.messages.clone(), seen))
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
            (0, 1, Vec::new(), Vec::new())
        };
        let count = messages.len();
        if selected.is_some() && snapshot == selected && !selecting && count != visible_count {
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
                if folder(pattern).is_some()
                    || matches!(*pattern, "*" | "%" | "\"*\"" | "\"%\"") =>
            {
                for name in ["INBOX", "Sent"] {
                    if folder(pattern).is_none() || folder(pattern) == Some(name) {
                        send(
                            &mut stream,
                            &format!("* {command} (\\Noinferiors) NIL {name}\r\n"),
                        )
                        .await?;
                    }
                }
                "OK List completed"
            }
            ("LIST" | "LSUB", [_, _]) => "OK List completed",
            ("STATUS", [_, items @ ..]) if requested.is_some() => {
                let items = items.join(" ");
                let Some(items) = items
                    .strip_prefix('(')
                    .and_then(|items| items.strip_suffix(')'))
                    .filter(|items| !items.trim().is_empty())
                else {
                    send(
                        &mut stream,
                        &format!("{tag} BAD Invalid STATUS item list\r\n"),
                    )
                    .await?;
                    continue;
                };
                let mut values = Vec::new();
                for item in items.split_ascii_whitespace().map(str::to_ascii_uppercase) {
                    let value = match item.as_str() {
                        "MESSAGES" => count,
                        "UNSEEN" => seen.iter().filter(|flag| !**flag).count(),
                        "RECENT" => 0,
                        "UIDNEXT" => next_uid,
                        "UIDVALIDITY" => validity as usize,
                        _ => {
                            send(
                                &mut stream,
                                &format!("{tag} BAD Unsupported STATUS item\r\n"),
                            )
                            .await?;
                            continue 'commands;
                        }
                    };
                    values.push(format!("{item} {value}"));
                }
                send(
                    &mut stream,
                    &format!(
                        "* STATUS {} ({})\r\n",
                        requested.expect("known mailbox"),
                        values.join(" ")
                    ),
                )
                .await?;
                "OK STATUS completed"
            }
            ("CREATE", [_]) if requested.is_some() => "NO Mailbox already exists",
            ("CREATE", [_]) => "NO Only INBOX and Sent are supported",
            ("APPEND", [_, options @ .., literal]) if requested == Some("Sent") => {
                let Some(length) = literal
                    .strip_prefix('{')
                    .and_then(|s| s.strip_suffix('}'))
                    .and_then(|s| s.parse::<usize>().ok())
                else {
                    send(
                        &mut stream,
                        &format!("{tag} BAD Invalid APPEND literal\r\n"),
                    )
                    .await?;
                    continue;
                };
                if !options.is_empty()
                    && !matches!(options, [flags] if flags.eq_ignore_ascii_case("(\\Seen)"))
                {
                    send(
                        &mut stream,
                        &format!("{tag} BAD Unsupported APPEND options\r\n"),
                    )
                    .await?;
                    continue;
                }
                if length == 0 || length > 2 * limit {
                    send(
                        &mut stream,
                        &format!("{tag} NO APPEND exceeds message size limit\r\n"),
                    )
                    .await?;
                    continue;
                }
                send(&mut stream, "+ Ready for message\r\n").await?;
                let mut mime = vec![0; length];
                stream.read_exact(&mut mime).await?;
                let mut end = [0; 2];
                stream.read_exact(&mut end).await?;
                if end != *b"\r\n" {
                    return Err(invalid("invalid APPEND terminator"));
                }
                let (archive, root) = (Arc::clone(sent.0), sent.1.to_owned());
                let saved = tokio::task::spawn_blocking(move || {
                    let mut archive = archive
                        .lock()
                        .map_err(|_| io::Error::other("Sent lock poisoned"))?;
                    refresh_sent(&root, &mut archive, limit)?;
                    if archive.total_bytes + mime.len() > 64 * 1024 * 1024 {
                        return Err(invalid("Sent archive is full"));
                    }
                    super::mailbox::publish(
                        &root.join((archive.messages.len() + 1).to_string()),
                        &mime,
                    )
                })
                .await?;
                if saved.is_ok() {
                    "OK APPEND completed"
                } else {
                    "NO Could not archive message"
                }
            }
            ("APPEND", _) => "NO APPEND is supported only for Sent",
            ("STATUS", [_, ..]) => "NO Mailbox does not exist",
            ("SELECT" | "EXAMINE", [_]) if requested.is_some() => {
                selected = requested;
                read_only = command == "EXAMINE";
                visible_count = count;
                let permanent = if read_only { "()" } else { "(\\Seen)" };
                send(&mut stream, &format!(
                    "* FLAGS (\\Seen)\r\n* {count} EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY {validity}] Snapshot generation\r\n* OK [UIDNEXT {next_uid}] Next UID\r\n* OK [PERMANENTFLAGS {permanent}] Supported flags\r\n",
                )).await?;
                if read_only {
                    "OK [READ-ONLY] Mailbox selected"
                } else {
                    "OK [READ-WRITE] Mailbox selected"
                }
            }
            ("SELECT" | "EXAMINE", [_]) => {
                selected = None;
                "NO Mailbox does not exist"
            }
            ("CHECK", []) if selected.is_some() => "OK CHECK completed",
            ("CLOSE" | "UNSELECT", []) if selected.is_some() => {
                selected = None;
                if command == "CLOSE" {
                    "OK CLOSE completed"
                } else {
                    "OK UNSELECT completed"
                }
            }
            ("SEARCH", _) if selected.is_some() => {
                let mut filters = Vec::new();
                let mut seen_filters = Vec::new();
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
                    if key.eq_ignore_ascii_case("SEEN") || key.eq_ignore_ascii_case("UNSEEN") {
                        seen_filters.push(key.eq_ignore_ascii_case("SEEN"));
                        continue;
                    }
                    let by_uid = key.eq_ignore_ascii_case("UID");
                    let set = if by_uid { keys.next() } else { Some(key) };
                    let Some(ranges) = set.and_then(|set| {
                        message_ranges(
                            set,
                            if by_uid {
                                messages.last().map_or(0, |message| message.uid)
                            } else {
                                count
                            },
                        )
                    }) else {
                        send(
                            &mut stream,
                            &format!("{tag} BAD Unsupported SEARCH criteria\r\n"),
                        )
                        .await?;
                        continue 'commands;
                    };
                    filters.push((by_uid, ranges));
                }
                let mut result = String::from("* SEARCH");
                for (index, message) in messages.iter().enumerate() {
                    let index = index + 1;
                    if seen_filters
                        .iter()
                        .all(|required| *required == seen[index - 1])
                        && filters.iter().all(|(by_uid, ranges)| {
                            let number = if *by_uid { message.uid } else { index };
                            ranges.iter().any(|range| range.contains(&number))
                        })
                    {
                        let number = if uid { message.uid } else { index };
                        write!(result, " {number}").expect("writing to String");
                    }
                }
                result.push_str("\r\n");
                send(&mut stream, &result).await?;
                "OK SEARCH completed"
            }
            ("STORE", [_, _, _]) if selected.is_some() && read_only => "NO Mailbox is read-only",
            ("STORE", [set, operation, flag_list]) if selected.is_some() => {
                let folder = selected.expect("selected mailbox");
                let (mailbox, root) = if folder == "Sent" { sent } else { inbox };
                let maximum = if uid {
                    messages.last().map_or(0, |message| message.uid)
                } else {
                    count
                };
                let Some(ranges) = message_ranges(set, maximum) else {
                    send(&mut stream, &format!("{tag} BAD Invalid message set\r\n")).await?;
                    continue;
                };
                let operation = operation.to_ascii_uppercase();
                let silent = operation.ends_with(".SILENT");
                let operation = operation.strip_suffix(".SILENT").unwrap_or(&operation);
                let flag_list = flag_list
                    .strip_prefix('(')
                    .and_then(|list| list.strip_suffix(')'))
                    .unwrap_or(flag_list);
                if !matches!(operation, "FLAGS" | "+FLAGS" | "-FLAGS")
                    || (!flag_list.is_empty() && !flag_list.eq_ignore_ascii_case("\\Seen"))
                {
                    send(
                        &mut stream,
                        &format!("{tag} BAD Unsupported STORE flags or operation\r\n"),
                    )
                    .await?;
                    continue;
                }
                let mutations = messages
                    .iter()
                    .enumerate()
                    .filter_map(|(index, message)| {
                        let number = if uid { message.uid } else { index + 1 };
                        if !ranges.iter().any(|range| range.contains(&number)) {
                            return None;
                        }
                        let value = match (operation, flag_list.is_empty()) {
                            ("FLAGS", empty) => Some(!empty),
                            ("+FLAGS", false) => Some(true),
                            ("-FLAGS", false) => Some(false),
                            _ => None,
                        };
                        Some((index + 1, message.uid, value))
                    })
                    .collect::<Vec<_>>();
                let root = root.to_owned();
                let mailbox = Arc::clone(mailbox);
                let updated = tokio::task::spawn_blocking(move || {
                    let _guard = mailbox
                        .lock()
                        .map_err(|_| io::Error::other("IMAP mailbox lock poisoned"))?;
                    let mut results = Vec::new();
                    for (index, uid, value) in mutations {
                        let path = seen_state(&root, folder, uid);
                        if let Some(value) = value {
                            set_seen(&path, value)?;
                        }
                        results.push((index, uid, path.try_exists()?));
                    }
                    Ok::<_, io::Error>(results)
                })
                .await?;
                match updated {
                    Ok(results) => {
                        if !silent {
                            for (index, uid, seen) in results {
                                send(
                                    &mut stream,
                                    &format!(
                                        "* {index} FETCH (UID {uid} FLAGS {})\r\n",
                                        flags(seen)
                                    ),
                                )
                                .await?;
                            }
                        }
                        "OK STORE completed"
                    }
                    Err(_) => "NO Could not save message flags",
                }
            }
            ("FETCH", [set, items @ ..]) if selected.is_some() => {
                fetch(&mut stream, &messages, &seen, set, items, uid).await?
            }
            _ => "BAD Unsupported command or state",
        };
        send(&mut stream, &format!("{tag} {response}\r\n")).await?;
    }
}

fn folder(name: &str) -> Option<&'static str> {
    let name = name
        .strip_prefix('"')
        .and_then(|name| name.strip_suffix('"'))
        .unwrap_or(name);
    ["INBOX", "Sent"]
        .into_iter()
        .find(|folder| folder.eq_ignore_ascii_case(name))
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
    seen: &[bool],
    set: &str,
    items: &[&str],
    uid: bool,
) -> io::Result<&'static str> {
    let Some(ranges) = message_ranges(
        set,
        if uid {
            messages.last().map_or(0, |message| message.uid)
        } else {
            messages.len()
        },
    ) else {
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
                    | "BODYSTRUCTURE"
            ) && header_fields(item).is_none()
                && body_section(item).is_none()
        })
    {
        return Ok("BAD Unsupported FETCH attributes");
    }
    if uid && !attributes.contains(&"UID") {
        attributes.insert(0, "UID");
    }
    for (index, message) in messages.iter().enumerate() {
        let index = index + 1;
        let number = if uid { message.uid } else { index };
        if !ranges.iter().any(|range| range.contains(&number)) {
            continue;
        }
        let mime = attributes
            .iter()
            .any(|item| *item == "BODYSTRUCTURE" || body_section(item).is_some())
            .then(|| mail_parser::MessageParser::default().parse(&message.mime))
            .flatten();
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
                "BODYSTRUCTURE" => {
                    let structure = mime.as_ref().and_then(|mail| body_structure(mail, 0, 0));
                    send(
                        stream,
                        &format!("BODYSTRUCTURE {}", structure.as_deref().unwrap_or("NIL")),
                    )
                    .await?;
                }
                attribute if body_section(attribute).is_some() => {
                    let section = body_section(attribute).expect("valid section");
                    let body = mime
                        .as_ref()
                        .and_then(|mail| section_part(mail, section))
                        .and_then(|part| {
                            let body_start = part.offset_body as usize;
                            if section == "HEADER" {
                                message.mime.get(part.offset_header as usize..body_start)
                            } else {
                                message.mime.get(body_start..part.offset_end as usize)
                            }
                        });
                    if let Some(body) = body {
                        send(stream, &format!("BODY[{section}] {{{}}}\r\n", body.len())).await?;
                        stream.write_all(body).await?;
                    } else {
                        send(stream, &format!("BODY[{section}] NIL")).await?;
                    }
                }
                "UID" => send(stream, &format!("UID {}", message.uid)).await?,
                "FLAGS" => send(stream, &format!("FLAGS {}", flags(seen[index - 1]))).await?,
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

fn body_section(attribute: &str) -> Option<&str> {
    let section = attribute
        .strip_prefix("BODY.PEEK[")
        .or_else(|| attribute.strip_prefix("BODY["))?
        .strip_suffix(']')?;
    (matches!(section, "HEADER" | "TEXT")
        || section
            .split('.')
            .all(|index| index.parse::<usize>().is_ok_and(|index| index > 0)))
    .then_some(section)
}

fn section_part<'a, 'm>(
    mail: &'a mail_parser::Message<'m>,
    section: &str,
) -> Option<&'a mail_parser::MessagePart<'m>> {
    let mut part = mail.part(0)?;
    if !matches!(section, "HEADER" | "TEXT") {
        for (depth, index) in section.split('.').enumerate() {
            let index = index.parse::<usize>().ok()?.checked_sub(1)?;
            if let mail_parser::PartType::Multipart(children) = &part.body {
                part = mail.part(*children.get(index)?)?;
            } else if depth != 0 || index != 0 {
                return None;
            }
        }
    }
    Some(part)
}

fn imap_string(value: &str) -> String {
    if !value.bytes().all(|byte| matches!(byte, 32..=126)) {
        return format!("{{{}}}\r\n{value}", value.len());
    }
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn mime_parameters(content: Option<&mail_parser::ContentType<'_>>) -> String {
    match content.and_then(mail_parser::ContentType::attributes) {
        Some(attributes) if !attributes.is_empty() => format!(
            "({})",
            attributes
                .iter()
                .map(|attribute| format!(
                    "{} {}",
                    imap_string(&attribute.name),
                    imap_string(&attribute.value)
                ))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        _ => "NIL".into(),
    }
}

fn body_structure(mail: &mail_parser::Message<'_>, id: u32, depth: usize) -> Option<String> {
    use mail_parser::{MimeHeaders, PartType};
    if depth > 32 {
        return None;
    }
    let part = mail.part(id)?;
    let content = part.content_type();
    let parameters = mime_parameters(content);
    let disposition = part.content_disposition().map_or_else(
        || "NIL".into(),
        |value| {
            format!(
                "({} {})",
                imap_string(value.ctype()),
                mime_parameters(Some(value))
            )
        },
    );
    if let PartType::Multipart(children) = &part.body {
        let children = children
            .iter()
            .map(|id| body_structure(mail, *id, depth + 1))
            .collect::<Option<Vec<_>>>()?
            .concat();
        let subtype = imap_string(
            content
                .and_then(mail_parser::ContentType::subtype)
                .unwrap_or("mixed"),
        );
        return Some(format!(
            "({children} {subtype} {parameters} {disposition} NIL NIL)"
        ));
    }
    // Nested message/rfc822 needs an envelope and its own body structure.
    if matches!(part.body, PartType::Message(_)) {
        return None;
    }
    let raw = mail
        .raw_message
        .get(part.offset_body as usize..part.offset_end as usize)?;
    let kind = content.map_or("text", mail_parser::ContentType::ctype);
    let subtype = content
        .and_then(mail_parser::ContentType::subtype)
        .unwrap_or("plain");
    let encoding = match part.encoding {
        mail_parser::Encoding::None => "7bit",
        mail_parser::Encoding::QuotedPrintable => "quoted-printable",
        mail_parser::Encoding::Base64 => "base64",
    };
    let lines = if kind.eq_ignore_ascii_case("text") {
        format!(" {}", raw.iter().filter(|byte| **byte == b'\n').count())
    } else {
        String::new()
    };
    Some(format!(
        "({} {} {parameters} NIL NIL \"{encoding}\" {}{lines} NIL {disposition} NIL NIL)",
        imap_string(kind),
        imap_string(subtype),
        raw.len()
    ))
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
    fn command_parser_preserves_quoted_arguments_and_uid_mode() {
        let command =
            parse_command(r#"9.1 uid fetch 1 (BODY.PEEK[HEADER.FIELDS (FROM TO)])"#).unwrap();
        assert_eq!(command.tag, "9.1");
        assert_eq!(command.name, "FETCH");
        assert!(command.uid);
        assert_eq!(
            command.arguments,
            ["1", "(BODY.PEEK[HEADER.FIELDS", "(FROM", "TO)])"]
        );
        let command = parse_command(r#"a LIST "" "Folder with \"quotes\"""#).unwrap();
        assert_eq!(command.arguments, [r#""""#, r#""Folder with \"quotes\"""#]);
        assert_eq!(
            parse_command(r#"b SELECT "unfinished"#).err(),
            Some(Some("b"))
        );
        assert_eq!(parse_command("c UID").err(), Some(Some("c")));
        assert_eq!(parse_command("+ NOOP").err(), Some(None));
    }

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
