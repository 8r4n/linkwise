//! BW500 level-4 DATA channel coding, following §3.6 of the published VARA HF protocol notes.
//!
//! This module operates at the coded-cell boundary. The differential two-band
//! waveform, reference grid and timing recovery are separate.

use super::coding::{self, Random24};

pub const FRAME_BYTES: usize = 46;
pub const CODED_CELLS: usize = 748;
const INFORMATION_BITS: usize = FRAME_BYTES * 8;
const PERMUTATION_SEED: u32 = 6_437_280;

fn permutations() -> (Vec<usize>, Vec<usize>) {
    let mut random = Random24(PERMUTATION_SEED);
    let channel = random.permutation(CODED_CELLS);
    let turbo = random.permutation(INFORMATION_BITS);
    (channel, turbo)
}

fn unpack(bits: &[u8]) -> Option<[u8; FRAME_BYTES]> {
    let mut information = bits.to_vec();
    coding::whiten(&mut information);
    let mut frame = [0; FRAME_BYTES];
    for (byte, bits) in frame.iter_mut().zip(information.as_chunks::<8>().0) {
        *byte = bits.iter().fold(0, |value, bit| (value << 1) | bit);
    }
    let (body, checksum) = frame.split_at(FRAME_BYTES - 2);
    (coding::crc(body).to_be_bytes() == checksum).then_some(frame)
}

/// Whitens a complete frame and returns its rate-1/2 turbo-coded bits in
/// channel order. Returns `None` when the supplied frame CRC is invalid.
pub fn encode(frame: &[u8; FRAME_BYTES]) -> Option<[u8; CODED_CELLS]> {
    let (body, checksum) = frame.split_at(FRAME_BYTES - 2);
    if coding::crc(body).to_be_bytes() != checksum {
        return None;
    }
    let (channel, turbo) = permutations();
    let mut information: Vec<_> = frame
        .iter()
        .flat_map(|byte| (0..8).rev().map(move |shift| (byte >> shift) & 1))
        .collect();
    coding::whiten(&mut information);
    let coded = coding::encode(&information, &turbo, false);
    Some(std::array::from_fn(|position| coded[channel[position]]))
}

/// Reconstructs and checks a frame from channel-order soft decisions.
/// Positive metrics favor 1 and zero denotes an erasure.
pub fn decode(cells: &[f64; CODED_CELLS]) -> Option<[u8; FRAME_BYTES]> {
    if cells.iter().any(|cell| !cell.is_finite()) {
        return None;
    }
    let (channel, turbo) = permutations();
    let mut coded = [0.0; CODED_CELLS];
    for (position, &cell) in channel.iter().zip(cells) {
        coded[*position] = cell;
    }
    coding::decode(&coded, &turbo, false, unpack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_documented_bw500_setup_frame() {
        let mut frame = [0; FRAME_BYTES];
        frame[..10].copy_from_slice(&[0x5e, 0x44, 0xd3, 0x28, 0, 0, 0, 0x80, 0x14, 0xf6]);
        frame[42..].copy_from_slice(&[0x04, 0x82, 0x1d, 0xee]);
        let bits = encode(&frame).unwrap();
        let cells = bits.map(|bit| f64::from(bit) * 2.0 - 1.0);
        assert_eq!(decode(&cells), Some(frame));
        assert_eq!(decode(&[0.0; CODED_CELLS]), None);
        frame[0] ^= 0x80;
        assert_eq!(encode(&frame), None);
    }
}
