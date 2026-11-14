//! Winlink Open B2F message envelopes.

pub mod compression;
pub mod imap;

pub mod mailbox;
pub mod mime;

pub mod mail_tls;
pub mod proposal;
pub mod session;
pub mod smtp;

pub mod transfer;

use std::io::{Error, ErrorKind, Result};

const HEADER_LIMIT: usize = 16 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Attachment<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Debug, PartialEq, Eq)]
pub struct Message<'a> {
    pub id: &'a str,
    pub date: &'a str,
    pub kind: &'a str,
    pub from: &'a str,
    pub to: Vec<&'a str>,
    pub cc: Vec<&'a str>,
    pub subject: &'a str,
    pub mbo: Option<&'a str>,
    pub body: &'a str,
    pub attachments: Vec<Attachment<'a>>,
}

fn invalid(reason: &'static str) -> Error {
    Error::new(ErrorKind::InvalidData, reason)
}

impl<'a> Message<'a> {
    /// Encode a bounded B2 envelope and compress it for a forwarding proposal.
    pub fn encode_compressed(
        &self,
        max_message_bytes: usize,
        max_compressed_bytes: usize,
    ) -> Result<(usize, Vec<u8>)> {
        let wire = self.encode(max_message_bytes)?;
        let compressed = compression::encode(&wire, max_compressed_bytes)?;
        Ok((wire.len(), compressed))
    }

    /// Parse an uncompressed envelope within the caller's message size limit.
    pub fn decode(wire: &'a [u8], max_bytes: usize) -> Result<Self> {
        if wire.len() > max_bytes {
            return Err(invalid("B2F message exceeds configured limit"));
        }
        let header_end = wire[..wire.len().min(HEADER_LIMIT)]
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .ok_or_else(|| invalid("missing or oversized B2F header"))?;
        let header = std::str::from_utf8(&wire[..header_end])
            .map_err(|_| invalid("invalid B2F header encoding"))?;
        let mut lines = header.split("\r\n");
        let first = lines.next().and_then(|line| line.split_once(':'));
        let Some((key, id)) = first else {
            return Err(invalid("missing B2F message ID"));
        };
        if !key.eq_ignore_ascii_case("Mid") {
            return Err(invalid("B2F message ID must be first"));
        }
        let mut message = Self {
            id: id.trim_ascii_start(),
            date: "",
            kind: "",
            from: "",
            to: Vec::new(),
            cc: Vec::new(),
            subject: "",
            mbo: None,
            body: "",
            attachments: Vec::new(),
        };
        let mut body_length = None;
        let mut files = Vec::new();
        for line in lines {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| invalid("invalid B2F header"))?;
            let value = value.trim_ascii_start();
            if !field_text(key) || !field_text(value) {
                return Err(invalid("invalid B2F header text"));
            }
            match key.to_ascii_lowercase().as_str() {
                "mid" => return Err(invalid("duplicate B2F message ID")),
                "date" => message.date = value,
                "type" => message.kind = value,
                "from" => message.from = value,
                "to" => message.to.push(value),
                "cc" => message.cc.push(value),
                "subject" => message.subject = value,
                "mbo" => message.mbo = Some(value),
                "body" if body_length.is_none() => body_length = Some(length(value)?),
                "body" => return Err(invalid("duplicate B2F body length")),
                "file" => {
                    let (size, name) = value
                        .split_once(' ')
                        .ok_or_else(|| invalid("invalid B2F file header"))?;
                    files.push((length(size)?, name));
                }
                _ => {}
            }
        }
        let mut payload = &wire[header_end + 4..];
        let body = read_part(
            &mut payload,
            body_length.ok_or_else(|| invalid("missing B2F body length"))?,
        )?;
        message.body =
            std::str::from_utf8(body).map_err(|_| invalid("invalid B2F body encoding"))?;
        for (size, name) in files {
            message.attachments.push(Attachment {
                name,
                bytes: read_part(&mut payload, size)?,
            });
        }
        // Winlink Express 1.8.6 appends one NUL after the final CRLF.
        if !matches!(payload, [] | [0]) {
            return Err(invalid("trailing bytes after B2F message"));
        }
        message.validate()?;
        Ok(message)
    }

    /// Encode an uncompressed envelope without exceeding the caller's limit.
    pub fn encode(&self, max_bytes: usize) -> Result<Vec<u8>> {
        self.validate()?;
        let mut header = String::new();
        let mut field = |key: &str, value: &str| -> Result<()> {
            if header
                .len()
                .saturating_add(key.len())
                .saturating_add(value.len())
                .saturating_add(6)
                > HEADER_LIMIT.min(max_bytes)
            {
                return Err(invalid("B2F header exceeds configured limit"));
            }
            header.push_str(key);
            header.push_str(": ");
            header.push_str(value);
            header.push_str("\r\n");
            Ok(())
        };
        field("Mid", self.id)?;
        field("Date", self.date)?;
        field("Type", self.kind)?;
        field("From", self.from)?;
        for address in &self.to {
            field("To", address)?;
        }
        for address in &self.cc {
            field("Cc", address)?;
        }
        field("Subject", self.subject)?;
        if let Some(mbo) = self.mbo {
            field("Mbo", mbo)?;
        }
        field("Body", &self.body.len().to_string())?;
        for file in &self.attachments {
            field("File", &format!("{} {}", file.bytes.len(), file.name))?;
        }
        header.push_str("\r\n");
        let parts = std::iter::once(self.body.as_bytes())
            .chain(self.attachments.iter().map(|file| file.bytes));
        let size = parts
            .clone()
            .try_fold(header.len(), |total, part| {
                total.checked_add(part.len())?.checked_add(2)
            })
            .filter(|&size| size <= max_bytes)
            .ok_or_else(|| invalid("B2F message exceeds configured limit"))?;
        let mut wire = header.into_bytes();
        wire.reserve_exact(size - wire.len());
        for part in parts {
            wire.extend_from_slice(part);
            wire.extend_from_slice(b"\r\n");
        }
        Ok(wire)
    }

    fn validate(&self) -> Result<()> {
        if !valid_mid(self.id)
            || self.date.is_empty()
            || self.kind.is_empty()
            || self.from.is_empty()
            || self.to.is_empty()
            || self.subject.len() > 128
            || self.body.is_empty()
            || !self.body.is_ascii()
        {
            return Err(invalid("invalid required B2F fields"));
        }
        if ![self.date, self.kind, self.from, self.subject]
            .into_iter()
            .chain(self.to.iter().copied())
            .chain(self.cc.iter().copied())
            .chain(self.mbo)
            .all(field_text)
            || self
                .to
                .iter()
                .chain(&self.cc)
                .any(|address| address.trim().is_empty())
            || self.attachments.iter().any(|file| {
                file.name.trim().is_empty() || file.name.len() > 255 || !field_text(file.name)
            })
        {
            return Err(invalid("invalid B2F header text"));
        }
        Ok(())
    }
}

fn field_text(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte == b' ' || byte.is_ascii_graphic())
}

fn checksum(data: &[u8]) -> u8 {
    data.iter()
        .copied()
        .fold(0_u8, u8::wrapping_add)
        .wrapping_neg()
}

fn valid_mid(id: &str) -> bool {
    !id.is_empty() && id.len() <= 12 && id.bytes().all(|byte| byte.is_ascii_graphic())
}

fn length(value: &str) -> Result<usize> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid("invalid B2F part length"));
    }
    value
        .parse()
        .map_err(|_| invalid("invalid B2F part length"))
}

fn read_part<'a>(payload: &mut &'a [u8], size: usize) -> Result<&'a [u8]> {
    let (part, rest) = payload
        .split_at_checked(size)
        .ok_or_else(|| invalid("truncated B2F part"))?;
    *payload = rest
        .strip_prefix(b"\r\n")
        .ok_or_else(|| invalid("missing B2F part terminator"))?;
    Ok(part)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) const FIXTURE: &[u8] = b"Mid: TEST123\r\nDate: 2026/11/13 12:00\r\nType: Private\r\nFrom: W1AW\r\nTo: N0CALL\r\nTo: SMTP:user@example.org\r\nCc: K1ABC\r\nSubject: Bench\r\nBody: 4\r\nFile: 4 test data.bin\r\n\r\nHi\r\n\r\n\x00\xff\r\n\r\n";

    #[test]
    fn binary_attachment_and_multiple_addresses() {
        let message = Message::decode(FIXTURE, FIXTURE.len()).unwrap();
        assert_eq!(message.to, ["N0CALL", "SMTP:user@example.org"]);
        assert_eq!(message.cc, ["K1ABC"]);
        assert_eq!(message.body, "Hi\r\n");
        assert_eq!(message.attachments[0].name, "test data.bin");
        assert_eq!(message.attachments[0].bytes, b"\x00\xff\r\n");
        assert_eq!(message.encode(FIXTURE.len()).unwrap(), FIXTURE);
        assert!(message.encode(FIXTURE.len() - 1).is_err());
    }

    #[test]
    fn accepts_one_express_terminator_after_the_attachment() {
        let mut wire = FIXTURE.to_vec();
        wire.push(0);
        assert_eq!(
            Message::decode(&wire, wire.len()).unwrap(),
            Message::decode(FIXTURE, FIXTURE.len()).unwrap()
        );
        wire.push(0);
        assert!(Message::decode(&wire, wire.len()).is_err());
    }

    #[test]
    fn rejects_truncation_and_invalid_lengths() {
        for end in 0..FIXTURE.len() {
            assert!(Message::decode(&FIXTURE[..end], FIXTURE.len()).is_err());
        }
        assert!(Message::decode(FIXTURE, FIXTURE.len() - 1).is_err());
        let mut oversized = FIXTURE.to_vec();
        let start = FIXTURE
            .windows(7)
            .position(|part| part == b"Body: 4")
            .unwrap();
        oversized.splice(
            start + 6..start + 7,
            b"99999999999999999999999".iter().copied(),
        );
        assert!(Message::decode(&oversized, oversized.len()).is_err());
    }

    #[test]
    fn rejects_header_injection_on_encode() {
        let mut message = Message::decode(FIXTURE, FIXTURE.len()).unwrap();
        message.subject = "Bench\r\nTo: BAD";
        assert!(message.encode(4096).is_err());
    }
}
