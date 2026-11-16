//! Signed and optionally encrypted inner B2F messages.
use std::{io, path::Path};

use base64::{Engine, engine::general_purpose::STANDARD};
use linkwise::winlink::{Attachment, Message, mailbox::DirectoryMailbox, mime};
use serde::{Deserialize, Serialize};

use crate::{
    frame_cli::Failure,
    pki::{self, Application, Pki, Signed},
};

const LIMIT: usize = 1024 * 1024;
const ATTACHMENT: &str = "linkwise-pki-v1.bin";

#[derive(Deserialize, Serialize)]
#[serde(
    deny_unknown_fields,
    tag = "mode",
    content = "content",
    rename_all = "snake_case"
)]
enum Protected {
    Signed(Signed),
    Encrypted(String),
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn recipient<'a>(message: &'a Message<'a>) -> io::Result<&'a str> {
    match message.to.as_slice() {
        [recipient] if message.cc.is_empty() => Ok(recipient),
        _ => Err(invalid(
            "protected radio mail requires exactly one recipient",
        )),
    }
}

fn protect(config: &Pki, wire: &[u8], encrypted: bool) -> io::Result<Vec<u8>> {
    let msg = Message::decode(wire, LIMIT)?;
    let to = recipient(&msg)?;
    if !msg.from.eq_ignore_ascii_case(config.callsign()) {
        return Err(invalid("radio email sender does not match the PKI station"));
    }
    let peer = to.to_ascii_uppercase();
    let signed = config.sign(pki::MAIL_CONTEXT, &peer, wire)?;
    let protected = if encrypted || config.require_encryption(&peer)? {
        let ciphertext = config.encrypt(Application::Mail, &peer, &serde_json::to_vec(&signed)?)?;
        Protected::Encrypted(STANDARD.encode(ciphertext))
    } else {
        Protected::Signed(signed)
    };
    let bytes = serde_json::to_vec(&protected)?;
    Message {
        id: msg.id,
        date: msg.date,
        kind: msg.kind,
        from: msg.from,
        to: msg.to,
        cc: vec![],
        subject: "Linkwise protected radio mail",
        mbo: msg.mbo,
        body: "Open this attachment with Linkwise to verify the message.",
        attachments: vec![Attachment {
            name: ATTACHMENT,
            bytes: &bytes,
        }],
    }
    .encode(LIMIT)
}

fn open(config: &Pki, wire: &[u8], require_encryption: bool) -> io::Result<(Vec<u8>, String)> {
    let outer = Message::decode(wire, LIMIT)?;
    let [attachment] = outer.attachments.as_slice() else {
        return Err(invalid("expected one protected radio mail attachment"));
    };
    if attachment.name != ATTACHMENT {
        return Err(invalid("missing protected radio mail attachment"));
    }
    let signed = match serde_json::from_slice::<Protected>(attachment.bytes)? {
        Protected::Signed(signed)
            if !require_encryption && !config.require_encryption(&signed.from)? =>
        {
            signed
        }
        Protected::Signed(_) => return Err(invalid("encrypted mail was required")),
        Protected::Encrypted(ciphertext) => {
            let ciphertext = STANDARD.decode(ciphertext).map_err(io::Error::other)?;
            let inner = config.decrypt(Application::Mail, &ciphertext)?;
            serde_json::from_slice(&inner)?
        }
    };
    let plaintext = config.verify(pki::MAIL_CONTEXT, &signed, LIMIT)?;
    let inner = Message::decode(&plaintext, LIMIT)?;
    if inner.id != outer.id
        || !inner.from.eq_ignore_ascii_case(&signed.from)
        || !recipient(&inner)?.eq_ignore_ascii_case(&signed.to)
    {
        return Err(invalid(
            "protected radio mail identity or message ID does not match its signature",
        ));
    }
    Ok((plaintext, signed.from))
}

pub fn run(args: &[std::ffi::OsString]) -> Result<(), Failure> {
    let [command, flag, config, rest @ ..] = args else {
        return Err(usage());
    };
    if flag != "--pki" {
        return Err(usage());
    }
    let (encrypted, rest) = match rest {
        [flag, rest @ ..] if flag == "--encrypt" => (true, rest),
        _ => (false, rest),
    };
    let [root, argument] = rest else {
        return Err(usage());
    };
    let config = Pki::load(Path::new(config)).map_err(Failure::operation)?;
    let mailbox = DirectoryMailbox::open(root, LIMIT).map_err(Failure::operation)?;
    match command.to_str() {
        Some("import" | "queue") => {
            let bytes = pki::read_bounded(Path::new(argument), 2 * LIMIT as u64)
                .map_err(Failure::operation)?;
            let original = if command == "import" {
                mime::import(&bytes, LIMIT).map_err(Failure::operation)?
            } else {
                bytes
            };
            let protected = protect(&config, &original, encrypted).map_err(Failure::operation)?;
            mailbox.enqueue(&protected).map_err(Failure::operation)
        }
        Some("export") => {
            let id = argument.to_str().ok_or_else(usage)?;
            let wire = mailbox.incoming(id).map_err(Failure::operation)?;
            let (message, from) = open(&config, &wire, encrypted).map_err(Failure::operation)?;
            let mail = mime::export(&message, LIMIT).map_err(Failure::operation)?;
            eprintln!("Verified radio email sender: {from}");
            io::Write::write_all(&mut io::stdout().lock(), &mail).map_err(Failure::operation)
        }
        _ => Err(usage()),
    }
}

fn usage() -> Failure {
    Failure::usage("mail import|queue|export --pki FILE [--encrypt] MAILBOX FILE_OR_ID")
}
