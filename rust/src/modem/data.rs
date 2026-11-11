//! Uncompressed BW500 DATA containers from published frame-layout observations.
//!
//! The changing full-frame field is supplied by the session owner. This module
//! does not invent its still-unqualified sequencing law. Ambiguous or unsupported
//! short forms are rejected rather than silently trimming possible payload.

use super::{bw500, coding};

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
    full_body(body_size(level)?, payload, field)
}

fn full_body(size: usize, payload: &[u8], field: u8) -> Option<Vec<u8>> {
    if payload.len() != size - 1 || field & 3 != 1 {
        return None;
    }
    let mut body = payload.to_vec();
    body.push(field);
    Some(checksum(body))
}

pub fn short(level: u8, payload: &[u8], caller: &str, next_receive_level: u8) -> Option<Vec<u8>> {
    short_body(body_size(level)?, payload, caller, next_receive_level)
}

fn short_body(
    size: usize,
    payload: &[u8],
    caller: &str,
    next_receive_level: u8,
) -> Option<Vec<u8>> {
    let delimiter_only = size == 44 && payload.len() == 41;
    if (payload.len() > size - 4 && !delimiter_only) || !(1..=4).contains(&next_receive_level) {
        return None;
    }
    // Validate the callsign through the same packed identity contract as setup.
    super::setup::LinkSetup {
        caller: caller.into(),
        high_speed: false,
        requested_level: 4,
    }
    .encode(super::setup::Profile::Bw500)?;
    if delimiter_only {
        let mut body = payload.to_vec();
        body.extend([0x14, next_receive_level, 0x82]);
        return Some(checksum(body));
    }
    let mut body = vec![0; size];
    body[..payload.len()].copy_from_slice(payload);
    body[payload.len()] = 0x14;
    body[payload.len() + 1] = coding::crc(caller.to_ascii_uppercase().as_bytes()).to_be_bytes()[0];
    body[size - 2] = next_receive_level;
    body[size - 1] = 0x82;
    Some(checksum(body))
}

pub fn decode(level: u8, bytes: &[u8], caller: &str) -> Option<Frame> {
    decode_wire(body_size(level)?, bytes, caller)
}

fn decode_wire(size: usize, bytes: &[u8], caller: &str) -> Option<Frame> {
    if bytes.len() != size + 2 || coding::crc(&bytes[..size]).to_be_bytes() != bytes[size..] {
        return None;
    }
    decode_body(&bytes[..size - 1], bytes[size - 1], caller)
}

/// Build one complete level-4 DATA frame for BW2300 or BW2750.
pub fn wide_full(payload: &[u8], field: u8) -> Option<[u8; super::wide_level4::FRAME_BYTES]> {
    full_body(super::wide_level4::FRAME_BYTES - 2, payload, field)?
        .try_into()
        .ok()
}

/// Maximum uncompressed payload in a wide level-4 closing frame.
pub const WIDE_SHORT_BYTES: usize = super::wide_level4::FRAME_BYTES - 6;

/// Build a closing wide level-4 frame carrying up to 86 payload bytes.
pub fn wide_short(payload: &[u8], caller: &str) -> Option<[u8; super::wide_level4::FRAME_BYTES]> {
    short_body(super::wide_level4::FRAME_BYTES - 2, payload, caller, 4)?
        .try_into()
        .ok()
}

/// Interpret a wide level-4 uncompressed DATA container after demodulation.
pub fn decode_wide(bytes: &[u8; super::wide_level4::FRAME_BYTES], caller: &str) -> Option<Frame> {
    decode_wire(super::wide_level4::FRAME_BYTES - 2, bytes, caller)
}

/// Interpret a wide frame body after the waveform decoder verifies the CRC.
pub fn decode_wide_received(body: &[u8], field: u8, caller: &str) -> Option<Frame> {
    if body.len() != super::wide_level4::FRAME_BYTES - 3 {
        return None;
    }
    decode_body(body, field, caller)
}

/// Interpret a DATA frame after the BW500 waveform decoder has verified its CRC.
/// The receiver exposes the body and control byte without the wire checksum.
pub fn decode_received(level: u8, frame: &bw500::Frame, caller: &str) -> Option<Frame> {
    if frame.payload.len() + 1 != body_size(level)? {
        return None;
    }
    decode_body(&frame.payload, frame.control, caller)
}

fn decode_body(body: &[u8], field: u8, caller: &str) -> Option<Frame> {
    if field & 3 == 1 {
        return Some(Frame {
            payload: body.to_vec(),
            field,
            next_receive_level: None,
        });
    }
    let (&next_receive_level, payload) = body.split_last()?;
    // Stock BW500 level 4 can use all 42 payload bytes and the
    // following rate byte without the padded close's callsign marker.
    if field == 0x83 && payload.len() == 42 && (1..=4).contains(&next_receive_level) {
        return Some(Frame {
            payload: payload.to_vec(),
            field,
            next_receive_level: Some(next_receive_level),
        });
    }
    if !matches!(field, 0x42 | 0x82) || !(1..=4).contains(&next_receive_level) {
        return None;
    }
    let key = coding::crc(caller.to_ascii_uppercase().as_bytes()).to_be_bytes()[0];
    let mut endings = payload.windows(2).enumerate().filter(|&(i, trailer)| {
        trailer == [0x14, key] && payload[i + 2..].iter().all(|&value| value == 0)
    });
    let end = match endings.next() {
        Some((end, _)) => end,
        // Stock level 4 uses the final byte before the rate as a
        // delimiter when all 41 payload bytes are occupied.
        None if field == 0x82 && payload.len() == 42 && payload.last() == Some(&0x14) => 41,
        None => return None,
    };
    if endings.next().is_some() {
        return None;
    }
    Some(Frame {
        payload: payload[..end].to_vec(),
        field,
        next_receive_level: Some(next_receive_level),
    })
}

/// Level-4 payload bytes in a full BW500 frame.
pub const FULL_LEVEL4_BYTES: usize = 43;
/// The largest delivery representable by the qualified transmit layouts.
pub const MAX_PLANNED_BYTES: usize = 4 * FULL_LEVEL4_BYTES + 31;

/// Bounded BW500 caller layouts observed against stock VARA. Each plan
/// finishes with a short frame, including an empty close when applicable.
pub struct TransmitPlan {
    full_fields: &'static [u8],
    full_level: u8,
    closing_level: u8,
}

impl TransmitPlan {
    pub fn new(length: usize) -> Option<Self> {
        if (1..=40).contains(&length) {
            return Some(Self {
                full_fields: &[],
                full_level: 4,
                closing_level: 4,
            });
        }
        if length == 84 {
            return Some(Self {
                full_fields: &[0x89],
                full_level: 4,
                closing_level: 4,
            });
        }
        let blocks = length / FULL_LEVEL4_BYTES;
        let remainder = length % FULL_LEVEL4_BYTES;
        if remainder > 31 {
            return None;
        }
        let full_fields: &'static [u8] = match blocks {
            1 => &[0x81],
            2 => &[0x8d, 0x81],
            3 => &[0x91, 0x8d, 0x81],
            4 => &[0x95, 0x91, 0x8d, 0x81],
            _ => return None,
        };
        Some(Self {
            full_fields,
            full_level: 4,
            closing_level: 3,
        })
    }

    /// Select the largest qualified first delivery from a nonempty stream.
    pub fn prefix(length: usize) -> Option<(usize, Self)> {
        (1..=length.min(MAX_PLANNED_BYTES))
            .rev()
            .find_map(|length| Self::new(length).map(|plan| (length, plan)))
    }

    /// Repacketize an entirely unacknowledged delivery after a rate-4 NAK.
    /// The five-full-frame layout was observed on stock VARA, including its
    /// repeated first field and final full frame that retains level 3.
    pub fn level3_retry(length: usize) -> Option<Self> {
        let full_fields: &'static [u8] = match length {
            1..=31 => &[],
            170..=201 => &[0x99, 0x99, 0x95, 0x91, 0x89],
            _ => return None,
        };
        Some(Self {
            full_fields,
            full_level: 3,
            closing_level: 3,
        })
    }

    pub fn full_payload_bytes(&self) -> usize {
        body_size(self.full_level).expect("known DATA record") - 1
    }

    pub fn full_frames(&self) -> usize {
        self.full_fields.len()
    }

    pub fn encode(&self, index: usize, bytes: &[u8], caller: &str) -> Option<(u8, Vec<u8>)> {
        let block = self.full_payload_bytes();
        let start = index.checked_mul(block)?;
        if let Some(&field) = self.full_fields.get(index) {
            let payload = bytes.get(start..start.checked_add(block)?)?;
            return Some((self.full_level, full(self.full_level, payload, field)?));
        }
        if index != self.full_fields.len() {
            return None;
        }
        Some((
            self.closing_level,
            short(self.closing_level, bytes.get(start..)?, caller, 4)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_stock_bw500_level4_unpadded_close() {
        let payload = b"N6MQS ()\rFD EM YB4CEFWBEHGH 21307 21037 0\r";
        let mut body = payload.to_vec();
        body.extend([4, 0x83]);
        let frame = decode(4, &checksum(body), "KN6MQS").unwrap();
        assert_eq!(frame.payload, payload);
        assert_eq!(frame.next_receive_level, Some(4));
    }

    #[test]
    fn decodes_stock_bw500_level4_delimiter_only_close() {
        let payload = b"6MQS ()\rFD EM L5TFGLOYFTZM 21308 21039 0\r";
        let mut wire = payload.to_vec();
        wire.extend([0x14, 4, 0x82, 0x20, 0xa9]);
        let frame = decode(4, &wire, "KN6MQS").unwrap();
        assert_eq!(short(4, payload, "KN6MQS", 4).unwrap(), wire);
        assert_eq!(frame.payload, payload);
        assert_eq!(frame.next_receive_level, Some(4));
    }

    #[test]
    fn transmit_plan_preserves_exact_payload_and_rejects_unqualified_lengths() {
        for len in [0, 41, 42, 75, 85, 204, 215] {
            assert!(TransmitPlan::new(len).is_none(), "{len}");
        }
        for len in [1, 40, 43, 74, 84, 86, 93, 129, 172, 200, 203] {
            let payload: Vec<_> = (0..len as u8).collect();
            let plan = TransmitPlan::new(len).unwrap();
            let mut received = Vec::new();
            for index in 0..=plan.full_frames() {
                let (level, wire) = plan.encode(index, &payload, "W1AW").unwrap();
                let frame = decode(level, &wire, "W1AW").unwrap();
                assert_eq!(
                    frame.next_receive_level.is_some(),
                    index == plan.full_frames()
                );
                received.extend(frame.payload);
            }
            assert_eq!(received, payload);
        }
    }

    #[test]
    fn accepts_stock_short_frame_with_0x42_control() {
        // Windows VARA HF 4.9.1, Ubuntu responder capture.
        let payload: Vec<_> = (172..200).collect();
        let mut wire = short(4, &payload, "KN6MQS", 4).unwrap();
        wire.truncate(44);
        wire[43] = 0x42;
        let decoded = decode(4, &checksum(wire), "KN6MQS").unwrap();
        assert_eq!(decoded.payload, payload);
        assert_eq!(decoded.field, 0x42);
        assert_eq!(decoded.next_receive_level, Some(4));
    }

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
        let received_frame = bw500::Frame {
            payload: wire[..34].to_vec(),
            control: wire[34],
        };
        assert_eq!(
            decode_received(3, &received_frame, "W9SSJ"),
            Some(Frame {
                payload: input[172..].to_vec(),
                field: 0x82,
                next_receive_level: Some(4),
            })
        );
        assert!(decode_received(4, &received_frame, "W9SSJ").is_none());
        received.extend(close.payload);
        assert_eq!(received, input);
    }

    #[test]
    fn rejects_corruption_wrong_session_and_unsupported_short_forms() {
        let mut wire = short(4, b"message", "W9SSJ", 4).unwrap();
        assert!(decode(4, &wire, "W1AW").is_none());
        wire[0] ^= 1;
        assert!(decode(4, &wire, "W9SSJ").is_none());
        assert!(short(4, &[0; 42], "W9SSJ", 4).is_none());
        assert!(full(4, &[0; 43], 0x82).is_none());
        assert!(short(4, b"x", "not_a_call", 4).is_none());
    }
}
