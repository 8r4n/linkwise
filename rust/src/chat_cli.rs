mod codec;
mod terminal;

use std::{
    collections::VecDeque,
    ffi::{OsStr, OsString},
    io::{self, IsTerminal, Write},
    net::Ipv4Addr,
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

const HELP: &str = "Usage: linkwise chat LOCAL [COMMAND_PORT DATA_PORT]
Direct VARA HF BW500 terminal chat via local linkwise-modemd (ports 8300/8301).
Listening starts off. Scrollback and input history are held in memory.

  /connect CALL      Start a direct call
  /listen on|off     Enable or disable incoming calls while idle
  /disconnect        Drain and end the current call
  /status            Show listening and session state
  /window N          Select window 0–9 (also Alt+0–9)
  /help              Show commands and keys
  /quit              Disconnect and exit (also Ctrl-C)

In the connected peer window, Enter sends text. Use // for a leading slash.
Up/Down recall input. Page Up/Down scroll. Backspace edits. Ctrl-U clears.";

const HOST_WAIT: Duration = Duration::from_secs(5);
const LINK_WAIT: Duration = Duration::from_secs(90);
const SEND_WAIT: Duration = Duration::from_secs(120);
const CLOSE_WAIT: Duration = Duration::from_secs(30);

pub async fn run(args: &[OsString]) -> Result<(), Failure> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        return writeln!(io::stdout().lock(), "{HELP}").map_err(Failure::operation);
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
        result = chat(&local, command_port, data_port) => result.map_err(Failure::operation),
    }
}

enum Link {
    Idle,
    Dialing(String, Instant),
    Connected(usize),
    Closing {
        window: Option<usize>,
        deadline: Instant,
        drain: bool,
    },
}

impl Link {
    fn window(&self) -> Option<usize> {
        match self {
            Self::Connected(index) => Some(*index),
            Self::Closing { window, .. } => *window,
            _ => None,
        }
    }
}

struct Chat {
    link: Link,
    listening: bool,
    listen_change: Option<(bool, Instant)>,
    pending: Option<(usize, usize, Instant)>,
    quitting: bool,
    view: View,
    decoder: codec::Decoder,
    early_data: Vec<u8>,
    history: VecDeque<String>,
    recall: Option<usize>,
}

impl Chat {
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
            Link::Closing { drain: true, .. } => "draining".into(),
            Link::Closing { .. } => "disconnecting".into(),
        };
        let pending = if self.pending.is_some() {
            " | TX pending"
        } else {
            ""
        };
        format!("{link} | {listening}{pending}")
    }

    fn notice(&mut self, local: &str, line: &str) -> io::Result<()> {
        if let Some(rest) = line.strip_prefix("CONNECTED ") {
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
            let early = std::mem::take(&mut self.early_data);
            self.inbound(&early)?;
        } else if line == "DISCONNECTED" {
            let index = self.link.window().unwrap_or(0);
            let message = if self.pending.take().is_some() {
                "Disconnected. Delivery unconfirmed. Nothing will be resent."
            } else {
                "Disconnected"
            };
            self.view.log(index, message);
            self.link = Link::Idle;
            self.decoder = codec::Decoder::default();
            self.early_data.clear();
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
            && let Some((remaining, previous, _)) = &mut self.pending
        {
            let current: usize = value.parse().map_err(io::Error::other)?;
            *remaining = remaining.saturating_sub(previous.saturating_sub(current));
            *previous = current;
            if current == 0 && *remaining == 0 {
                self.pending = None;
                self.view.log(
                    self.link.window().unwrap_or(0),
                    "Modem acknowledged queued text",
                );
            }
        }
        Ok(())
    }

    fn inbound(&mut self, bytes: &[u8]) -> io::Result<()> {
        if let Some(index) = self.link.window() {
            let peer = self.view.windows[index].peer.clone();
            self.decoder.push(bytes, |record| {
                if record != b"&IT&" {
                    self.view
                        .log(index, format!("<{peer}> {}", codec::display(record)));
                }
            })?;
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
        let (command, window) = match self.link {
            Link::Idle | Link::Closing { .. } => return Ok(()),
            Link::Dialing(..) => ("ABORT", None),
            Link::Connected(index) => ("DISCONNECT", Some(index)),
        };
        let drain = self.pending.is_some();
        if !drain {
            request(control, command).await?;
        }
        self.link = Link::Closing {
            window,
            deadline: Instant::now() + CLOSE_WAIT,
            drain,
        };
        Ok(())
    }

    async fn submit(
        &mut self,
        local: &str,
        control: &mut Control,
        data: &mut tokio::net::tcp::OwnedWriteHalf,
    ) -> io::Result<()> {
        let index = self.view.selected;
        let line = self.view.windows[index].draft.clone();
        if line.trim().is_empty() {
            return Ok(());
        }
        if line.starts_with('/') && !line.starts_with("//") {
            let args: Vec<_> = line.split_ascii_whitespace().collect();
            match args.as_slice() {
                ["/help"] => {
                    for line in HELP.lines().skip(4) {
                        self.view.log(index, line);
                    }
                }
                ["/status"] => self.view.log(index, self.status()),
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
            {
                self.view.log(index, "Text kept as draft. Select the connected peer and wait for the transmit queue.");
                return Ok(());
            }
            let text = if line.starts_with("//") {
                &line[1..]
            } else {
                &line
            };
            match codec::encode(text) {
                Ok(bytes) => {
                    timeout(HOST_WAIT, data.write_all(&bytes)).await??;
                    self.pending = Some((bytes.len(), 0, Instant::now() + SEND_WAIT));
                    self.view.log(index, format!("<{local}> {text}"));
                }
                Err(error) => {
                    self.view.log(index, error.to_string());
                    return Ok(());
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
}

async fn request(control: &mut Control, command: &str) -> io::Result<()> {
    timeout(HOST_WAIT, control.request(command)).await?
}

async fn chat(local: &str, command_port: u16, data_port: u16) -> io::Result<()> {
    let (control, data) = timeout(HOST_WAIT, async {
        tokio::try_join!(
            TcpStream::connect((Ipv4Addr::LOCALHOST, command_port)),
            TcpStream::connect((Ipv4Addr::LOCALHOST, data_port)),
        )
    })
    .await?
    .map_err(|error| io::Error::other(format!("cannot attach to local modem: {error}")))?;
    let mut control = Control::new(control);
    timeout(HOST_WAIT, async {
        for command in [format!("MYCALL {local}"), "P2P SESSION".into(), "BW500".into(), "LISTEN OFF".into()] {
            control.command(&command).await?;
        }
        Ok::<_, io::Error>(())
    }).await.map_err(|_| io::Error::other("modem attachment timed out; check if it is occupied or awaiting a waveform peer"))?
        .map_err(|error| io::Error::other(format!("modem setup failed; check callsign, BW500 mode, and whether another client owns it: {error}")))?;

    let _terminal = Terminal::enter()?;
    let mut chat = Chat {
        link: Link::Idle,
        listening: false,
        listen_change: None,
        pending: None,
        quitting: false,
        view: View::new(),
        decoder: codec::Decoder::default(),
        early_data: Vec::new(),
        history: VecDeque::new(),
        recall: None,
    };
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
                if matches!(chat.link, Link::Dialing(_, deadline) if now >= deadline) {
                    chat.view.log(0, "Call timed out");
                    chat.close(&mut control).await?;
                }
                if matches!(chat.link, Link::Closing { deadline, .. } if now >= deadline) {
                    let _ = request(&mut control, "ABORT").await;
                    return Err(io::Error::other("disconnect timed out; delivery may be unconfirmed"));
                }
                if let Link::Closing { ref mut drain, .. } = chat.link
                    && *drain && chat.pending.is_none()
                {
                    request(&mut control, "DISCONNECT").await?;
                    *drain = false;
                }
                if chat.listen_change.is_some_and(|(_, deadline)| now >= deadline) {
                    return Err(io::Error::other("modem did not acknowledge listen change"));
                }
                if chat.pending.is_some_and(|(_, _, deadline)| now >= deadline) && !matches!(chat.link, Link::Closing { .. }) {
                    chat.view.log(chat.link.window().unwrap_or(0), "Transmit timed out; attempting to disconnect");
                    chat.close(&mut control).await?;
                }
                if event::poll(Duration::ZERO)? {
                    match event::read()? {
                        Event::Key(key) if key.kind != KeyEventKind::Release => {
                            let window = &mut chat.view.windows[chat.view.selected];
                            match key.code {
                                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                    chat.quitting = true;
                                    chat.close(&mut control).await?;
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
                                KeyCode::Enter if !chat.quitting => chat.submit(local, &mut control, &mut write).await?,
                                _ => {}
                            }
                            true
                        }
                        Event::Resize(..) => true,
                        _ => false,
                    }
                } else {
                    false
                }
            }
        };
        if chat.quitting && matches!(chat.link, Link::Idle) {
            return Ok(());
        }
        if redraw {
            chat.view.draw(local, &chat.status())?;
        }
    }
}
