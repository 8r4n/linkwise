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

/// Whitens a complete frame and returns its rate-1/2 turbo-coded bits in
/// channel order. Returns `None` when the supplied frame CRC is invalid.
pub fn encode(frame: &[u8; FRAME_BYTES]) -> Option<[u8; CODED_CELLS]> {
    let (channel, turbo) = permutations();
    coding::encode_frame(frame, &channel, &turbo)
}

/// Reconstructs and checks a frame from channel-order soft decisions.
/// Positive metrics favor 1 and zero denotes an erasure.
pub fn decode(cells: &[f64; CODED_CELLS]) -> Option<[u8; FRAME_BYTES]> {
    let (channel, turbo) = permutations();
    coding::decode_frame(cells, &channel, &turbo)
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
