//! Conversion between internet mail and the local B2F P2P mailbox.

use std::io::Result;

use chrono::{DateTime, NaiveDateTime, Utc};
use mail_builder::MessageBuilder;
use mail_parser::{MessageParser, MimeHeaders, PartType};

use super::{Attachment, Message, invalid};

/// Import a message for one Winlink P2P recipient.
pub fn import(raw: &[u8], limit: usize) -> Result<Vec<u8>> {
    if raw.len() > limit.saturating_mul(2) {
        return Err(invalid("MIME message exceeds configured limit"));
    }
    let mail = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| invalid("invalid MIME message"))?;
    if mail.parts.iter().any(|part| part.is_encoding_problem) {
        return Err(invalid("invalid MIME transfer encoding"));
    }
    let from = one_address(mail.from())?;
    let to = one_address(mail.to())?;
    if mail.cc().is_some() || mail.bcc().is_some() {
        return Err(invalid("P2P mail supports one recipient"));
    }
    if mail.text_body_count() == 0 {
        return Err(invalid("P2P mail requires a plain-text body"));
    }
    let text = mail
        .body_text(0)
        .ok_or_else(|| invalid("missing MIME text body"))?;
    let body = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n");
    // B2F requires a nonempty body. Represent an empty MIME text part as
    // a blank line so attachment-only messages retain their payload.
    let body = if body.is_empty() { "\r\n" } else { &body };
    let date = mail
        .date()
        .and_then(|date| DateTime::from_timestamp(date.to_timestamp(), 0))
        .unwrap_or_else(Utc::now)
        .format("%Y/%m/%d %H:%M")
        .to_string();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let attachments = mail
        .attachments()
        .map(|part| {
            let name = part
                .attachment_name()
                .ok_or_else(|| invalid("MIME attachment requires a filename"))?;
            if !matches!(
                part.body,
                PartType::Binary(_) | PartType::InlineBinary(_) | PartType::Message(_)
            ) {
                return Err(invalid("unsupported MIME attachment encoding"));
            }
            Ok(Attachment {
                name,
                bytes: part.contents(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Message {
        id: &id[..12],
        date: &date,
        kind: "Private",
        from: &from,
        to: vec![&to],
        cc: vec![],
        subject: mail.subject().unwrap_or_default(),
        mbo: None,
        body,
        attachments,
    }
    .encode(limit)
}

/// Render a stored B2F message as RFC 5322/MIME.
pub fn export(wire: &[u8], limit: usize) -> Result<Vec<u8>> {
    let message = Message::decode(wire, limit)?;
    let date = NaiveDateTime::parse_from_str(message.date, "%Y/%m/%d %H:%M")
        .map_err(|_| invalid("invalid B2F message date"))?;
    let mut mail = MessageBuilder::new()
        .message_id(format!("{}@winlink.org", message.id))
        .date(date.and_utc().timestamp())
        .from(mail_address(message.from)?)
        .to(message
            .to
            .iter()
            .map(|address| mail_address(address))
            .collect::<Result<Vec<_>>>()?)
        .subject(message.subject)
        .text_body(message.body);
    if !message.cc.is_empty() {
        mail = mail.cc(message
            .cc
            .iter()
            .map(|address| mail_address(address))
            .collect::<Result<Vec<_>>>()?);
    }
    for file in message.attachments {
        mail = mail.attachment("application/octet-stream", file.name, file.bytes);
    }
    mail.write_to_vec()
}

fn one_address(addresses: Option<&mail_parser::Address<'_>>) -> Result<String> {
    let mut addresses = addresses
        .ok_or_else(|| invalid("missing MIME address"))?
        .iter();
    let address = addresses
        .next()
        .and_then(|address| address.address.as_deref())
        .ok_or_else(|| invalid("invalid MIME address"))?;
    if addresses.next().is_some() {
        return Err(invalid("P2P mail supports one sender and one recipient"));
    }
    let (call, domain) = address
        .split_once('@')
        .ok_or_else(|| invalid("expected callsign@winlink.org"))?;
    if !domain.eq_ignore_ascii_case("winlink.org") || !valid_call(call) {
        return Err(invalid("expected callsign@winlink.org"));
    }
    Ok(call.to_ascii_uppercase())
}

pub(crate) fn valid_call(call: &str) -> bool {
    !call.is_empty()
        && call.len() <= 12
        && call
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn mail_address(address: &str) -> Result<String> {
    if valid_call(address) {
        return Ok(format!("{address}@winlink.org"));
    }
    let bare = address.strip_prefix("SMTP:").unwrap_or(address);
    if let Some((local, domain)) = bare.split_once('@')
        && !local.is_empty()
        && !domain.is_empty()
        && bare
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~@".contains(&byte))
        && !domain.contains('@')
    {
        return Ok(bare.to_owned());
    }
    Err(invalid("invalid B2F mailbox address"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_only_mime_imports_with_a_blank_b2f_body() {
        let source = b"From: N0CALL@winlink.org\r\nTo: W1AW@winlink.org\r\n\
            Subject: Attachment only\r\nMIME-Version: 1.0\r\n\
            Content-Type: multipart/mixed; boundary=bench\r\n\r\n\
            --bench\r\nContent-Type: text/plain\r\n\r\n\r\n\
            --bench\r\nContent-Type: application/octet-stream\r\n\
            Content-Disposition: attachment; filename=bench.bin\r\n\
            Content-Transfer-Encoding: base64\r\n\r\nAP8=\r\n--bench--\r\n";
        let wire = import(source, 4096).unwrap();
        let message = Message::decode(&wire, 4096).unwrap();
        assert_eq!(message.body, "\r\n");
        assert_eq!(message.attachments[0].bytes, b"\x00\xff");
    }

    #[test]
    fn mime_round_trip_preserves_binary_attachment() {
        let source = MessageBuilder::new()
            .from("N0CALL@winlink.org")
            .to("w1aw@winlink.org")
            .subject("Bench")
            .text_body("Line one\nLine two")
            .attachment(
                "application/octet-stream",
                "bench.bin",
                &b"\x00\xff\r\n"[..],
            )
            .write_to_vec()
            .unwrap();
        let wire = import(&source, 4096).unwrap();
        let message = Message::decode(&wire, 4096).unwrap();
        assert_eq!(message.to, ["W1AW"]);
        assert_eq!(message.body.trim_end(), "Line one\r\nLine two");
        let rendered = export(&wire, 4096).unwrap();
        let mail = MessageParser::default().parse(&rendered).unwrap();
        assert_eq!(mail.subject(), Some("Bench"));
        assert_eq!(
            mail.attachment(0).unwrap().attachment_name(),
            Some("bench.bin")
        );
        assert_eq!(mail.attachment(0).unwrap().contents(), b"\x00\xff\r\n");
    }
}
