mod codec;
mod file;
#[cfg(feature = "secure-transfer")]
pub(crate) mod pki_chat;
mod terminal;

use std::{
    collections::VecDeque,
    ffi::{OsStr, OsString},
    io::{self, IsTerminal, Write},
    net::Ipv4Addr,
    path::Path,
    time::Duration,
};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Instant, interval, timeout},
};

use crate::{
    frame_cli::{Failure, interruption},
    session_cli::{Control, callsign, parse_ports},
};
use terminal::{Terminal, View};

const HELP: &str = "Usage: linkwise chat [--pki FILE [--encrypt | --enroll]] [--secure-config FILE] LOCAL [COMMAND_PORT DATA_PORT]
Direct VARA HF BW500 terminal chat via local linkwise-modemd (ports 8300/8301).
Listening starts off. Scrollback and input history are held in memory.

  /connect CALL      Start a direct call
  /listen on|off     Enable or disable incoming calls while idle
  /disconnect        Drain and end the current call
  /encrypt           Request or accept encryption on the current verified call
  /cert offer [--encrypt]      Send public certificates during enrollment
  /cert inspect                Show fingerprints and validation results
  /cert accept [replace] SIGNING [ENCRYPTION]   Confirm out-of-band fingerprints
  /status            Show listening and session state
  /who               Show who's in the current chat
  /window N          Select window 0–9 (also Alt+0–9)
  /sendfile PATH               Offer a gzip file with SHA-256
  /accept 1 [PATH]             Accept the file and optionally choose a destination
  /reject 1                    Decline the file offer
  /cancel 1                    Stop a file transfer
  /transfers                   Show the current file transfer
  /proof PATH                  Save the last verified message from this peer
  /sendfile --encrypt PATH     Send over a dedicated authenticated session
  /receivefile --encrypt PATH  Accept a dedicated authenticated session
  /help              Show commands and keys
  /quit              Disconnect and exit (also Ctrl-C)

In the connected peer window, Enter sends text. Use // for a leading slash.
Up/Down recall input. Page Up/Down scroll. Backspace edits. Ctrl-U clears.";

const HOST_WAIT: Duration = Duration::from_secs(5);
const LINK_WAIT: Duration = Duration::from_secs(90);
const SEND_WAIT: Duration = Duration::from_secs(120);
const CLOSE_WAIT: Duration = Duration::from_secs(30);
const DATA_TAIL_WAIT: Duration = Duration::from_millis(250);
#[cfg(feature = "secure-transfer")]
const REVOCATION_REFRESH: Duration = Duration::from_secs(30);

pub async fn run(args: &[OsString]) -> Result<(), Failure> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        return writeln!(io::stdout().lock(), "{HELP}").map_err(Failure::operation);
    }
    let mut args = args;
    let mut secure_config = None;
    let mut pki_config = None;
    let mut encrypt = false;
    let mut enroll = false;
    loop {
        match args {
            [flag, path, rest @ ..] if flag == "--secure-config" && secure_config.is_none() => {
                secure_config = Some(path.clone());
                args = rest;
            }
            [flag, path, rest @ ..] if flag == "--pki" && pki_config.is_none() => {
                pki_config = Some(path.clone());
                args = rest;
            }
            [flag, rest @ ..] if flag == "--encrypt" && !encrypt => {
                encrypt = true;
                args = rest;
            }
            [flag, rest @ ..] if flag == "--enroll" && !enroll => {
                enroll = true;
                args = rest;
            }
            _ => break,
        }
    }
    if (secure_config.is_some() || pki_config.is_some()) && !cfg!(feature = "secure-transfer") {
        return Err(Failure::usage(
            "this build does not support PKI or encrypted file transfer",
        ));
    }
    if encrypt && pki_config.is_none() {
        return Err(Failure::usage("--encrypt requires --pki FILE"));
    }
    if enroll && (pki_config.is_none() || encrypt || secure_config.is_some()) {
        return Err(Failure::usage(
            "--enroll requires --pki FILE and runs a certificate-only call",
        ));
    }
    let [local, ports @ ..] = args.strip_prefix(&[OsString::from("--")]).unwrap_or(args) else {
        return Err(Failure::usage(
            "use linkwise chat LOCAL [COMMAND_PORT DATA_PORT]",
        ));
    };
    let local = callsign(local)?;
    let (command_port, data_port) = parse_ports(ports)?;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Failure::usage("chat requires an interactive terminal"));
    }

    tokio::select! {
        biased;
        event = linkwise::signals::stop() => Err(interruption(event, "chat interrupted; delivery may be unconfirmed")),
        result = chat(&local, (command_port, data_port), secure_config, pki_config, encrypt, enroll) => result.map_err(Failure::operation),
    }
}

enum Link {
    Idle,
    Dialing(String, Instant),
    Connected(usize),
    Closing {
        window: Option<usize>,
        deadline: Option<Instant>,
    },
    ReceivingTail(usize, Instant),
}

impl Link {
    fn window(&self) -> Option<usize> {
        match self {
            Self::Connected(index) | Self::ReceivingTail(index, _) => Some(*index),
            Self::Closing { window, .. } => *window,
            _ => None,
        }
    }
}

#[derive(Default)]
enum Departure {
    #[default]
    Disconnected,
    Left,
    TimedOut(&'static str),
}

struct Chat {
    link: Link,
    peer_present: bool,
    departure: Departure,
    listening: bool,
    listen_change: Option<(bool, Instant)>,
    pending: Option<(usize, usize, Instant)>,
    pending_file: bool,
    files: file::Transfer,
    quitting: bool,
    view: View,
    decoder: codec::Decoder,
    early_data: Vec<u8>,
    history: VecDeque<String>,
    recall: Option<usize>,
    #[cfg(feature = "secure-transfer")]
    secure_config: Option<OsString>,
    #[cfg(feature = "secure-transfer")]
    secure_job: Option<crate::secure_cli::ChatTransfer>,
    #[cfg(feature = "secure-transfer")]
    pki: Option<pki_chat::Session>,
    #[cfg(feature = "secure-transfer")]
    pki_deadline: Option<Instant>,
    #[cfg(feature = "secure-transfer")]
    pki_recheck_at: Option<Instant>,
    #[cfg(feature = "secure-transfer")]
    pki_rejected: bool,
}

impl Chat {
    fn new() -> Self {
        Self {
            link: Link::Idle,
            peer_present: false,
            departure: Departure::default(),
            listening: false,
            listen_change: None,
            pending: None,
            pending_file: false,
            files: file::Transfer::default(),
            quitting: false,
            view: View::new(),
            decoder: codec::Decoder::default(),
            early_data: Vec::new(),
            history: VecDeque::new(),
            recall: None,
            #[cfg(feature = "secure-transfer")]
            secure_config: None,
            #[cfg(feature = "secure-transfer")]
            secure_job: None,
            #[cfg(feature = "secure-transfer")]
            pki: None,
            #[cfg(feature = "secure-transfer")]
            pki_deadline: None,
            #[cfg(feature = "secure-transfer")]
            pki_recheck_at: None,
            #[cfg(feature = "secure-transfer")]
            pki_rejected: false,
        }
    }

    fn status(&self) -> String {
        let listening = if self.listening {
            "listen on"
        } else {
            "listen off"
        };
        let link = match &self.link {
            Link::Idle => "idle".into(),
            Link::Dialing(peer, _) => format!("calling {peer}"),
            Link::Connected(index) => format!("connected {}", self.view.windows[*index].peer),
            Link::Closing { deadline: None, .. } | Link::ReceivingTail(..) => "draining".into(),
            Link::Closing { .. } => "disconnecting".into(),
        };
        let pending = if self.pending.is_some() {
            " | TX pending"
        } else {
            ""
        };
        let file = if self.files.busy() { " | file 1" } else { "" };
        let status = format!("{link} | {listening}{pending}{file}");
        #[cfg(feature = "secure-transfer")]
        if let Some(pki) = &self.pki {
            let protection = if self.pki_rejected {
                "PKI failed"
            } else {
                pki.status()
            };
            return format!("{status} | {protection}");
        }
        status
    }

    fn presence_notice(&mut self, index: usize, message: &str) {
        let time = chrono::Local::now().format("%H:%M");
        let line = format!("{time}  {message}");
        if index != 0 {
            self.view.log(index, line.clone());
        }
        self.view.log(0, line);
    }

    fn peer_joined(&mut self, index: usize) {
        if !self.peer_present && matches!(self.link, Link::Connected(active) if active == index) {
            self.peer_present = true;
            let peer = &self.view.windows[index].peer;
            let label = if self.signed_chat() {
                " (verified)"
            } else {
                ""
            };
            self.presence_notice(index, &format!("{peer} joined the chat{label}"));
        }
    }

    fn who(&mut self, local: &str, index: usize) {
        self.view.log(index, format!("You: {local}"));
        if let Link::Connected(active) = self.link
            && (index == 0 || index == active)
            && !self.verification_rejected()
        {
            let peer = &self.view.windows[active].peer;
            let label = if self.peer_present {
                #[cfg(feature = "secure-transfer")]
                let protected = self.pki.as_ref().map(|pki| pki.status());
                #[cfg(not(feature = "secure-transfer"))]
                let protected: Option<&str> = None;
                protected.unwrap_or("connected")
            } else {
                "verification pending"
            };
            self.view.log(index, format!("Peer: {peer} ({label})"));
        } else {
            self.view.log(index, "No peer connected.");
        }
    }

    fn verification_rejected(&self) -> bool {
        #[cfg(feature = "secure-transfer")]
        return self.pki_rejected;
        #[cfg(not(feature = "secure-transfer"))]
        false
    }

    fn expired_call(&mut self, now: Instant) -> bool {
        let Link::Dialing(peer, deadline) = &self.link else {
            return false;
        };
        if now < *deadline {
            return false;
        }
        self.presence_notice(0, &format!("{peer} timed out (call unanswered)"));
        true
    }

    fn transmit_timed_out(&self, now: Instant) -> bool {
        self.link.window().is_some()
            && !matches!(self.link, Link::ReceivingTail(..))
            && !matches!(self.departure, Departure::TimedOut(_))
            && self.pending.is_some_and(|(_, _, deadline)| now >= deadline)
    }

    fn notice(&mut self, local: &str, line: &str) -> io::Result<()> {
        if let Some(rest) = line.strip_prefix("CONNECTED ") {
            if matches!(self.link, Link::ReceivingTail(..)) {
                self.finish_disconnect();
            }
            let parts: Vec<_> = rest.split_ascii_whitespace().collect();
            let [caller, called, "500"] = parts.as_slice() else {
                return Err(io::Error::other("unexpected modem connection notice"));
            };
            let peer = match &self.link {
                Link::Idle
                    if (self.listening || self.listen_change.is_some_and(|(on, _)| on))
                        && *called == local =>
                {
                    *caller
                }
                Link::Dialing(peer, _) if *caller == local && *called == peer => *called,
                Link::Closing { .. } => return Ok(()),
                _ => return Err(io::Error::other("modem connected to an unexpected station")),
            };
            let peer =
                callsign(OsStr::new(peer)).map_err(|_| io::Error::other("invalid modem peer"))?;
            let index = self.view.peer(&peer);
            self.view.select(index);
            self.view.log(index, "Connected at BW500");
            self.link = Link::Connected(index);
            #[cfg(feature = "secure-transfer")]
            if let Some(pki) = &mut self.pki {
                if let Err(error) = pki.start(&peer, *caller == local) {
                    self.reject_pki(index, error);
                    return Ok(());
                }
                self.pki_deadline = Some(Instant::now() + LINK_WAIT);
                self.pki_recheck_at = Some(Instant::now() + REVOCATION_REFRESH);
            }
            if !self.signed_chat() {
                self.peer_joined(index);
            }
            let early = std::mem::take(&mut self.early_data);
            self.inbound(&early)?;
        } else if line == "DISCONNECTED" {
            if let Some(index) = self.link.window() {
                // DATA and command notices are written by separate socket tasks.
                // Keep routing the tail to this peer while the DATA socket drains.
                self.link = Link::ReceivingTail(index, Instant::now() + DATA_TAIL_WAIT);
            } else {
                self.finish_disconnect();
            }
        } else if line == "OK" {
            if let Some((value, _)) = self.listen_change.take() {
                self.listening = value;
                if !value {
                    self.early_data.clear();
                }
                self.view.log(
                    0,
                    if value {
                        "Listening enabled"
                    } else {
                        "Listening disabled"
                    },
                );
            }
        } else if line == "WRONG" {
            if self.listen_change.take().is_some() {
                self.view.log(0, "Modem rejected listen change");
            } else if matches!(self.link, Link::Dialing(..)) {
                self.view.log(0, "Modem rejected call");
                self.link = Link::Idle;
                self.early_data.clear();
            } else {
                return Err(io::Error::other("modem rejected chat command"));
            }
        } else if let Some(value) = line.strip_prefix("BUFFER ")
            && let Some((remaining, previous, deadline)) = &mut self.pending
        {
            let current: usize = value.parse().map_err(io::Error::other)?;
            let acknowledged = previous.saturating_sub(current);
            if acknowledged > 0 {
                *remaining = remaining.saturating_sub(acknowledged);
                *deadline = Instant::now() + SEND_WAIT;
                if self.pending_file {
                    self.files.progress();
                }
            }
            *previous = current;
            if current == 0 && *remaining == 0 {
                self.pending = None;
                if std::mem::take(&mut self.pending_file) {
                    self.files.acknowledged();
                } else {
                    self.view.log(
                        self.link.window().unwrap_or(0),
                        "Modem acknowledged queued text",
                    );
                }
            }
        }
        Ok(())
    }

    fn finish_disconnect(&mut self) {
        let index = self.link.window().unwrap_or(0);
        #[cfg(feature = "secure-transfer")]
        if !self.pki_rejected && self.pki.as_ref().is_some_and(|pki| pki.upgrading()) {
            self.reject_pki(
                index,
                pki_chat::Failure::Encryption(io::Error::other(
                    "call ended before the encryption upgrade completed",
                )),
            );
        }
        let peer = &self.view.windows[index].peer;
        let event = match std::mem::take(&mut self.departure) {
            Departure::TimedOut(reason) => Some(format!("{peer} timed out ({reason})")),
            Departure::Left if self.peer_present => Some(format!("{peer} left the chat")),
            Departure::Disconnected if self.peer_present => Some(format!("{peer} disconnected")),
            _ => None,
        };
        self.peer_present = false;
        let delivery_unconfirmed = self.pending.take().is_some();
        if let Some(event) = event {
            self.presence_notice(index, &event);
        } else if !matches!(self.link, Link::Idle) {
            self.view.log(index, "Disconnected");
        }
        if delivery_unconfirmed {
            self.view
                .log(index, "Delivery unconfirmed. Nothing will be resent.");
        }
        if self.files.busy() {
            self.view.log(
                index,
                "File transfer ended on disconnect. Delivery may be unconfirmed.",
            );
        }
        self.files = file::Transfer::default();
        self.pending_file = false;
        self.link = Link::Idle;
        self.decoder = codec::Decoder::default();
        self.early_data.clear();
        #[cfg(feature = "secure-transfer")]
        if let Some(pki) = &mut self.pki {
            pki.reset();
            self.pki_deadline = None;
            self.pki_recheck_at = None;
            self.pki_rejected = false;
        }
    }

    #[cfg(feature = "secure-transfer")]
    fn reject_pki(&mut self, index: usize, error: pki_chat::Failure) {
        self.view
            .log(index, error.format(&self.view.windows[index].peer));
        self.pki_deadline = None;
        self.pki_recheck_at = None;
        self.pki_rejected = true;
    }

    fn inbound(&mut self, bytes: &[u8]) -> io::Result<()> {
        #[cfg(feature = "secure-transfer")]
        if self.pki_rejected {
            return Ok(());
        }
        if let Some(index) = self.link.window() {
            if !bytes.is_empty() {
                self.files.received_bytes();
            }
            let peer = self.view.windows[index].peer.clone();
            #[cfg(feature = "secure-transfer")]
            if let Some(pki) = &mut self.pki {
                let mut error = None;
                self.decoder.push(bytes, |record| {
                    if error.is_none() {
                        match pki.receive(record) {
                            Ok(messages) => {
                                for message in messages {
                                    self.view.log(index, message);
                                }
                            }
                            Err(failure) => error = Some(failure),
                        }
                    }
                })?;
                if let Some(error) = error {
                    self.reject_pki(index, error);
                    return Ok(());
                }
                let verified = pki.verified();
                if pki.upgrading() {
                    self.pki_deadline
                        .get_or_insert_with(|| Instant::now() + LINK_WAIT);
                } else if verified || pki.enrollment_ready() {
                    self.pki_deadline = None;
                }
                if verified {
                    self.peer_joined(index);
                }
                if let Link::ReceivingTail(_, deadline) = &mut self.link
                    && !bytes.is_empty()
                {
                    *deadline = Instant::now() + DATA_TAIL_WAIT;
                }
                return Ok(());
            }
            self.decoder.push(bytes, |record| {
                if let Some(result) = self.files.receive(record) {
                    self.view.log(
                        index,
                        result.unwrap_or_else(|error| format!("File transfer: {error}")),
                    );
                } else if record != b"&IT&" {
                    self.view.log(
                        index,
                        terminal::format_message(
                            &peer,
                            terminal::MessageProtection::Plain,
                            &codec::display(record),
                        ),
                    );
                }
            })?;
            if let Link::ReceivingTail(_, deadline) = &mut self.link
                && !bytes.is_empty()
            {
                *deadline = Instant::now() + DATA_TAIL_WAIT;
            }
        } else if matches!(self.link, Link::Dialing(..))
            || self.listening
            || self.listen_change.is_some_and(|(on, _)| on)
        {
            if self.early_data.len() + bytes.len() > 4096 {
                return Err(io::Error::other(
                    "DATA arrived without a matching connection notice",
                ));
            }
            self.early_data.extend_from_slice(bytes);
        }
        Ok(())
    }

    async fn close(&mut self, control: &mut Control) -> io::Result<()> {
        if matches!(self.link, Link::Connected(_)) && self.files.busy() {
            return self.abort_file(control).await;
        }
        let (command, window) = match self.link {
            Link::Idle | Link::Closing { .. } | Link::ReceivingTail(..) => return Ok(()),
            Link::Dialing(..) => ("ABORT", None),
            Link::Connected(index) => ("DISCONNECT", Some(index)),
        };
        let deadline = if self.pending.is_none() {
            request(control, command).await?;
            Some(Instant::now() + CLOSE_WAIT)
        } else {
            None
        };
        if matches!(self.link, Link::Connected(_)) {
            self.departure = Departure::Left;
        }
        self.link = Link::Closing { window, deadline };
        Ok(())
    }

    async fn abort_file(&mut self, control: &mut Control) -> io::Result<()> {
        request(control, "ABORT").await?;
        let window = self.link.window();
        self.view.log(
            window.unwrap_or(0),
            "File transfer cancelled. Delivery may be unconfirmed.",
        );
        self.files = file::Transfer::default();
        self.link = Link::Closing {
            window,
            deadline: Some(Instant::now() + CLOSE_WAIT),
        };
        Ok(())
    }

    async fn pump_file(&mut self, data: &mut tokio::net::tcp::OwnedWriteHalf) -> io::Result<bool> {
        if self.pending.is_some() || !matches!(self.link, Link::Connected(_)) {
            return Ok(false);
        }
        #[cfg(feature = "secure-transfer")]
        if let Some(pki) = &mut self.pki {
            let record = match pki.next() {
                Ok(Some(record)) => record,
                Ok(None) => return Ok(false),
                Err(error) => {
                    self.reject_pki(self.link.window().unwrap_or(0), error);
                    return Ok(true);
                }
            };
            timeout(HOST_WAIT, data.write_all(&record)).await??;
            self.pending = Some((record.len(), 0, Instant::now() + SEND_WAIT));
            self.pending_file = true;
            return Ok(true);
        }
        let Some(chunk) = self.files.next_chunk() else {
            return Ok(false);
        };
        timeout(HOST_WAIT, data.write_all(chunk)).await??;
        self.pending = Some((chunk.len(), 0, Instant::now() + SEND_WAIT));
        self.pending_file = true;
        Ok(true)
    }

    async fn submit(
        &mut self,
        local: &str,
        control: &mut Control,
        data: &mut tokio::net::tcp::OwnedWriteHalf,
        ports: (u16, u16),
    ) -> io::Result<()> {
        let index = self.view.selected;
        let line = self.view.windows[index].draft.clone();
        if line.trim().is_empty() {
            return Ok(());
        }
        let (command, arguments) = line
            .split_once(|ch: char| ch.is_ascii_whitespace())
            .map_or((line.as_str(), ""), |(command, arguments)| {
                (command, arguments.trim())
            });
        let (option, value) = arguments
            .split_once(|ch: char| ch.is_ascii_whitespace())
            .map_or((arguments, ""), |(option, value)| (option, value.trim()));
        let encrypted = match (command, option) {
            ("/sendfile", "--encrypt") => Some((true, value)),
            ("/receivefile", "--encrypt") => Some((false, value)),
            _ => None,
        };
        if let Some((send, path)) = encrypted {
            self.encrypted_file(local, ports, control, index, send, path)
                .await?;
        } else if command == "/proof" {
            #[cfg(feature = "secure-transfer")]
            let result = self
                .pki
                .as_ref()
                .ok_or_else(|| io::Error::other("start chat with --pki"))
                .and_then(|pki| {
                    pki.export_proof(&self.view.windows[index].peer, Path::new(arguments))
                });
            #[cfg(not(feature = "secure-transfer"))]
            let result: io::Result<()> =
                Err(io::Error::other("PKI support is unavailable in this build"));
            let status = match result {
                Ok(()) => format!("Saved signed message to {arguments}"),
                Err(error) => format!("Proof export failed: {error}"),
            };
            self.view.log(index, status);
        } else if command == "/sendfile" {
            let path = arguments;
            let result = if self.signed_chat() {
                Err(io::Error::other(
                    "signed chat accepts only signed messages. Use a dedicated encrypted file session for files.",
                ))
            } else if path.is_empty() || path.starts_with("--") {
                Err(io::Error::other(
                    "use /sendfile PATH or /sendfile --encrypt PATH",
                ))
            } else if matches!(self.link, Link::Connected(active) if index == active) {
                self.files.offer(Path::new(path))
            } else {
                Err(io::Error::other("select the connected peer to send a file"))
            };
            self.view.log(
                index,
                result.unwrap_or_else(|error| format!("File offer failed: {error}")),
            );
        } else if line == "/accept 1" || line.starts_with("/accept 1 ") {
            let path = line["/accept 1".len()..].trim();
            let result = if matches!(self.link, Link::Connected(active) if index == active) {
                self.files
                    .accept((!path.is_empty()).then(|| Path::new(path)))
            } else {
                Err(io::Error::other(
                    "select the connected peer to accept a file",
                ))
            };
            self.view.log(
                index,
                result.unwrap_or_else(|error| format!("File acceptance failed: {error}")),
            );
        } else if line.starts_with('/') && !line.starts_with("//") {
            let args: Vec<_> = line.split_ascii_whitespace().collect();
            match args.as_slice() {
                ["/help"] => {
                    for line in HELP.lines().skip(4) {
                        self.view.log(index, line);
                    }
                }
                ["/status"] => self.view.log(index, self.status()),
                ["/who"] => self.who(local, index),
                ["/encrypt"] => self.request_chat_encryption(index),
                ["/cert", arguments @ ..] => self.certificate_command(index, arguments),
                ["/transfers"] => {
                    let status = self.files.status();
                    let status = match self.decoder.partial_file_body() {
                        Some((received, length)) => {
                            format!("{status} | {received} / {length} Base64 bytes received")
                        }
                        None => status,
                    };
                    self.view.log(index, status);
                }
                ["/reject", "1"] if matches!(self.link, Link::Connected(active) if active == index) =>
                {
                    let result = self.files.reject();
                    self.view
                        .log(index, result.unwrap_or_else(|error| error.to_string()));
                }
                ["/cancel", "1"]
                    if self.files.busy()
                        && matches!(self.link, Link::Connected(active) if active == index) =>
                {
                    if let Some(message) = self.files.cancel_at_record_boundary() {
                        self.view.log(index, message);
                    } else {
                        self.abort_file(control).await?;
                    }
                }
                ["/window", number] => {
                    if let Ok(window) = number.parse::<usize>()
                        && window < self.view.windows.len()
                    {
                        self.view.select(window);
                    } else {
                        self.view.log(index, "No such window");
                    }
                }
                ["/connect", peer]
                    if matches!(self.link, Link::Idle) && self.listen_change.is_none() =>
                {
                    match callsign(OsStr::new(peer)) {
                        Ok(peer) if peer != local => {
                            request(control, &format!("CONNECT {local} {peer}")).await?;
                            self.view.log(index, format!("Calling {peer}"));
                            self.link = Link::Dialing(peer, Instant::now() + LINK_WAIT);
                        }
                        _ => self.view.log(index, "Enter a valid remote callsign"),
                    }
                }
                ["/listen", value @ ("on" | "off")]
                    if matches!(self.link, Link::Idle) && self.listen_change.is_none() =>
                {
                    let on = *value == "on";
                    request(control, if on { "LISTEN ON" } else { "LISTEN OFF" }).await?;
                    self.listen_change = Some((on, Instant::now() + HOST_WAIT));
                }
                ["/disconnect"] => self.close(control).await?,
                ["/quit"] => {
                    self.quitting = true;
                    self.close(control).await?;
                }
                ["/connect", _] | ["/listen", _] => self.view.log(
                    index,
                    "Call and listen changes require an idle modem and valid arguments",
                ),
                _ => self
                    .view
                    .log(index, "Unknown command or invalid arguments. See /help."),
            }
        } else {
            if !matches!(self.link, Link::Connected(active) if active == index)
                || self.pending.is_some()
                || self.files.busy()
            {
                self.view.log(index, "Text kept as draft. Select the connected peer and wait for the transmit queue.");
                return Ok(());
            }
            let text = if line.starts_with("//") {
                &line[1..]
            } else {
                &line
            };
            #[cfg(feature = "secure-transfer")]
            let signed = if let Some(pki) = &mut self.pki {
                if let Err(error) = pki.send_text(text) {
                    self.view.log(index, error.format(local));
                    return Ok(());
                }
                self.view.log(index, pki.format_message(local, text));
                true
            } else {
                false
            };
            #[cfg(not(feature = "secure-transfer"))]
            let signed = false;
            if !signed {
                match codec::encode(text) {
                    Ok(bytes) => {
                        timeout(HOST_WAIT, data.write_all(&bytes)).await??;
                        self.pending = Some((bytes.len(), 0, Instant::now() + SEND_WAIT));
                        self.view.log(
                            index,
                            terminal::format_message(
                                local,
                                terminal::MessageProtection::Plain,
                                text,
                            ),
                        );
                    }
                    Err(error) => {
                        self.view.log(index, error.to_string());
                        return Ok(());
                    }
                }
            }
        }
        self.view.windows[index].draft.clear();
        if self.history.len() == 100 {
            self.history.pop_front();
        }
        self.history.push_back(line);
        self.recall = None;
        Ok(())
    }

    async fn encrypted_file(
        &mut self,
        _local: &str,
        _ports: (u16, u16),
        _control: &mut Control,
        index: usize,
        _send: bool,
        _path: &str,
    ) -> io::Result<()> {
        #[cfg(not(feature = "secure-transfer"))]
        self.view.log(
            index,
            "This build does not support encrypted file transfer.",
        );
        #[cfg(feature = "secure-transfer")]
        {
            let Some(config) = &self.secure_config else {
                self.view.log(
                    index,
                    "Start chat with --secure-config FILE to use encryption.",
                );
                return Ok(());
            };
            if _path.is_empty()
                || _path == "-"
                || self.secure_job.is_some()
                || self.listen_change.is_some()
                || self.files.busy()
                || !matches!(self.link, Link::Idle | Link::Connected(_))
            {
                self.view.log(
                    index,
                    "Use a file path when the modem is idle or connected.",
                );
                return Ok(());
            }
            let peer = self
                .link
                .window()
                .map(|window| self.view.windows[window].peer.as_str());
            match crate::secure_cli::ChatTransfer::prepare(
                _send,
                config,
                _path.into(),
                _local,
                peer,
                _ports,
            ) {
                Ok(job) => {
                    self.close(_control).await?;
                    self.secure_job = Some(job);
                    self.view
                        .log(index, "Handing off to the encrypted session.");
                }
                Err(error) => self
                    .view
                    .log(index, format!("Encrypted transfer setup failed: {error}")),
            }
        }
        Ok(())
    }

    fn signed_chat(&self) -> bool {
        #[cfg(feature = "secure-transfer")]
        return self.pki.is_some();
        #[cfg(not(feature = "secure-transfer"))]
        false
    }

    fn certificate_command(&mut self, index: usize, _arguments: &[&str]) {
        #[cfg(feature = "secure-transfer")]
        {
            let Some(pki) = &mut self.pki else {
                self.view
                    .log(index, "Start both clients with --pki FILE --enroll.");
                return;
            };
            if !matches!(self.link, Link::Connected(active) if active == index) {
                self.view
                    .log(index, "Select the connected peer to use /cert.");
                return;
            }
            let result = match _arguments {
                ["offer"] | ["offer", "--encrypt"] => pki
                    .offer_certificate(_arguments.len() == 2)
                    .map(|()| vec!["Public certificate offer queued.".into()]),
                ["inspect"] => pki.inspect_certificate(),
                ["accept", fingerprints @ ..] => {
                    let (replace, fingerprints) = match fingerprints {
                        ["replace", rest @ ..] => (true, rest),
                        _ => (false, fingerprints),
                    };
                    pki.accept_certificate(replace, fingerprints).map(|()| {
                        vec![
                            "Certificates saved to the shared PKI profile.".into(),
                            "Reconnect with --pki to verify the peer in chat.".into(),
                        ]
                    })
                }
                _ => Err(io::Error::other(
                    "use /cert offer [--encrypt], /cert inspect, or /cert accept [replace] SIGNING [ENCRYPTION]",
                )),
            };
            match result {
                Ok(lines) => {
                    for line in lines {
                        self.view.log(index, line);
                    }
                }
                Err(error) => self
                    .view
                    .log(index, format!("Certificate command failed: {error}")),
            }
        }
        #[cfg(not(feature = "secure-transfer"))]
        self.view.log(
            index,
            "Certificate enrollment requires a PKI-enabled build.",
        );
    }

    fn request_chat_encryption(&mut self, index: usize) {
        if !matches!(self.link, Link::Connected(active) if active == index) {
            self.view
                .log(index, "Select the connected peer to use /encrypt.");
            return;
        }
        #[cfg(feature = "secure-transfer")]
        if let Some(pki) = &mut self.pki {
            match pki.request_encryption() {
                Ok(message) => {
                    self.pki_deadline
                        .get_or_insert_with(|| Instant::now() + LINK_WAIT);
                    self.view.log(index, message);
                }
                Err(error @ pki_chat::Failure::Input(_)) => {
                    self.view
                        .log(index, error.format(&self.view.windows[index].peer));
                }
                Err(error) => self.reject_pki(index, error),
            }
            return;
        }
        self.view
            .log(index, "Start chat with --pki to use /encrypt.");
    }

    fn detach_when_idle(&self) -> bool {
        let detach = self.quitting;
        #[cfg(feature = "secure-transfer")]
        let detach = detach || self.secure_job.is_some();
        detach && matches!(self.link, Link::Idle)
    }
}

async fn request(control: &mut Control, command: &str) -> io::Result<()> {
    timeout(HOST_WAIT, control.request(command)).await?
}

async fn chat(
    local: &str,
    ports: (u16, u16),
    _secure_config: Option<OsString>,
    _pki_config: Option<OsString>,
    _encrypt: bool,
    _enroll: bool,
) -> io::Result<()> {
    let (command_port, data_port) = ports;
    let (control, data) = timeout(HOST_WAIT, async {
        tokio::try_join!(
            TcpStream::connect((Ipv4Addr::LOCALHOST, command_port)),
            TcpStream::connect((Ipv4Addr::LOCALHOST, data_port)),
        )
    })
    .await?
    .map_err(|error| io::Error::other(format!("cannot attach to local modem: {error}")))?;
    let mut control = Control::new(control);
    let mut state = Chat::new();
    #[cfg(feature = "secure-transfer")]
    {
        state.secure_config = _secure_config;
        if let Some(path) = _pki_config {
            let pki = crate::pki::Pki::load(Path::new(&path))?;
            if pki.callsign() != local {
                return Err(io::Error::other(
                    "chat callsign does not match shared PKI configuration",
                ));
            }
            state.pki = Some(pki_chat::Session::new(pki, _encrypt, _enroll));
        }
        let mut data = data;
        while !state.quitting {
            data = attached(local, ports, &mut state, &mut control, data).await?;
            let Some(job) = state.secure_job.take() else {
                return Ok(());
            };
            if state.quitting {
                return Ok(());
            }
            println!("Linkwise encrypted file transfer in progress.");
            let status = match job.run(&mut control, &mut data).await {
                Ok(()) => "Encrypted file transfer complete.".into(),
                Err(error) => format!("Encrypted file transfer failed: {error}"),
            };
            state.view.log(state.view.selected, status);
            state.listening = false;
        }
        Ok(())
    }
    #[cfg(not(feature = "secure-transfer"))]
    attached(local, ports, &mut state, &mut control, data)
        .await
        .map(|_| ())
}

async fn attached(
    local: &str,
    ports: (u16, u16),
    chat: &mut Chat,
    control: &mut Control,
    data: TcpStream,
) -> io::Result<TcpStream> {
    timeout(HOST_WAIT, async {
        for command in [format!("MYCALL {local}"), "P2P SESSION".into(), "BW500".into(), "LISTEN OFF".into()] {
            control.command(&command).await?;
        }
        Ok::<_, io::Error>(())
    }).await.map_err(|_| io::Error::other("modem attachment timed out; check if it is occupied or awaiting a waveform peer"))?
        .map_err(|error| io::Error::other(format!("modem setup failed; check callsign, BW500 mode, and whether another client owns it: {error}")))?;

    let _terminal = Terminal::enter()?;
    let (mut read, mut write) = data.into_split();
    let mut buffer = [0; 4096];
    let mut tick = interval(Duration::from_millis(25));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    chat.view.log(
        0,
        "Attached to Linkwise modem. Use /connect CALL or /listen on.",
    );
    chat.view.draw(local, &chat.status())?;
    loop {
        let redraw = tokio::select! {
            biased;
            line = control.line() => {
                let line = line.map_err(|error| {
                    if chat.pending.is_some() {
                        io::Error::other(format!("{error}; delivery unconfirmed"))
                    } else {
                        error
                    }
                })?;
                chat.notice(local, &line)?;
                true
            }
            size = read.read(&mut buffer) => {
                let size = size?;
                if size == 0 {
                    return Err(io::Error::other("modem DATA port closed; delivery may be unconfirmed"));
                }
                chat.inbound(&buffer[..size])?;
                true
            }
            _ = tick.tick() => {
                let now = Instant::now();
                let mut redraw = false;
                #[cfg(feature = "secure-transfer")]
                if chat.pki_recheck_at.is_some_and(|deadline| now >= deadline)
                    && let Link::Connected(index) = chat.link
                    && let Some(pki) = &mut chat.pki
                {
                    chat.pki_recheck_at = Some(now + REVOCATION_REFRESH);
                    if let Err(error) = pki.refresh_revocation() {
                        chat.reject_pki(index, error);
                        redraw = true;
                    }
                }
                #[cfg(feature = "secure-transfer")]
                if chat.pki_deadline.is_some_and(|deadline| now >= deadline)
                    && let Some(index) = chat.link.window()
                {
                    let upgrading = chat.pki.as_ref().is_some_and(|pki| pki.upgrading());
                    let (reason, error) = if upgrading {
                        ("encryption upgrade", pki_chat::Failure::Encryption(io::Error::other("encryption upgrade timed out")))
                    } else {
                        ("peer verification", io::Error::other("peer authentication timed out").into())
                    };
                    chat.departure = Departure::TimedOut(reason);
                    chat.reject_pki(index, error);
                    redraw = true;
                }
                if chat.expired_call(now) {
                    chat.close(control).await?;
                    redraw = true;
                }
                if matches!(chat.link, Link::ReceivingTail(_, deadline) if now >= deadline) {
                    chat.finish_disconnect();
                    redraw = true;
                }
                if matches!(chat.link, Link::Closing { deadline: Some(deadline), .. } if now >= deadline) {
                    let _ = request(control, "ABORT").await;
                    return Err(io::Error::other("disconnect timed out; delivery may be unconfirmed"));
                }
                if let Link::Closing { ref mut deadline, .. } = chat.link
                    && deadline.is_none() && chat.pending.is_none()
                {
                    request(control, "DISCONNECT").await?;
                    *deadline = Some(Instant::now() + CLOSE_WAIT);
                    redraw = true;
                }
                if chat.listen_change.is_some_and(|(_, deadline)| now >= deadline) {
                    return Err(io::Error::other("modem did not acknowledge listen change"));
                }
                if chat.transmit_timed_out(now) {
                    chat.departure = Departure::TimedOut("no transmit acknowledgments");
                    request(control, "ABORT").await?;
                    chat.link = Link::Closing {
                        window: chat.link.window(),
                        deadline: Some(Instant::now() + CLOSE_WAIT),
                    };
                    redraw = true;
                }
                if chat.files.expired(now) && matches!(chat.link, Link::Connected(_)) {
                    chat.view.log(chat.link.window().unwrap_or(0), "File transfer timed out.");
                    chat.abort_file(control).await?;
                    redraw = true;
                }
                if event::poll(Duration::ZERO)? {
                    match event::read()? {
                        Event::Key(key) if key.kind != KeyEventKind::Release => {
                            let window = &mut chat.view.windows[chat.view.selected];
                            match key.code {
                                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                    chat.quitting = true;
                                    chat.close(control).await?;
                                }
                                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                    window.draft.clear();
                                    chat.recall = None;
                                }
                                KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::ALT) => {
                                    if let Some(index) = ch.to_digit(10) {
                                        chat.view.select(index as usize);
                                        chat.recall = None;
                                    }
                                }
                                KeyCode::Char(ch) if !ch.is_control() && !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                    if window.draft.len() + ch.len_utf8() <= 4 * codec::MAX_TEXT {
                                        window.draft.push(ch);
                                        chat.recall = None;
                                    }
                                }
                                KeyCode::Backspace => { window.draft.pop(); chat.recall = None; }
                                KeyCode::PageUp => chat.view.page(true)?,
                                KeyCode::PageDown => chat.view.page(false)?,
                                KeyCode::Up if !chat.history.is_empty() => {
                                    let index = chat.recall.unwrap_or(chat.history.len()).saturating_sub(1);
                                    window.draft.clone_from(&chat.history[index]);
                                    chat.recall = Some(index);
                                }
                                KeyCode::Down if chat.recall.is_some() => {
                                    let index = chat.recall.unwrap() + 1;
                                    if let Some(value) = chat.history.get(index) {
                                        window.draft.clone_from(value);
                                        chat.recall = Some(index);
                                    } else {
                                        window.draft.clear();
                                        chat.recall = None;
                                    }
                                }
                                KeyCode::Enter if !chat.quitting => chat.submit(local, control, &mut write, ports).await?,
                                _ => {}
                            }
                            true
                        }
                        Event::Resize(..) => true,
                        _ => redraw,
                    }
                } else {
                    redraw
                }
            }
        };
        if chat.detach_when_idle() {
            return read.reunite(write).map_err(io::Error::other);
        }
        #[cfg(feature = "secure-transfer")]
        if chat.pki_rejected
            && let Link::Connected(index) = chat.link
        {
            let command = if chat.pending.is_none() {
                "DISCONNECT"
            } else {
                "ABORT"
            };
            request(control, command).await?;
            chat.link = Link::Closing {
                window: Some(index),
                deadline: Some(Instant::now() + CLOSE_WAIT),
            };
        }
        if chat.files.needs_abort() && matches!(chat.link, Link::Connected(_)) {
            chat.abort_file(control).await?;
        }
        let pumped = chat.pump_file(&mut write).await?;
        if redraw || pumped {
            chat.view.draw(local, &chat.status())?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_data_after_disconnect_stays_with_the_old_peer() {
        let mut chat = Chat::new();
        chat.listening = true;
        chat.notice("KN6MQS", "CONNECTED BB2BB KN6MQS 500").unwrap();
        chat.inbound(b"5 he").unwrap();
        chat.notice("KN6MQS", "DISCONNECTED").unwrap();
        assert!(matches!(chat.link, Link::ReceivingTail(1, _)));
        chat.inbound(b"llo").unwrap();
        assert!(chat.early_data.is_empty());
        chat.finish_disconnect();
        chat.notice("KN6MQS", "CONNECTED CC3CC KN6MQS 500").unwrap();
        assert!(matches!(chat.link, Link::Connected(2)));
        chat.inbound(b"5 world").unwrap();
    }

    #[test]
    fn only_acknowledgement_progress_extends_the_send_deadline() {
        let mut chat = Chat::new();
        let initial = Instant::now();
        chat.pending = Some((20, 0, initial));
        chat.notice("KN6MQS", "BUFFER 20").unwrap();
        chat.notice("KN6MQS", "BUFFER 20").unwrap();
        assert_eq!(chat.pending, Some((20, 20, initial)));
        chat.notice("KN6MQS", "BUFFER 10").unwrap();
        let (remaining, queued, resumed) = chat.pending.unwrap();
        assert_eq!((remaining, queued), (10, 10));
        assert!(resumed >= initial + SEND_WAIT);
        chat.notice("KN6MQS", "BUFFER 0").unwrap();
        assert!(chat.pending.is_none());
    }

    #[test]
    fn presence_distinguishes_idle_time_from_transmit_timeout() {
        let mut chat = Chat::new();
        chat.listening = true;
        chat.notice("KN6MQS", "CONNECTED BB2BB KN6MQS 500").unwrap();
        let now = Instant::now();
        assert!(!chat.transmit_timed_out(now + SEND_WAIT));
        chat.pending = Some((10, 10, now));
        assert!(chat.transmit_timed_out(now));
        chat.departure = Departure::TimedOut("no transmit acknowledgments");
        assert!(!chat.transmit_timed_out(now));
        chat.notice("KN6MQS", "DISCONNECTED").unwrap();
        chat.finish_disconnect();
        chat.notice("KN6MQS", "DISCONNECTED").unwrap();

        for index in [0, 1] {
            let lines = chat.view.windows[index].scrollback();
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.ends_with("BB2BB joined the chat"))
                    .count(),
                1
            );
            assert_eq!(
                lines
                    .iter()
                    .filter(|line| line.ends_with("BB2BB timed out (no transmit acknowledgments)"))
                    .count(),
                1
            );
        }
        assert!(!chat.peer_present);
        assert!(
            chat.view.windows[1]
                .scrollback()
                .iter()
                .any(|line| line.starts_with("Delivery unconfirmed."))
        );
    }

    #[test]
    fn unanswered_call_names_the_peer_in_status() {
        let mut chat = Chat::new();
        let deadline = Instant::now() + LINK_WAIT;
        chat.link = Link::Dialing("BB2BB".into(), deadline);
        assert!(!chat.expired_call(deadline - Duration::from_secs(1)));
        assert!(chat.expired_call(deadline));
        assert!(
            chat.view.windows[0].scrollback()[0].ends_with("BB2BB timed out (call unanswered)")
        );
    }
}
