//! Callsign-keyed single-tone VARA HF control waveforms.

use std::f64::consts::PI;

use super::{
    coding::{self, Random24},
    waveform::{Tone, energy},
};

const SYMBOL_SAMPLES: usize = 2048;
const REQUEST_PREAMBLE: &[u8] = &[74, 68, 70, 60, 60, 77, 50, 76, 78, 74];
const RESPONSE_PREAMBLE: &[u8] = &[62, 67, 55, 66, 59, 72, 68, 55];

#[derive(Clone, Copy, Debug)]
pub enum Bandwidth {
    Hz500,
    Hz2300,
    Hz2750,
}

#[derive(Clone, Copy, Debug)]
pub enum Message {
    ConnectRequest(Bandwidth),
    ConnectResponse(Bandwidth),
    /// Observed caller recovery burst in a stock BW500 session.
    Bw500Recovery,
    Bw500Confirm,
}

impl Message {
    /// Returns the 48 kHz / 2048-point bin sequence for a dialed callsign.
    pub fn tones(self, called: &str) -> Option<Vec<u8>> {
        use Bandwidth::*;
        let (head, count, seed_offset, preadvance, base, coarse_bins, alternate) = match self {
            Self::ConnectRequest(Hz500) => (REQUEST_PREAMBLE, 31, 57, 1, 50, 2, false),
            Self::ConnectRequest(Hz2300) => (REQUEST_PREAMBLE, 31, 50, 1, 29, 5, true),
            Self::ConnectRequest(Hz2750) => (REQUEST_PREAMBLE, 31, 58, 1, 29, 5, true),
            Self::ConnectResponse(Hz500) => (RESPONSE_PREAMBLE, 15, 289, 721, 50, 2, false),
            Self::ConnectResponse(Hz2300) => (RESPONSE_PREAMBLE, 15, 289, 511, 29, 5, true),
            Self::ConnectResponse(Hz2750) => (RESPONSE_PREAMBLE, 15, 579, 91, 22, 6, true),
            Self::Bw500Recovery => (&[74][..], 31, 60, 1117, 50, 2, false),
            Self::Bw500Confirm => (&[62][..], 15, 1551, 91, 50, 2, false),
        };
        let mut random = keyed_stream(called, seed_offset, preadvance)?;
        let mut tones = head.to_vec();
        for index in 0..count {
            let coarse = (random.advance() * coarse_bins) >> 24;
            let fine = (random.advance() * 7) >> 24;
            let parity = u32::from(alternate && index % 2 == 0);
            tones.push((base + parity + 14 * coarse + 2 * fine) as u8);
        }
        Some(tones)
    }

    /// Renders one control burst as normalized 48 kHz mono audio for bench tests.
    pub fn synthesize(self, called: &str) -> Option<Vec<f32>> {
        let bins = self.tones(called)?;
        Some(render(bins.into_iter().map(|bin| [bin])))
    }

    /// Searches for an expected burst near the estimated start of a 48 kHz
    /// bench recording. The caller supplies the expected message and callsign.
    pub fn find(self, called: &str, audio: &[f32], estimated_start: usize) -> Option<usize> {
        let tones: Vec<_> = self
            .tones(called)?
            .into_iter()
            .map(|bin| Tone::new(usize::from(bin), SYMBOL_SAMPLES))
            .collect();
        let length = tones.len() * SYMBOL_SAMPLES;
        let last = audio.len().checked_sub(length)?;
        let from = estimated_start.saturating_sub(SYMBOL_SAMPLES);
        let through = estimated_start.saturating_add(SYMBOL_SAMPLES).min(last);
        let score = |start| {
            tones
                .iter()
                .zip(audio[start..start + length].as_chunks::<SYMBOL_SAMPLES>().0)
                .map(|(tone, column)| {
                    let norm = energy(column) * SYMBOL_SAMPLES as f64 / 2.0;
                    tone.power(column) / norm.max(f64::EPSILON)
                })
                .fold((0.0, 1.0_f64), |(sum, weakest), score| {
                    (sum + score, weakest.min(score))
                })
        };
        let (start, (sum, weakest)) = (from..=through)
            .step_by(64)
            .map(|start| (start, score(start)))
            .max_by(|a, b| a.1.0.total_cmp(&b.1.0))?;
        (sum >= 0.80 * tones.len() as f64 && weakest >= 0.30).then_some(start)
    }
}

fn keyed_stream(called: &str, seed_offset: u32, preadvance: u32) -> Option<Random24> {
    if called.is_empty()
        || !called
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'/'))
    {
        return None;
    }
    let called = called.to_ascii_uppercase();
    let crc = u32::from(coding::crc(called.as_bytes()));
    let seed = (crc + seed_offset) & 0x7fff;
    let hi = (f64::from(seed).to_bits() >> 32) as u32;
    let folded = ((hi & 0xffff) << 8) ^ ((hi >> 8) & 0xffff00);
    let mut random = Random24((folded & 0xffff00) | 0x86);

    const BASIS: [[u32; 6]; 3] = [
        [32, 64, 128, 258, 36, 216],
        [102, 204, 408, 274, 6, 22],
        [110, 220, 440, 338, 134, 310],
    ];
    let mut hash = 254;
    for (byte, basis) in called.bytes().rev().zip(BASIS) {
        for (index, term) in basis.into_iter().enumerate() {
            if ((byte ^ b'A') >> index) & 1 != 0 {
                hash ^= term;
            }
        }
    }
    for _ in 0..preadvance + hash + ((crc + 50) >> 15) {
        random.advance();
    }

    Some(random)
}

/// Builds the eleven-symbol response to link setup, keyed to the caller.
/// Both tones of each symbol are returned in ascending bin order.
pub fn connected_ack_tones(bandwidth: Bandwidth, caller: &str) -> Option<[[u8; 2]; 11]> {
    let (base, coarse_bins, alternate) = match bandwidth {
        Bandwidth::Hz500 => (50, 2, false),
        Bandwidth::Hz2300 => (29, 5, true),
        Bandwidth::Hz2750 => (22, 6, true),
    };
    let mut random = keyed_stream(caller, 1080, 1)?;
    let mut pairs = [[0; 2]; 11];
    pairs[..4].copy_from_slice(&[[64, 67], [56, 74], [64, 69], [68, 78]]);
    for (index, pair) in pairs[4..].iter_mut().enumerate() {
        let first = random.draw(coarse_bins);
        let mut second = random.draw(coarse_bins);
        while second == first {
            second = random.draw(coarse_bins);
        }
        let parity = usize::from(alternate && index % 2 == 0);
        *pair = [
            (base + parity + 14 * first + 2 * random.draw(7)) as u8,
            (base + parity + 14 * second + 2 * random.draw(7)) as u8,
        ];
        pair.sort_unstable();
    }
    Some(pairs)
}

/// Renders the response to link setup at 48 kHz for offline bench work.
pub fn synthesize_connected_ack(bandwidth: Bandwidth, caller: &str) -> Option<Vec<f32>> {
    Some(render(connected_ack_tones(bandwidth, caller)?))
}

/// Match the entire caller-keyed ACK, including both tones of every symbol.
/// Energy or the shared four-symbol preamble alone is insufficient.
pub fn find_connected_ack(
    bandwidth: Bandwidth,
    caller: &str,
    audio: &[f32],
    estimated_start: usize,
) -> Option<usize> {
    let pairs = connected_ack_tones(bandwidth, caller)?;
    let tones: Vec<_> = pairs
        .map(|pair| pair.map(|bin| Tone::new(usize::from(bin), SYMBOL_SAMPLES)))
        .into();
    let length = tones.len() * SYMBOL_SAMPLES;
    let last = audio.len().checked_sub(length)?;
    let first = estimated_start.saturating_sub(SYMBOL_SAMPLES);
    let through = estimated_start.saturating_add(SYMBOL_SAMPLES).min(last);
    let (start, score) = (first..=through)
        .step_by(32)
        .map(|start| {
            let mut total = 0.0;
            let mut weakest = 1.0_f64;
            for (pair, column) in tones
                .iter()
                .zip(audio[start..start + length].as_chunks::<SYMBOL_SAMPLES>().0)
            {
                let scale = energy(column) * SYMBOL_SAMPLES as f64 / 2.0;
                let powers = pair
                    .each_ref()
                    .map(|tone| tone.power(column) / scale.max(f64::EPSILON));
                total += powers.iter().sum::<f64>();
                weakest = weakest.min(powers[0]).min(powers[1]);
            }
            (start, (total, weakest))
        })
        .max_by(|a, b| a.1.0.total_cmp(&b.1.0))?;
    (score.0 >= 0.8 * tones.len() as f64 && score.1 >= 0.15).then_some(start)
}

fn render<const N: usize>(symbols: impl IntoIterator<Item = [u8; N]>) -> Vec<f32> {
    const OVERLAP: usize = 32;
    let mut audio = Vec::new();
    for (column, bins) in symbols.into_iter().enumerate() {
        let start = column * SYMBOL_SAMPLES;
        audio.resize(start + SYMBOL_SAMPLES + OVERLAP, 0.0);
        for bin in bins {
            let tone = Tone::new(usize::from(bin), SYMBOL_SAMPLES);
            for i in 0..SYMBOL_SAMPLES + OVERLAP {
                let edge = |i| (1.0 - (PI * (i as f64 + 0.5) / OVERLAP as f64).cos()) * 0.5;
                let gain = if i < OVERLAP {
                    edge(i)
                } else if i >= SYMBOL_SAMPLES {
                    1.0 - edge(i - SYMBOL_SAMPLES)
                } else {
                    1.0
                };
                audio[start + i] +=
                    (0.5 / N as f64 * gain * tone.quadrature[i % SYMBOL_SAMPLES].1) as f32;
            }
        }
    }
    audio
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_two_stock_bw500_connected_ack_vectors() {
        for (caller, tail) in [
            (
                "AAAA1",
                [
                    [58, 64],
                    [52, 72],
                    [54, 74],
                    [56, 68],
                    [58, 72],
                    [54, 68],
                    [58, 68],
                ],
            ),
            (
                "W9SSJ",
                [
                    [54, 72],
                    [56, 70],
                    [56, 76],
                    [54, 66],
                    [58, 72],
                    [54, 76],
                    [50, 72],
                ],
            ),
        ] {
            let pairs = connected_ack_tones(Bandwidth::Hz500, caller).unwrap();
            assert_eq!(pairs[..4], [[64, 67], [56, 74], [64, 69], [68, 78]]);
            assert_eq!(pairs[4..], tail);
            let audio = synthesize_connected_ack(Bandwidth::Hz500, caller).unwrap();
            assert_eq!(audio.len(), 11 * SYMBOL_SAMPLES + 32);
        }
    }

    #[test]
    fn matches_published_bw2300_call_vectors() {
        let request = Message::ConnectRequest(Bandwidth::Hz2300)
            .tones("K7ABC")
            .unwrap();
        assert_eq!(
            &request[10..],
            [
                36, 53, 86, 69, 52, 85, 76, 61, 98, 49, 64, 71, 68, 73, 52, 61, 74, 85, 90, 45, 46,
                89, 44, 37, 82, 83, 36, 61, 38, 43, 32,
            ]
        );
        let reply = Message::ConnectResponse(Bandwidth::Hz2300)
            .tones("K7ABC")
            .unwrap();
        assert_eq!(
            &reply[8..],
            [80, 71, 84, 77, 70, 33, 56, 79, 56, 95, 98, 59, 50, 29, 76]
        );
    }

    #[test]
    fn identifies_expected_call_and_rejects_other_calls() {
        let message = Message::ConnectRequest(Bandwidth::Hz500);
        let mut audio = vec![0.0; 2681];
        audio.extend(message.synthesize("W9SSJ-5").unwrap());
        assert!(
            message
                .find("W9SSJ-5", &audio, 2560)
                .unwrap()
                .abs_diff(2681)
                <= 64
        );
        assert_eq!(message.find("W9SSJ-10", &audio, 2560), None);
        assert_eq!(
            Message::ConnectRequest(Bandwidth::Hz2300).find("W9SSJ-5", &audio, 2560),
            None
        );
    }
}
