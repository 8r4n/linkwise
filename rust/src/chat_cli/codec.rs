//! Connected VarAC DATA records, independent of TCP read and RF frame boundaries.
use std::io;

pub const MAX_TEXT: usize = 1024;
const MAX_RECORD: usize = 64 * 1024 - 1;

pub fn encode(text: &str) -> io::Result<Vec<u8>> {
    if text.trim().is_empty() || text.len() > MAX_TEXT || text.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "enter 1–1024 bytes of text without control characters",
        ));
    }
    Ok(format!("{} {text}", text.len()).into_bytes())
}

#[derive(Default)]
pub struct Decoder {
    digits: usize,
    size: usize,
    body: Option<Vec<u8>>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8], mut record: impl FnMut(&[u8])) -> io::Result<()> {
        for &byte in bytes {
            if let Some(body) = &mut self.body {
                body.push(byte);
                if body.len() == self.size {
                    record(body);
                    *self = Self::default();
                }
            } else if byte.is_ascii_digit() && self.digits < 5 {
                self.digits += 1;
                self.size = self.size * 10 + usize::from(byte - b'0');
            } else if byte == b' ' && (1..=MAX_RECORD).contains(&self.size) {
                self.body = Some(Vec::with_capacity(self.size));
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid VarAC record length",
                ));
            }
        }
        Ok(())
    }
}

pub fn display(bytes: &[u8]) -> String {
    let mut chars = String::from_utf8_lossy(bytes)
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(4097)
        .collect::<String>();
    if let Some((index, _)) = chars.char_indices().nth(4096) {
        chars.truncate(index);
        chars.push('…');
    }
    chars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_capture_matches_at_every_read_boundary() {
        let alpha = "Alpha confirms stock text 123.";
        let bravo = "Bravo confirms stock text 456.";
        let captured = b"15 de BB2BB <S:VC>4 &IT&30 Alpha confirms stock text 123.30 Bravo confirms stock text 456.";
        assert_eq!(encode(alpha).unwrap(), b"30 Alpha confirms stock text 123.");
        assert_eq!(encode(bravo).unwrap(), b"30 Bravo confirms stock text 456.");
        for split in 0..=captured.len() {
            let mut decoder = Decoder::default();
            let mut records = Vec::new();
            decoder
                .push(&captured[..split], |record| records.push(display(record)))
                .unwrap();
            decoder
                .push(&captured[split..], |record| records.push(display(record)))
                .unwrap();
            assert_eq!(records, ["de BB2BB <S:VC>", "&IT&", alpha, bravo]);
        }
    }

    #[test]
    fn framing_and_terminal_text_are_bounded() {
        assert!(Decoder::default().push(b"65536 ", |_| {}).is_err());
        assert!(Decoder::default().push(b"x ", |_| {}).is_err());
        assert!(encode("").is_err());
        assert!(encode(&"a".repeat(MAX_TEXT + 1)).is_err());
        assert!(encode("\u{1b}[2J").is_err());
        assert_eq!(display(b"a\x1b\n\xff"), "a  \u{fffd}");
    }
}
