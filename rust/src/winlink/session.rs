//! Peer-to-peer B2 forwarding over an established bidirectional byte stream.

use std::io::{BufRead, Error, ErrorKind, Result, Write};

use super::{
    Message, invalid,
    proposal::{self, Answer, Proposal, ProposalKind},
    transfer,
};

pub(super) const BATCH: usize = 5;
const MAX_LINE: usize = 512;
const MAX_GREETING_LINES: usize = 32;
const MAX_TURNS: usize = 1024;

#[derive(Clone, Copy)]
pub enum Role {
    Caller,
    Listener,
}

#[derive(Clone, Copy)]
pub struct Settings<'a> {
    pub local_call: &'a str,
    pub peer_call: &'a str,
    pub max_message_bytes: usize,
    pub max_compressed_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Delivery {
    Confirmed,
    Rejected,
}

/// Session storage. Successful inbound writes must be durable before returning.
pub trait Mailbox {
    /// Return at most five envelopes for this peer, excluding the given IDs.
    fn outgoing(&mut self, peer: &str, excluded: &[String]) -> Result<Vec<Vec<u8>>>;
    fn contains(&mut self, id: &str) -> Result<bool>;
    fn store(&mut self, id: &str, envelope: &[u8]) -> Result<()>;
    fn finish(&mut self, id: &str, delivery: Delivery) -> Result<()>;
}

/// Run one P2P session. The stream must enforce read and write timeouts.
/// The owner handles connection teardown after return, including on error.
pub fn exchange(
    stream: &mut (impl BufRead + Write),
    role: Role,
    settings: Settings<'_>,
    mailbox: &mut impl Mailbox,
) -> Result<()> {
    for call in [settings.local_call, settings.peer_call] {
        if call.is_empty()
            || call.len() > 12
            || !call
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(invalid("invalid B2F station callsign"));
        }
    }
    let mut incoming = match role {
        Role::Caller => {
            read_greeting(stream, true)?;
            send_greeting(stream, settings, false)?;
            None
        }
        Role::Listener => {
            send_greeting(stream, settings, true)?;
            {
                let first = read_greeting(stream, false)?;
                Some(read_turn(stream, settings, first)?)
            }
        }
    };
    let mut deferred = Vec::new();
    let mut confirmed: Vec<String> = Vec::new();
    let mut remote_empty = false;
    for _ in 0..MAX_TURNS {
        if let Some(turn) = incoming.take() {
            // A complete, valid next turn is the protocol's implicit acknowledgment.
            for id in confirmed.drain(..) {
                mailbox.finish(&id, Delivery::Confirmed)?;
            }
            match turn {
                Turn::Quit => return Ok(()),
                Turn::Empty => remote_empty = true,
                Turn::Proposals(block) => {
                    remote_empty = false;
                    receive(stream, settings, mailbox, &block)?;
                }
            }
        }

        let outgoing = mailbox.outgoing(settings.peer_call, &deferred)?;
        if outgoing.len() > BATCH {
            return Err(invalid("mailbox returned too many B2F messages"));
        }
        if outgoing.is_empty() {
            send(stream, if remote_empty { b"FQ\r" } else { b"FF\r" })?;
            if remote_empty {
                return Ok(());
            }
        } else {
            confirmed = transmit(stream, settings, mailbox, &outgoing, &mut deferred)?;
        }
        incoming = Some(read_turn(stream, settings, None)?);
    }
    Err(invalid("B2F session exceeded turn limit"))
}

enum Turn {
    Quit,
    Empty,
    Proposals(String),
}

fn read_turn(
    stream: &mut impl BufRead,
    settings: Settings<'_>,
    first: Option<String>,
) -> Result<Turn> {
    let mut next = first;
    let mut block = String::new();
    for _ in 0..MAX_GREETING_LINES {
        let line = match next.take() {
            Some(line) => line,
            None => read_line(stream)?,
        };
        if line == "\r" || line.starts_with(';') {
            continue;
        }
        if block.is_empty() {
            match line.as_str() {
                "FF\r" => return Ok(Turn::Empty),
                "FQ\r" => return Ok(Turn::Quit),
                _ => {}
            }
        }
        if line.starts_with("F> ") {
            block.push_str(&line);
            proposal::decode(
                block.as_bytes(),
                settings.max_message_bytes,
                settings.max_compressed_bytes,
            )?;
            return Ok(Turn::Proposals(block));
        }
        if !line.starts_with("FC ") || block.len() + line.len() > MAX_LINE {
            return Err(invalid("unexpected B2F forwarding command"));
        }
        block.push_str(&line);
    }
    Err(invalid("too many lines in B2F forwarding turn"))
}

fn send_greeting(stream: &mut impl Write, settings: Settings<'_>, prompt: bool) -> Result<()> {
    send(
        stream,
        format!(
            ";FW: {}\r[Linkwise-0.1-B2F$]\r; {} DE {}{}\r",
            settings.local_call,
            settings.peer_call,
            settings.local_call,
            if prompt { ">" } else { "" },
        )
        .as_bytes(),
    )
}

fn read_greeting(stream: &mut impl BufRead, prompt: bool) -> Result<Option<String>> {
    let mut has_b2 = false;
    for _ in 0..MAX_GREETING_LINES {
        let line = read_line(stream)?;
        let text = line.trim_end_matches('\r');
        if text.starts_with(";PQ") || text.starts_with("***") {
            return Err(invalid("peer rejected P2P or requires authentication"));
        }
        if let Some(sid) = text
            .strip_prefix('[')
            .and_then(|text| text.strip_suffix(']'))
        {
            has_b2 = sid
                .rsplit_once('-')
                .is_some_and(|(_, features)| features.contains("B2") && features.contains('F'));
            if !has_b2 {
                return Err(invalid("peer does not advertise B2 forwarding"));
            }
        }
        if prompt && text.ends_with('>') {
            return has_b2
                .then_some(None)
                .ok_or_else(|| invalid("missing B2 station ID"));
        }
        if !prompt && (text.starts_with("FC ") || matches!(text, "FF" | "FQ")) {
            if !has_b2 {
                return Err(invalid("missing B2 station ID"));
            }
            return Ok(Some(line));
        }
    }
    Err(invalid("B2F greeting exceeded line limit"))
}

fn receive(
    stream: &mut (impl BufRead + Write),
    settings: Settings<'_>,
    mailbox: &mut impl Mailbox,
    block: &str,
) -> Result<()> {
    let proposals = proposal::decode(
        block.as_bytes(),
        settings.max_message_bytes,
        settings.max_compressed_bytes,
    )?;
    let mut reply = String::from("FS ");
    let mut accepted = Vec::new();
    for item in proposals {
        if item.kind != ProposalKind::Message {
            reply.push('=');
        } else if mailbox.contains(item.id)? {
            reply.push('-');
        } else {
            reply.push('+');
            accepted.push(item);
        }
    }
    reply.push('\r');
    send(stream, reply.as_bytes())?;
    for item in accepted {
        let envelope = transfer::receive_message(
            stream,
            &item,
            &[],
            settings.max_message_bytes,
            settings.max_compressed_bytes,
        )?;
        mailbox.store(item.id, &envelope)?;
    }
    Ok(())
}

fn transmit(
    stream: &mut (impl BufRead + Write),
    settings: Settings<'_>,
    mailbox: &mut impl Mailbox,
    outgoing: &[Vec<u8>],
    deferred: &mut Vec<String>,
) -> Result<Vec<String>> {
    let mut offers = Vec::new();
    let mut payloads = Vec::new();
    for envelope in outgoing {
        let message = Message::decode(envelope, settings.max_message_bytes)?;
        let compressed = super::compression::encode(envelope, settings.max_compressed_bytes)?;
        offers.push(Proposal {
            kind: ProposalKind::Message,
            id: message.id,
            message_bytes: envelope.len(),
            compressed_bytes: compressed.len(),
        });
        payloads.push((message.subject, compressed));
    }
    let block = proposal::encode(
        &offers,
        settings.max_message_bytes,
        settings.max_compressed_bytes,
    )?;
    send(stream, block.as_bytes())?;
    let mut answer = read_line(stream)?;
    for _ in 0..MAX_GREETING_LINES {
        if !answer.starts_with(';') {
            break;
        }
        answer = read_line(stream)?;
    }
    let answers = proposal::decode_answers(&answer, &offers)?;
    let mut sent = Vec::new();
    for ((offer, (title, compressed)), answer) in offers.iter().zip(payloads).zip(answers) {
        match answer {
            Answer::Accept { offset } => {
                transfer::write(
                    stream,
                    title,
                    &compressed,
                    offset,
                    settings.max_compressed_bytes,
                )?;
                sent.push(offer.id.to_owned());
            }
            Answer::Reject => mailbox.finish(offer.id, Delivery::Rejected)?,
            Answer::Defer => deferred.push(offer.id.to_owned()),
        }
    }
    stream.flush()?;
    Ok(sent)
}

fn send(stream: &mut impl Write, bytes: &[u8]) -> Result<()> {
    stream.write_all(bytes)?;
    stream.flush()
}

fn read_line(stream: &mut impl BufRead) -> Result<String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    // Pat skips optional LF bytes between protocol lines.
    if stream.fill_buf()?.first() == Some(&b'\n') {
        stream.consume(1);
    }
    stream.take(MAX_LINE as u64).read_until(b'\r', &mut bytes)?;
    if bytes.is_empty() {
        return Err(Error::new(
            ErrorKind::UnexpectedEof,
            "B2F connection closed",
        ));
    }
    if bytes.last() != Some(&b'\r') {
        return Err(invalid("unterminated B2F protocol line"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("invalid B2F protocol text"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::winlink::mailbox::DirectoryMailbox;
    use std::{
        fs,
        io::{BufReader, Read},
        net::{TcpListener, TcpStream},
        path::PathBuf,
        thread,
        time::Duration,
    };

    struct Root(PathBuf);

    impl Root {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("linkwise-test-{}", uuid::Uuid::new_v4())))
        }

        fn mailbox(&self, call: &str) -> DirectoryMailbox {
            DirectoryMailbox::open(self.0.join(call), 8192).unwrap()
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Connection(BufReader<TcpStream>);

    impl Connection {
        fn new(socket: TcpStream) -> Self {
            let timeout = Some(Duration::from_secs(5));
            socket.set_read_timeout(timeout).unwrap();
            socket.set_write_timeout(timeout).unwrap();
            Self(BufReader::new(socket))
        }
    }

    impl Read for Connection {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            self.0.read(buf)
        }
    }

    impl BufRead for Connection {
        fn fill_buf(&mut self) -> Result<&[u8]> {
            self.0.fill_buf()
        }

        fn consume(&mut self, count: usize) {
            self.0.consume(count);
        }
    }

    impl Write for Connection {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            self.0.get_mut().write(buf)
        }

        fn flush(&mut self) -> Result<()> {
            self.0.get_mut().flush()
        }
    }

    fn settings<'a>(local_call: &'a str, peer_call: &'a str) -> Settings<'a> {
        Settings {
            local_call,
            peer_call,
            max_message_bytes: 8192,
            max_compressed_bytes: 8192,
        }
    }

    fn envelope(id: &str, from: &str, to: &str, attachment: &[u8]) -> Vec<u8> {
        Message {
            id,
            date: "2026/11/13 12:00",
            kind: "Private",
            from,
            to: vec![to],
            cc: Vec::new(),
            subject: "B2F loopback",
            mbo: None,
            body: "Bench\r\n",
            attachments: vec![super::super::Attachment {
                name: "bench.bin",
                bytes: attachment,
            }],
        }
        .encode(8192)
        .unwrap()
    }

    fn run_pair(caller: &mut DirectoryMailbox, mut listener: DirectoryMailbox) {
        let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = server.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let mut stream = Connection::new(server.accept().unwrap().0);
            exchange(
                &mut stream,
                Role::Listener,
                settings("W1AW", "N0CALL"),
                &mut listener,
            )
        });
        let mut stream = Connection::new(TcpStream::connect(address).unwrap());
        exchange(
            &mut stream,
            Role::Caller,
            settings("N0CALL", "W1AW"),
            caller,
        )
        .unwrap();
        drop(stream);
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn both_directions_persist_exact_envelopes_and_finish_cleanly() {
        let root = Root::new();
        let mut caller = root.mailbox("caller");
        let listener = root.mailbox("listener");
        let attachment: Vec<_> = (0..4096).map(|i| (i % 251) as u8).collect();
        for i in 0..6 {
            caller
                .enqueue(&envelope(&format!("A{i}"), "N0CALL", "W1AW", &attachment))
                .unwrap();
        }
        let back = envelope("B", "W1AW", "N0CALL", b"\x00\xff\r\n");
        listener.enqueue(&back).unwrap();
        run_pair(&mut caller, listener);
        let mut listener = root.mailbox("listener");
        assert_eq!(caller.incoming("B").unwrap(), back);
        for i in 0..6 {
            let id = format!("A{i}");
            assert_eq!(
                listener.incoming(&id).unwrap(),
                envelope(&id, "N0CALL", "W1AW", &attachment)
            );
        }
        assert!(caller.outgoing("W1AW", &[]).unwrap().is_empty());
        assert!(listener.outgoing("N0CALL", &[]).unwrap().is_empty());
    }

    #[test]
    fn lost_ack_retains_outbox_and_retry_rejects_already_stored_message() {
        let root = Root::new();
        let mut caller = root.mailbox("caller");
        let data = envelope("RETRY", "N0CALL", "W1AW", b"\x80\x00\xff");
        caller.enqueue(&data).unwrap();
        let mut listener = root.mailbox("listener");
        let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = server.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let mut stream = Connection::new(server.accept().unwrap().0);
            let options = settings("W1AW", "N0CALL");
            send_greeting(&mut stream, options, true).unwrap();
            let first = read_greeting(&mut stream, false).unwrap();
            let Turn::Proposals(block) = read_turn(&mut stream, options, first).unwrap() else {
                panic!("expected proposal");
            };
            receive(&mut stream, options, &mut listener, &block).unwrap();
            // The peer drops after durable storage and before sending the next turn.
        });
        let mut stream = Connection::new(TcpStream::connect(address).unwrap());
        assert!(
            exchange(
                &mut stream,
                Role::Caller,
                settings("N0CALL", "W1AW"),
                &mut caller
            )
            .is_err()
        );
        worker.join().unwrap();
        assert_eq!(
            root.mailbox("caller").outgoing("W1AW", &[]).unwrap(),
            [data.clone()]
        );
        run_pair(&mut caller, root.mailbox("listener"));
        assert!(caller.outgoing("W1AW", &[]).unwrap().is_empty());
        assert_eq!(root.mailbox("listener").incoming("RETRY").unwrap(), data);
    }
}
