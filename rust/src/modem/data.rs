//! Uncompressed BW500 DATA containers from published frame-layout observations.
//!
//! The changing full-frame field is supplied by the session owner. This module
//! does not invent its still-unqualified sequencing law. Ambiguous or unsupported
//! short forms are rejected rather than silently trimming possible payload.

use super::coding;

#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    pub payload: Vec<u8>,
    pub field: u8,
    pub next_receive_level: Option<u8>,
}

fn body_size(level: u8) -> Option<usize> {
    match level {
        1 => Some(10),
        2 => Some(23),
        3 => Some(35),
        4 => Some(44),
        _ => None,
    }
}

fn checksum(mut body: Vec<u8>) -> Vec<u8> {
    body.extend(coding::crc(&body).to_be_bytes());
    body
}

pub fn full(level: u8, payload: &[u8], field: u8) -> Option<Vec<u8>> {
    let size = body_size(level)?;
    if payload.len() != size - 1 || field & 3 != 1 {
        return None;
    }
    let mut body = payload.to_vec();
    body.push(field);
    Some(checksum(body))
}

pub fn short(level: u8, payload: &[u8], caller: &str, next_receive_level: u8) -> Option<Vec<u8>> {
    let size = body_size(level)?;
    if payload.len() > size - 4 || !(1..=4).contains(&next_receive_level) {
        return None;
    }
    // Validate the callsign through the same packed identity contract as setup.
    super::setup::LinkSetup {
        caller: caller.into(),
        high_speed: false,
        requested_level: 4,
    }
    .encode(super::setup::Profile::Bw500)?;
    let mut body = vec![0; size];
    body[..payload.len()].copy_from_slice(payload);
    body[payload.len()] = 0x14;
    body[payload.len() + 1] = coding::crc(caller.to_ascii_uppercase().as_bytes()).to_be_bytes()[0];
    body[size - 2] = next_receive_level;
    body[size - 1] = 0x82;
    Some(checksum(body))
}

pub fn decode(level: u8, bytes: &[u8], caller: &str) -> Option<Frame> {
    let size = body_size(level)?;
    if bytes.len() != size + 2 || coding::crc(&bytes[..size]).to_be_bytes() != bytes[size..] {
        return None;
    }
    let field = bytes[size - 1];
    if field & 3 == 1 {
        return Some(Frame {
            payload: bytes[..size - 1].to_vec(),
            field,
            next_receive_level: None,
        });
    }
    if field != 0x82 || !(1..=4).contains(&bytes[size - 2]) {
        return None;
    }
    let key = coding::crc(caller.to_ascii_uppercase().as_bytes()).to_be_bytes()[0];
    let mut endings = (0..=size - 4).filter(|&i| {
        bytes[i] == 0x14
            && bytes[i + 1] == key
            && bytes[i + 2..size - 2].iter().all(|&value| value == 0)
    });
    let end = endings.next()?;
    if endings.next().is_some() {
        return None;
    }
    Some(Frame {
        payload: bytes[..end].to_vec(),
        field,
        next_receive_level: Some(bytes[size - 2]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reassembles_the_documented_200_byte_delivery_without_padding() {
        let input: Vec<u8> = (0..200).collect();
        let mut received = Vec::new();
        for (index, field) in [0x95, 0x91, 0x8d, 0x81].into_iter().enumerate() {
            let wire = full(4, &input[index * 43..(index + 1) * 43], field).unwrap();
            received.extend(decode(4, &wire, "W9SSJ").unwrap().payload);
        }
        let wire = short(3, &input[172..], "W9SSJ", 4).unwrap();
        assert_eq!(&wire[28..35], &[0x14, 0xf6, 0, 0, 0, 4, 0x82]);
        let close = decode(3, &wire, "W9SSJ").unwrap();
        assert_eq!(close.next_receive_level, Some(4));
        received.extend(close.payload);
        assert_eq!(received, input);
    }

    #[test]
    fn rejects_corruption_wrong_session_and_unsupported_short_forms() {
        let mut wire = short(4, b"message", "W9SSJ", 4).unwrap();
        assert!(decode(4, &wire, "W1AW").is_none());
        wire[0] ^= 1;
        assert!(decode(4, &wire, "W9SSJ").is_none());
        assert!(short(4, &[0; 41], "W9SSJ", 4).is_none());
        assert!(full(4, &[0; 43], 0x82).is_none());
        assert!(short(4, b"x", "not_a_call", 4).is_none());
    }
}
