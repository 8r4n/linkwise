//! B2F binary forwarding records for an already compressed message.

use std::io::{Read, Result, Write};

use super::{Message, checksum, compression, field_text, invalid, length, proposal::Proposal};

const SOH: u8 = 1;
const STX: u8 = 2;
const EOT: u8 = 4;
const BLOCK_BYTES: usize = 256;
const MIN_COMPRESSED_BYTES: usize = 6;
pub(super) const MAX_OFFSET: usize = 999_999;

#[derive(Debug, Eq, PartialEq)]
pub struct Transfer {
    pub title: String,
    pub suffix: Vec<u8>,
}

/// Receive the outstanding suffix and verify the complete envelope against
/// its proposal. The caller can persist the returned uncompressed envelope
/// only after this succeeds. A failed transfer leaves the saved prefix intact.
pub fn receive_message(
    input: &mut impl Read,
    proposal: &Proposal<'_>,
    prefix: &[u8],
    max_message_bytes: usize,
    max_compressed_bytes: usize,
) -> Result<Vec<u8>> {
    if !(1..=max_message_bytes).contains(&proposal.message_bytes) {
        return Err(invalid("B2F message exceeds configured limit"));
    }
    let transfer = read(
        input,
        proposal.compressed_bytes,
        prefix.len(),
        max_compressed_bytes,
    )?;
    let compressed = if prefix.is_empty() {
        transfer.suffix
    } else {
        let mut joined = Vec::with_capacity(proposal.compressed_bytes);
        joined.extend_from_slice(prefix);
        joined.extend_from_slice(&transfer.suffix);
        joined
    };
    let wire = compression::decode(&compressed, proposal.message_bytes)?;
    if Message::decode(&wire, max_message_bytes)?.id != proposal.id {
        return Err(invalid("B2F message ID differs from proposal"));
    }
    Ok(wire)
}

/// Read one transfer, leaving subsequent protocol bytes in the stream.
/// The caller provides the proposal size and the offset it requested.
pub fn read(
    input: &mut impl Read,
    compressed_bytes: usize,
    offset: usize,
    max_compressed_bytes: usize,
) -> Result<Transfer> {
    let remaining = suffix_len(compressed_bytes, offset, max_compressed_bytes)?;
    let [marker, header_len] = pair(input)?;
    if marker != SOH || header_len < 3 {
        return Err(invalid("invalid B2F transfer header"));
    }
    let mut header = vec![0; usize::from(header_len)];
    input.read_exact(&mut header)?;
    let mut fields = header.split(|&b| b == 0);
    let (Some(title), Some(position), Some([]), None) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return Err(invalid("invalid B2F transfer fields"));
    };
    let title = std::str::from_utf8(title).map_err(|_| invalid("invalid B2F transfer title"))?;
    let position =
        std::str::from_utf8(position).map_err(|_| invalid("invalid B2F transfer offset"))?;
    if !field_text(title) || position.len() > 6 || length(position)? != offset {
        return Err(invalid("invalid B2F transfer title or offset"));
    }
    let mut suffix = Vec::new();
    loop {
        let [marker, count] = pair(input)?;
        match marker {
            STX => {
                let count = if count == 0 {
                    BLOCK_BYTES
                } else {
                    usize::from(count)
                };
                let start = suffix.len();
                if count > remaining - start {
                    return Err(invalid("B2F data exceeds proposed size"));
                }
                suffix.resize(start + count, 0);
                input.read_exact(&mut suffix[start..])?;
            }
            EOT if suffix.len() == remaining && count == checksum(&suffix) => {
                return Ok(Transfer {
                    title: title.into(),
                    suffix,
                });
            }
            EOT => return Err(invalid("invalid B2F data length or checksum")),
            _ => return Err(invalid("unknown B2F binary record")),
        }
    }
}

/// Write a transfer from the requested offset in the compressed message.
pub fn write(
    output: &mut impl Write,
    title: &str,
    compressed: &[u8],
    offset: usize,
    max_compressed_bytes: usize,
) -> Result<()> {
    suffix_len(compressed.len(), offset, max_compressed_bytes)?;
    let position = offset.to_string();
    let header_len = title.len().saturating_add(position.len() + 2);
    if header_len > usize::from(u8::MAX) || !field_text(title) {
        return Err(invalid("invalid B2F transfer title"));
    }
    output.write_all(&[SOH, header_len as u8])?;
    output.write_all(title.as_bytes())?;
    output.write_all(&[0])?;
    output.write_all(position.as_bytes())?;
    output.write_all(&[0])?;
    let suffix = &compressed[offset..];
    for chunk in suffix.chunks(BLOCK_BYTES) {
        output.write_all(&[STX, chunk.len() as u8])?;
        output.write_all(chunk)?;
    }
    output.write_all(&[EOT, checksum(suffix)])
}

fn suffix_len(total: usize, offset: usize, max: usize) -> Result<usize> {
    if !(MIN_COMPRESSED_BYTES..=max).contains(&total) || offset > MAX_OFFSET || offset > total {
        return Err(invalid("invalid B2F transfer size or offset"));
    }
    Ok(total - offset)
}

fn pair(input: &mut impl Read) -> Result<[u8; 2]> {
    let mut bytes = [0; 2];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_length_block_and_partial_transfer() {
        let data = [3; 257];
        let mut wire = Vec::new();
        write(&mut wire, "Test", &data, 1, 257).unwrap();
        assert_eq!(&wire[..11], b"\x01\x07Test\x001\x00\x02\x00");
        assert_eq!(&wire[wire.len() - 2..], &[4, 0]);
        wire.extend_from_slice(b"FF\r");
        let mut stream = wire.as_slice();
        assert_eq!(read(&mut stream, 257, 1, 257).unwrap().suffix, data[1..]);
        assert_eq!(stream, b"FF\r");
        assert!(read(&mut wire.as_slice(), 257, 0, 257).is_err());
    }

    #[test]
    fn resumed_message_verifies_the_original_envelope_and_proposal() {
        let fixture = super::super::tests::FIXTURE;
        let message = Message::decode(fixture, 1024).unwrap();
        let (message_bytes, compressed) = message.encode_compressed(1024, 1024).unwrap();
        let mut proposal = Proposal {
            kind: super::super::proposal::ProposalKind::Message,
            id: message.id,
            message_bytes,
            compressed_bytes: compressed.len(),
        };
        let mut records = Vec::new();
        write(&mut records, message.subject, &compressed, 9, 1024).unwrap();
        let prefix = &compressed[..9];
        assert_eq!(
            receive_message(&mut records.as_slice(), &proposal, prefix, 1024, 1024).unwrap(),
            fixture
        );
        let mut corrupted = prefix.to_vec();
        corrupted[0] ^= 1;
        assert!(
            receive_message(&mut records.as_slice(), &proposal, &corrupted, 1024, 1024).is_err()
        );
        proposal.id = "WRONG";
        assert!(receive_message(&mut records.as_slice(), &proposal, prefix, 1024, 1024).is_err());
    }

    #[test]
    fn rejects_bad_lengths_and_corruption() {
        let mut wire = Vec::new();
        write(&mut wire, "", &[0, 1, 2, 3, 4, 5], 0, 6).unwrap();
        for end in 0..wire.len() {
            assert!(read(&mut &wire[..end], 6, 0, 6).is_err());
        }
        assert!(read(&mut wire.as_slice(), 7, 0, 7).is_err());
        assert!(read(&mut wire.as_slice(), 6, 0, 5).is_err());
        *wire.last_mut().unwrap() ^= 1;
        assert!(read(&mut wire.as_slice(), 6, 0, 6).is_err());
    }
}
