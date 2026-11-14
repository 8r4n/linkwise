//! Winlink B2 LZHUF compression with a 2 KiB dictionary and CRC-16/XMODEM.

mod huffman;

use std::io::Result;

use super::invalid;
use huffman::Tree;

const WINDOW: usize = 2048;
const MAX_MATCH: usize = 60;
const MIN_MATCH: usize = 3;
const HEADER: usize = 6;
const NONE: usize = usize::MAX;
const HASH_BUCKETS: usize = 4096;
const SEARCH_LIMIT: usize = 64;

/// Compress a payload into the Winlink B2 wire format within the supplied limit.
pub fn encode(input: &[u8], max_compressed_bytes: usize) -> Result<Vec<u8>> {
    let size = u32::try_from(input.len()).map_err(|_| invalid("B2 payload exceeds size field"))?;
    if max_compressed_bytes < HEADER {
        return Err(invalid("B2 compressed payload exceeds configured limit"));
    }
    let mut output = BitsOut {
        data: vec![0; HEADER],
        used: 0,
        limit: max_compressed_bytes,
        overflow: false,
    };
    output.data[2..HEADER].copy_from_slice(&size.to_le_bytes());
    let mut tree = Tree::new();
    let mut dictionary = MatchFinder {
        heads: vec![NONE; HASH_BUCKETS],
        previous: vec![NONE; WINDOW],
    };
    let mut cursor = 0;
    while cursor < input.len() {
        let (distance, count) = dictionary.longest(input, cursor);
        let symbol = if count >= MIN_MATCH {
            256 + count - MIN_MATCH
        } else {
            usize::from(input[cursor])
        };
        tree.encode(symbol, |bit| output.bit(bit));
        if count >= MIN_MATCH {
            let position = distance - 1;
            let (code, width) = prefix(position >> 6);
            output.bits(code, width);
            output.bits(position & 63, 6);
        }
        if output.overflow {
            return Err(invalid("B2 compressed payload exceeds configured limit"));
        }
        let end = cursor + if count >= MIN_MATCH { count } else { 1 };
        for index in cursor..end {
            dictionary.insert(input, index);
        }
        cursor = end;
    }
    let crc = crc16(&output.data[2..]);
    output.data[..2].copy_from_slice(&crc.to_le_bytes());
    Ok(output.data)
}

/// Decompress only if the wire size agrees with the accepted B2 proposal.
/// The caller must apply its size limits before accepting that proposal.
pub fn decode(input: &[u8], expected_bytes: usize) -> Result<Vec<u8>> {
    let Some((header, compressed)) = input.split_at_checked(HEADER) else {
        return Err(invalid("truncated B2 compression header"));
    };
    let expected_crc = u16::from_le_bytes([header[0], header[1]]);
    let size = u32::from_le_bytes([header[2], header[3], header[4], header[5]]) as usize;
    if size != expected_bytes || crc16(&input[2..]) != expected_crc {
        return Err(invalid("invalid B2 compression size or CRC"));
    }
    let mut tree = Tree::new();
    let mut bits = BitsIn {
        data: compressed,
        offset: 0,
    };
    // Build output incrementally so a short invalid stream cannot trigger
    // a large allocation based solely on the declared uncompressed size.
    let mut decoded = Vec::new();
    while decoded.len() < size {
        let symbol = tree.decode(|| bits.bit())?;
        if symbol < 256 {
            decoded.push(symbol as u8);
        } else {
            let count = symbol - 256 + MIN_MATCH;
            if count > size - decoded.len() {
                return Err(invalid("B2 match exceeds declared output size"));
            }
            let distance = bits.distance()?;
            for _ in 0..count {
                let value = match decoded.len().checked_sub(distance) {
                    Some(index) => decoded[index],
                    None if distance - decoded.len() <= WINDOW - MAX_MATCH => b' ',
                    None => 0,
                };
                decoded.push(value);
            }
        }
    }
    let consumed = bits.offset.div_ceil(8);
    if consumed != compressed.len()
        || (bits.offset % 8 != 0 && (compressed[consumed - 1] << (bits.offset % 8)) != 0)
    {
        return Err(invalid("trailing data in B2 compressed payload"));
    }
    Ok(decoded)
}

struct MatchFinder {
    heads: Vec<usize>,
    previous: Vec<usize>,
}

impl MatchFinder {
    fn insert(&mut self, data: &[u8], at: usize) {
        if let Some(bucket) = bucket(data, at) {
            self.previous[at % WINDOW] = self.heads[bucket];
            self.heads[bucket] = at;
        }
    }

    fn longest(&self, data: &[u8], at: usize) -> (usize, usize) {
        let Some(bucket) = bucket(data, at) else {
            return (0, 0);
        };
        let mut candidate = self.heads[bucket];
        let limit = MAX_MATCH.min(data.len() - at);
        let mut best = (0, 0);
        for _ in 0..SEARCH_LIMIT {
            if candidate == NONE || at - candidate > WINDOW {
                break;
            }
            let count = data[at..at + limit]
                .iter()
                .zip(&data[candidate..])
                .take_while(|(a, b)| a == b)
                .count();
            if count > best.1 {
                best = (at - candidate, count);
            }
            if count == limit {
                break;
            }
            candidate = self.previous[candidate % WINDOW];
        }
        best
    }
}

fn bucket(data: &[u8], at: usize) -> Option<usize> {
    let bytes = data.get(at..at + MIN_MATCH)?;
    let hash = bytes
        .iter()
        .fold(0, |hash, &byte| hash * 31 + usize::from(byte));
    Some(hash % HASH_BUCKETS)
}

// Canonical prefix for the upper position bits. Only the first 32 codes
// occur with a 2 KiB dictionary.
fn prefix(high: usize) -> (usize, usize) {
    match high {
        0 => (0, 3),
        1..=3 => (high + 1, 4),
        4..=11 => (high + 6, 5),
        12..=23 => (high + 24, 6),
        _ => (high + 72, 7),
    }
}

struct BitsOut {
    data: Vec<u8>,
    used: usize,
    limit: usize,
    overflow: bool,
}

impl BitsOut {
    fn bit(&mut self, value: u8) {
        if self.used == 0 {
            if self.data.len() == self.limit {
                self.overflow = true;
                return;
            }
            self.data.push(0);
        }
        *self.data.last_mut().expect("output has a header") |= value << (7 - self.used);
        self.used = (self.used + 1) % 8;
    }

    fn bits(&mut self, value: usize, count: usize) {
        for bit in (0..count).rev() {
            self.bit(((value >> bit) & 1) as u8);
        }
    }
}

struct BitsIn<'a> {
    data: &'a [u8],
    offset: usize,
}

impl BitsIn<'_> {
    fn bit(&mut self) -> Result<u8> {
        let byte = self
            .data
            .get(self.offset / 8)
            .ok_or_else(|| invalid("truncated B2 bitstream"))?;
        let value = (byte >> (7 - self.offset % 8)) & 1;
        self.offset += 1;
        Ok(value)
    }

    fn distance(&mut self) -> Result<usize> {
        let mut code = 0;
        for width in 1..=7 {
            code = (code << 1) | usize::from(self.bit()?);
            if let Some(high) = (0..WINDOW / 64).find(|&high| prefix(high) == (code, width)) {
                let mut position = high;
                for _ in 0..6 {
                    position = (position << 1) | usize::from(self.bit()?);
                }
                return Ok(position + 1);
            }
        }
        Err(invalid("invalid B2 dictionary distance"))
    }
}

fn crc16(input: &[u8]) -> u16 {
    let mut crc = 0;
    for &byte in input {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::winlink::Message;

    // Generated using wl2k-go v1.0.1 NewB2Writer from MESSAGE.
    const REFERENCE: &[u8] = &[
        0x7d, 0x0d, 0x8f, 0x00, 0x00, 0x00, 0xec, 0xfd, 0x7e, 0x1c, 0x6d, 0x67, 0x83, 0xa3, 0xdf,
        0xc0, 0xde, 0xef, 0xb7, 0xf9, 0x9c, 0xb7, 0x43, 0xda, 0x01, 0xf1, 0xc2, 0xc1, 0xbc, 0xde,
        0x56, 0xe1, 0x6e, 0xef, 0x6a, 0x58, 0x57, 0xbb, 0xa9, 0x45, 0x9a, 0xb6, 0x93, 0x6e, 0xae,
        0x67, 0x82, 0xff, 0x23, 0x02, 0xfd, 0xcf, 0xf6, 0x20, 0x15, 0x18, 0x82, 0x54, 0xae, 0x95,
        0x0f, 0xef, 0xf3, 0x56, 0xb7, 0x8e, 0x5e, 0x6c, 0xc4, 0x80, 0xd4, 0xd4, 0x0f, 0xf6, 0xaa,
        0x73, 0xe5, 0x7b, 0x11, 0xd3, 0xe7, 0x5f, 0x00, 0xfb, 0xbe, 0xd6, 0xbe, 0xfb, 0x68, 0x1f,
        0xe7, 0x37, 0xfd, 0x47, 0x7d, 0x16, 0x59, 0xfd, 0xf6, 0x1d, 0x2c, 0x30, 0x70, 0x25, 0x8c,
        0x1b, 0xfe, 0x13, 0x25, 0xde, 0x52, 0x06, 0xd1, 0xbf, 0xee, 0xf7, 0x52, 0x0f, 0xa2, 0x99,
        0x61, 0xc6, 0xfa, 0x87, 0x58, 0x40, 0xb8, 0xcc, 0x6e, 0x3b, 0x1f, 0x90, 0xc8, 0xe4, 0xb2,
        0x79, 0x4c, 0xab, 0x79, 0x77, 0x06,
    ];
    const MESSAGE: &[u8] = b"Mid: TEST123\r\nDate: 2026/11/13 12:00\r\nType: Private\r\nFrom: W1AW\r\nTo: N0CALL\r\nSubject: Bench\r\nBody: 4\r\nFile: 12 test.bin\r\n\r\nHi\r\n\r\n\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\r\n";

    #[test]
    fn independent_message_fixture_preserves_attachment() {
        let plain = decode(REFERENCE, MESSAGE.len()).unwrap();
        assert_eq!(plain, MESSAGE);
        let message = Message::decode(&plain, MESSAGE.len()).unwrap();
        assert_eq!(message.attachments[0].bytes, (0..12).collect::<Vec<_>>());
        let (size, wire) = message.encode_compressed(MESSAGE.len(), 1024).unwrap();
        assert_eq!(size, MESSAGE.len());
        assert_eq!(decode(&wire, size).unwrap(), MESSAGE);
        assert!(message.encode_compressed(MESSAGE.len() - 1, 1024).is_err());
        assert!(
            message
                .encode_compressed(MESSAGE.len(), wire.len() - 1)
                .is_err()
        );
    }

    #[test]
    fn round_trips_through_tree_rebuild_and_dictionary_wrap() {
        let mut state = 0x42_u32;
        let mut data = (0..100_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect::<Vec<_>>();
        data.extend_from_within(99_000..);
        let wire = encode(&data, 110_000).unwrap();
        assert_eq!(decode(&wire, data.len()).unwrap(), data);
        let repeated = vec![b'x'; 4096];
        let wire = encode(&repeated, 256).unwrap();
        assert_eq!(decode(&wire, repeated.len()).unwrap(), repeated);
        assert_eq!(encode(&[], HEADER).unwrap(), [0; HEADER]);
    }

    #[test]
    fn rejects_bad_size_checksum_and_trailing_data() {
        assert_eq!(crc16(b"123456789"), 0x31c3);
        assert!(decode(REFERENCE, MESSAGE.len() + 1).is_err());
        for end in 0..REFERENCE.len() {
            assert!(decode(&REFERENCE[..end], MESSAGE.len()).is_err());
        }
        let mut corrupted = REFERENCE.to_vec();
        corrupted[20] ^= 1;
        assert!(decode(&corrupted, MESSAGE.len()).is_err());
        let mut trailing = REFERENCE.to_vec();
        trailing.push(0);
        let crc = crc16(&trailing[2..]);
        trailing[..2].copy_from_slice(&crc.to_le_bytes());
        assert!(decode(&trailing, MESSAGE.len()).is_err());
    }
}
