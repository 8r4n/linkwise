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
    let unpadded = size == 44 && payload.len() == 42;
    if (payload.len() > size - 4 && !delimiter_only && !unpadded)
        || !(1..=4).contains(&next_receive_level)
    {
        return None;
    }
    // Validate the callsign through the same packed identity contract as setup.
    super::setup::LinkSetup {
        caller: caller.into(),
        high_speed: false,
        requested_level: 4,
    }
    .encode(super::setup::Profile::Bw500)?;
    if unpadded {
        let mut body = payload.to_vec();
        body.extend([next_receive_level, 0x83]);
        return Some(checksum(body));
    }
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

pub const WIDE_FULL_BYTES: usize = super::wide_level4::FRAME_BYTES - 3;
pub const WIDE_LOWER_FULL_BYTES: usize = super::wide_level4::LOWER_FRAME_BYTES - 3;

pub const WIDE_ROBUST_FULL_BYTES: usize = super::wide_level4::ROBUST_FRAME_BYTES - 3;

pub fn wide_lower_delivery_frame<const FRAME_BYTES: usize>(
    index: usize,
    bytes: &[u8],
    caller: &str,
) -> Option<[u8; FRAME_BYTES]> {
    let size = FRAME_BYTES - 2;
    let block = size - 1;
    let start = index.checked_mul(block)?;
    let remainder = bytes.get(start..)?;
    if remainder.len() >= block {
        let field = if block == WIDE_ROBUST_FULL_BYTES && remainder.len() >= block + WIDE_FULL_BYTES
        {
            0x95
        } else {
            0x89
        };
        full_body(size, &remainder[..block], field)?
    } else {
        short_body(size, remainder, caller, 4)?
    }
    .try_into()
    .ok()
}

pub fn wide_lower_fits(length: usize, block: usize) -> bool {
    length % block <= block - 3
}

/// Limit a fixed-level over to two full blocks and one closing block.
pub fn wide_delivery_prefix(length: usize) -> usize {
    let length = length.min(2 * WIDE_FULL_BYTES + WIDE_SHORT_BYTES);
    let blocks = length / WIDE_FULL_BYTES;
    blocks * WIDE_FULL_BYTES + (length % WIDE_FULL_BYTES).min(WIDE_SHORT_BYTES)
}

pub fn wide_delivery_frame(
    index: usize,
    bytes: &[u8],
    caller: &str,
) -> Option<[u8; super::wide_level4::FRAME_BYTES]> {
    let fields: &[u8] = match bytes.len() / WIDE_FULL_BYTES {
        0 => &[],
        1 => &[0x89],
        2 => &[0x8d, 0x89],
        _ => return None,
    };
    let start = index.checked_mul(WIDE_FULL_BYTES)?;
    if let Some(&field) = fields.get(index) {
        return wide_full(bytes.get(start..start + WIDE_FULL_BYTES)?, field);
    }
    if index == fields.len() {
        return wide_short(bytes.get(start..)?, caller);
    }
    None
}

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
    if !matches!(
        body.len(),
        WIDE_FULL_BYTES | WIDE_LOWER_FULL_BYTES | WIDE_ROBUST_FULL_BYTES
    ) {
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
    // Stock BW500 can fill the closing block at levels 3 and 4,
    // leaving only the rate byte before the 0x83 control.
    if field == 0x83 && matches!(payload.len(), 33 | 42) && (1..=4).contains(&next_receive_level) {
        return Some(Frame {
            payload: payload.to_vec(),
            field,
            next_receive_level: Some(next_receive_level),
        });
    }
    let stock_bw500_close = field == 0x02 && body.len() == 43;
    if !(stock_bw500_close || matches!(field, 0x42 | 0x82))
        || !(1..=4).contains(&next_receive_level)
    {
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
        if (1..=42).contains(&length) {
            return Some(Self {
                full_fields: &[],
                full_level: 4,
                closing_level: 4,
            });
        }
        if (75..=85).contains(&length) {
            return Some(Self {
                full_fields: &[0x89],
                full_level: 4,
                closing_level: 4,
            });
        }
        if length == MAX_PLANNED_BYTES {
            return Some(Self {
                full_fields: &[0x95, 0x91, 0x8d, 0x89],
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
            34..=65 => &[0x89],
            68..=99 => &[0x8d, 0x89],
            102..=133 => &[0x91, 0x8d, 0x89],
            136..=167 => &[0x95, 0x91, 0x8d, 0x89],
            170..=201 => &[0x99, 0x99, 0x95, 0x91, 0x89],
            _ => return None,
        };
        Some(Self {
            full_fields,
            full_level: 3,
            closing_level: 3,
        })
    }

    /// Retry only bytes beyond the acknowledged level-4 prefix.
    pub fn level3_retry_after_prefix(&self, index: usize, length: usize) -> Option<(usize, Self)> {
        if self.full_level != 4 || index == 0 {
            return None;
        }
        let acknowledged = index.checked_mul(self.full_payload_bytes())?;
        if index != 1 || length != 200 {
            return Some((
                acknowledged,
                Self::level3_retry(length.checked_sub(acknowledged)?)?,
            ));
        }
        Some((
            acknowledged,
            Self {
                full_fields: &[0x95, 0x91, 0x91, 0x81],
                full_level: 3,
                closing_level: 3,
            },
        ))
    }

    /// Resume at level 4 after a rate-increase acknowledgment of the
    /// first level-3 retry block.
    pub fn level4_retry_continuation(&self, index: usize, length: usize) -> Option<(usize, Self)> {
        if self.full_level != 3 || index != 0 {
            return None;
        }
        let full_fields: &'static [u8] = match length {
            200 => &[0x91, 0x8d, 0x89],
            157 => &[0x8d, 0x89],
            _ => {
                return Some((
                    self.full_payload_bytes(),
                    Self::new(length.checked_sub(self.full_payload_bytes())?)?,
                ));
            }
        };
        Some((
            self.full_payload_bytes(),
            Self {
                full_fields,
                full_level: 4,
                closing_level: 4,
            },
        ))
    }

    pub fn full_payload_bytes(&self) -> usize {
        body_size(self.full_level).expect("known DATA record") - 1
    }

    pub fn full_frames(&self) -> usize {
        self.full_fields.len()
    }

    pub fn closes_at_level3(&self) -> bool {
        self.closing_level == 3
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
    fn wide_level4_delivery_preserves_bytes_across_frame_boundaries() {
        for length in [1, 85, 86, 87, 88, 89, 90, 175, 176, 177, 178, 203, 264, 265] {
            let payload: Vec<_> = (0..length).map(|index| index as u8).collect();
            let mut pending = payload.as_slice();
            let mut received = Vec::new();
            while !pending.is_empty() {
                let size = wide_delivery_prefix(pending.len());
                let frames = size / WIDE_FULL_BYTES;
                for index in 0..=frames {
                    let wire = wide_delivery_frame(index, &pending[..size], "KN6MQS").unwrap();
                    let frame = decode_wide(&wire, "KN6MQS").unwrap();
                    assert_eq!(frame.next_receive_level, (index == frames).then_some(4));
                    received.extend(frame.payload);
                }
                pending = &pending[size..];
            }
            assert_eq!(received, payload, "payload length {length}");
        }
    }

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
    fn decodes_stock_bw500_level3_unpadded_close() {
        let payload = [
            0xb9, 0x7b, 0xef, 0xfe, 0x83, 0x87, 0xec, 0xd1, 0x63, 0xfe, 0xe4, 0xe9, 0xb3, 0xe7,
            0x2f, 0x5e, 0xbe, 0x7a, 0xfd, 0xe6, 0x2d, 0x67, 0x7f, 0x06, 0, 0xb5, 0x55, 0x10, 0x10,
            0xb8, 0x01, 0, 0,
        ];
        let mut body = payload.to_vec();
        body.extend([4, 0x83]);
        let frame = decode(3, &checksum(body), "KN6MQS").unwrap();
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
        for len in [0, 204, 215] {
            assert!(TransmitPlan::new(len).is_none(), "{len}");
        }
        for len in [
            1, 40, 41, 42, 43, 74, 75, 80, 84, 85, 86, 93, 129, 172, 200, 203,
        ] {
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
    fn accepts_stock_bw500_express_end_of_proposals() {
        // The stock caller sent this after Express stored the P2P message.
        let mut payload = vec![0; 43];
        payload[..5].copy_from_slice(&[b'F', b'F', b'\r', 0x14, 0x85]);
        payload[42] = 4;
        let frame = bw500::Frame {
            payload,
            control: 0x02,
        };
        let decoded = decode_received(4, &frame, "KN6MQS").unwrap();
        assert_eq!(decoded.payload, b"FF\r");
        assert_eq!(decoded.next_receive_level, Some(4));
        assert!(decode_received(4, &frame, "W1A").is_none());
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
        assert!(short(4, &[0; 43], "W9SSJ", 4).is_none());
        assert!(full(4, &[0; 43], 0x82).is_none());
        assert!(short(4, b"x", "not_a_call", 4).is_none());
    }
}
