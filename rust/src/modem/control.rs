//! Callsign-keyed single-tone VARA HF control waveforms.

use std::f64::consts::PI;

use super::{
    coding::{self, Random24},
    waveform::{Tone, energy},
};

const SYMBOL_SAMPLES: usize = 2048;
const REQUEST_PREAMBLE: &[u8] = &[74, 68, 70, 60, 60, 77, 50, 76, 78, 74];
const RESPONSE_PREAMBLE: &[u8] = &[62, 67, 55, 66, 59, 72, 68, 55];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    WideConfirm(Bandwidth),
    WideSetupNak(Bandwidth),
    QueryMissingData,
    Bw500DataNakQuery,
    /// Called-station-keyed rejection of a link-setup burst.
    Bw500SetupNak,
    /// First BW500 poll sent by the caller after confirmation.
    Bw500KeepaliveA,
    /// Second BW500 poll sent by the caller after confirmation.
    Bw500KeepaliveB,
    /// Caller-keyed request for the transmit turn.
    Bw500CallerTurnRequest,
    /// Short caller-keyed turn request following an acknowledged release.
    Bw500CallerTurnRequestShort,
    /// Caller-keyed request sent by the responder.
    Bw500ResponderTurnRequest,
    Bw500FinalQueryShort,
    Bw500FinalQuery,
    Bw500AlternateQueryShort,
    Bw500AlternateQuery,
    Bw500IntermediateQueryAck,
    Bw500Drained,
    Bw500ResponderDrained,
    Bw500ResponderOverNak,
    Bw500CallerDataNak,
    Bw500CallerTurnIdle,
    Bw500ResponderTurnIdle,
    Bw500CallerTurnRelease,
    Bw500ResponderTurnRelease,
    SessionDisconnect {
        responder: bool,
    },
    SessionDisconnectShort,
}

impl Message {
    /// Returns the 48 kHz / 2048-point bin sequence for the message's callsign key.
    pub fn tones(self, key: &str) -> Option<Vec<u8>> {
        self.tones_on(key, Bandwidth::Hz500)
    }

    fn tones_on(self, key: &str, bandwidth: Bandwidth) -> Option<Vec<u8>> {
        use Bandwidth::*;
        let (head, count, seed_offset, preadvance, base, coarse_bins, alternate) = match self {
            Self::ConnectRequest(Hz500) => (REQUEST_PREAMBLE, 31, 57, 1, 50, 2, false),
            Self::ConnectRequest(Hz2300) => (REQUEST_PREAMBLE, 31, 50, 1, 29, 5, true),
            Self::ConnectRequest(Hz2750) => (REQUEST_PREAMBLE, 31, 58, 1, 29, 5, true),
            Self::ConnectResponse(Hz500) => (RESPONSE_PREAMBLE, 15, 289, 721, 50, 2, false),
            Self::ConnectResponse(Hz2300) => (RESPONSE_PREAMBLE, 15, 289, 511, 29, 5, true),
            Self::ConnectResponse(Hz2750) => (RESPONSE_PREAMBLE, 15, 579, 91, 22, 6, true),
            Self::Bw500Recovery => (&[74][..], 31, 60, 1117, 50, 2, false),
            Self::Bw500SetupNak | Self::WideSetupNak(Hz500) => {
                (&[62][..], 15, 289, 31, 50, 2, false)
            }
            Self::WideSetupNak(Hz2300) => (&[62][..], 15, 289, 31, 29, 5, true),
            Self::WideSetupNak(Hz2750) => (&[62][..], 15, 289, 31, 22, 6, true),
            Self::Bw500Confirm | Self::WideConfirm(Hz500) => {
                (&[62][..], 15, 1551, 91, 50, 2, false)
            }
            Self::WideConfirm(Hz2300) => (&[62][..], 15, 1551, 91, 29, 5, true),
            Self::WideConfirm(Hz2750) => (&[62][..], 15, 1551, 91, 22, 6, true),
            Self::Bw500KeepaliveA => (&[74][..], 31, 1550, 187, 50, 2, false),
            Self::Bw500KeepaliveB => (&[74][..], 31, 60, 1241, 50, 2, false),
            Self::Bw500FinalQueryShort => (&[62][..], 15, 61, 331, 50, 2, false),
            Self::Bw500FinalQuery => (&[74][..], 31, 60, 683, 50, 2, false),
            Self::Bw500AlternateQueryShort => (&[62][..], 15, 61, 361, 50, 2, false),
            Self::Bw500AlternateQuery => (&[74][..], 31, 60, 745, 50, 2, false),
            Self::QueryMissingData => (&[62][..], 15, 289, 541, 50, 2, false),
            Self::Bw500DataNakQuery => (&[74][..], 31, 288, 63, 50, 2, false),
            Self::Bw500IntermediateQueryAck => (&[74][..], 31, 288, 1, 50, 2, false),
            Self::Bw500Drained => (&[74][..], 31, 60, 807, 50, 2, false),
            Self::Bw500ResponderDrained => (&[74][..], 31, 288, 807, 50, 2, false),
            Self::Bw500ResponderOverNak => (&[74][..], 31, 288, 1117, 50, 2, false),
            Self::Bw500CallerDataNak => (&[74][..], 31, 60, 63, 50, 2, false),
            Self::Bw500CallerTurnIdle => (&[74][..], 31, 850, 63, 50, 2, false),
            Self::Bw500ResponderTurnIdle => (&[74][..], 31, 1078, 63, 50, 2, false),
            Self::Bw500CallerTurnRelease => (&[62, 67][..], 15, 61, 391, 50, 2, false),
            Self::Bw500ResponderTurnRelease => (&[62, 67][..], 15, 289, 391, 50, 2, false),
            Self::Bw500ResponderTurnRequest => (&[74][..], 31, 1078, 1, 50, 2, false),
            Self::Bw500CallerTurnRequest => (&[74][..], 31, 850, 1, 50, 2, false),
            Self::Bw500CallerTurnRequestShort => (&[62, 67][..], 15, 851, 1, 50, 2, false),
            Self::SessionDisconnect { responder: true } => (&[74][..], 31, 578, 311, 50, 2, false),
            Self::SessionDisconnect { responder: false } => (&[74][..], 31, 60, 125, 50, 2, false),
            Self::SessionDisconnectShort => (&[62][..], 15, 61, 61, 50, 2, false),
        };
        let (base, coarse_bins, alternate) = match self {
            Self::Bw500KeepaliveA
            | Self::Bw500KeepaliveB
            | Self::Bw500CallerTurnRequest
            | Self::Bw500ResponderTurnRequest
            | Self::Bw500CallerTurnRelease
            | Self::Bw500ResponderTurnRelease
            | Self::QueryMissingData
            | Self::Bw500FinalQuery
            | Self::Bw500AlternateQuery
            | Self::Bw500IntermediateQueryAck
            | Self::Bw500Drained
            | Self::Bw500ResponderDrained
            | Self::SessionDisconnectShort
            | Self::SessionDisconnect { .. } => {
                let (base, bins, alternate) = answer_geometry(bandwidth);
                (base as u32, bins as u32, alternate)
            }
            _ => (base, coarse_bins, alternate),
        };
        let mut random = keyed_stream(key, seed_offset, preadvance)?;
        let mut tones = head.to_vec();
        for index in 0..count {
            let parity = u32::from(alternate && index % 2 == 0);
            tones.push(single_tone_bin(&mut random, base + parity, coarse_bins));
        }
        Some(tones)
    }

    /// Renders one control burst as normalized 48 kHz mono audio for bench tests.
    pub fn synthesize(self, key: &str) -> Option<Vec<f32>> {
        self.synthesize_on(key, Bandwidth::Hz500)
    }

    /// Render a session control using the negotiated bandwidth's alphabet.
    pub fn synthesize_on(self, key: &str, bandwidth: Bandwidth) -> Option<Vec<f32>> {
        let bins = self.tones_on(key, bandwidth)?;
        Some(render(bins.into_iter().map(|bin| [bin])))
    }

    /// Searches for an expected burst near the estimated start of a 48 kHz
    /// bench recording. The caller supplies the expected message and callsign.
    pub fn find(self, key: &str, audio: &[f32], estimated_start: usize) -> Option<usize> {
        self.find_on(key, audio, estimated_start, Bandwidth::Hz500)
    }

    /// Match a session control using the negotiated bandwidth's alphabet.
    pub fn find_on(
        self,
        key: &str,
        audio: &[f32],
        estimated_start: usize,
        bandwidth: Bandwidth,
    ) -> Option<usize> {
        let bins = self.tones_on(key, bandwidth)?;
        let robust = bandwidth == Bandwidth::Hz500
            && (bins.len() >= 20 || matches!(self, Self::Bw500Confirm));
        if let Some(start) = find_single_tones(&bins, audio, estimated_start, robust) {
            return Some(start);
        }
        if bandwidth != Bandwidth::Hz500 {
            return None;
        }
        // The stock alternatives share a preamble but require a complete
        // match to their own callsign-keyed tail.
        let (head, seed, advances, count): (&[u8], _, &[_], _) = match self {
            Self::ConnectResponse(Bandwidth::Hz500) => (RESPONSE_PREAMBLE, 289, &[691, 661], 15),
            Self::Bw500Confirm => (&[62], 1551, &[61], 15),
            Self::Bw500KeepaliveA => (&[74], 1550, &[125], 31),
            _ => return None,
        };
        advances.iter().find_map(|&advance| {
            let mut random = keyed_stream(key, seed, advance)?;
            let mut expected = head.to_vec();
            expected.extend((0..count).map(|_| single_tone_bin(&mut random, 50, 2)));
            find_single_tones(&expected, audio, estimated_start, true)
        })
    }
}

fn single_tone_bin(random: &mut Random24, base: u32, coarse_bins: u32) -> u8 {
    let coarse = (random.advance() * coarse_bins) >> 24;
    let fine = (random.advance() * 7) >> 24;
    (base + 14 * coarse + 2 * fine) as u8
}

fn find_single_tones(
    bins: &[u8],
    audio: &[f32],
    estimated_start: usize,
    robust: bool,
) -> Option<usize> {
    let length = bins.len() * SYMBOL_SAMPLES;
    let last = audio.len().checked_sub(length)?;
    let from = estimated_start.saturating_sub(SYMBOL_SAMPLES);
    let through = estimated_start.saturating_add(SYMBOL_SAMPLES).min(last);
    let tones_at = |hz: f64, bins: &[u8]| {
        bins.iter()
            .map(|&bin| {
                Tone::at_bin(
                    f64::from(bin) + hz * SYMBOL_SAMPLES as f64 / 48_000.0,
                    SYMBOL_SAMPLES,
                )
            })
            .collect::<Vec<_>>()
    };
    let search = |tones: &[Tone], from, through, step, mean, floor| {
        (from..=through)
            .step_by(step)
            .filter_map(|start| {
                let (sum, weakest) = single_tone_score(tones, &audio[start..]);
                (sum >= mean * tones.len() as f64 && weakest >= floor).then_some((start, sum))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(start, _)| start)
    };
    let tones = tones_at(0.0, bins);
    if let Some(start) = search(&tones, from, through, 64, 0.80, 0.30) {
        return Some(start);
    }
    if !robust {
        return None;
    }

    if let Some(start) = search(&tones, from, through, 64, 0.70, 0.25) {
        return Some(start);
    }
    // Use a short prefix to propose offset/timing hypotheses. The entire
    // keyed sequence still has to pass the acceptance test at one offset.
    for hz in [-8.0, 8.0, -15.0, 15.0, -22.0, 22.0] {
        let prefix = tones_at(hz, &bins[..4]);
        let Some(start) = search(&prefix, from, through, 256, 0.64, 0.20) else {
            continue;
        };
        let tones = tones_at(hz, bins);
        if let Some(start) = search(
            &tones,
            start.saturating_sub(256).max(from),
            (start + 256).min(through),
            64,
            0.70,
            0.25,
        ) {
            return Some(start);
        }
    }
    None
}

fn single_tone_score(tones: &[Tone], audio: &[f32]) -> (f64, f64) {
    tones
        .iter()
        .zip(audio.chunks_exact(SYMBOL_SAMPLES))
        .map(|(tone, column)| {
            let norm = energy(column) * SYMBOL_SAMPLES as f64 / 2.0;
            tone.power(column) / norm.max(f64::EPSILON)
        })
        .fold((0.0, 1.0_f64), |(sum, weakest), score| {
            (sum + score, weakest.min(score))
        })
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
    responder_ack_tones(bandwidth, caller, 0)
}

fn responder_ack_tones(bandwidth: Bandwidth, caller: &str, frame: usize) -> Option<[[u8; 2]; 11]> {
    answer_pairs(bandwidth, caller, 1080, frame)
}

fn answer_pairs(bandwidth: Bandwidth, key: &str, seed: u32, frame: usize) -> Option<[[u8; 2]; 11]> {
    let tail = answer_tail(bandwidth, key, seed, frame)?;
    let mut pairs = [[0; 2]; 11];
    pairs[..4].copy_from_slice(&[[64, 67], [56, 74], [64, 69], [68, 78]]);
    pairs[4..].copy_from_slice(&tail);
    Some(pairs)
}

fn answer_tail(bandwidth: Bandwidth, key: &str, seed: u32, frame: usize) -> Option<[[u8; 2]; 7]> {
    let (base, coarse_bins, alternate) = answer_geometry(bandwidth);
    let mut random = keyed_stream(key, seed, 1)?;
    Some(answer_frame(
        &mut random,
        base,
        coarse_bins,
        alternate,
        frame,
    ))
}

fn answer_geometry(bandwidth: Bandwidth) -> (usize, usize, bool) {
    match bandwidth {
        Bandwidth::Hz500 => (50, 2, false),
        Bandwidth::Hz2300 => (29, 5, true),
        Bandwidth::Hz2750 => (22, 6, true),
    }
}

pub(super) fn lead_stream(bandwidth: Bandwidth, caller: &str, responder: bool) -> Option<Random24> {
    let (base, coarse_bins, alternate) = answer_geometry(bandwidth);
    let mut random = keyed_stream(caller, if responder { 1080 } else { 852 }, 1)?;
    answer_frame(&mut random, base, coarse_bins, alternate, 1);
    Some(random)
}

fn answer_frame(
    random: &mut Random24,
    base: usize,
    coarse_bins: usize,
    alternate: bool,
    frame: usize,
) -> [[u8; 2]; 7] {
    let mut pairs = [[0; 2]; 7];
    for _ in 0..=frame {
        for (index, pair) in pairs.iter_mut().enumerate() {
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
    }
    pairs
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bw500IntermediateReply {
    SameLevel,
    IncreaseOne,
    IncreaseTwo,
    IncreaseThree,
    IncreaseFour,
    DecreaseOne,
    NotDelivered,
}

impl Bw500IntermediateReply {
    /// Called-station keyed intermediate answer. The role identifies the
    /// station sending the answer, independently of which station sent DATA.
    pub fn tones(self, called: &str, responder_sends: bool) -> Option<[[u8; 2]; 8]> {
        self.tones_on(called, responder_sends, Bandwidth::Hz500)
    }

    fn tones_on(
        self,
        called: &str,
        responder_sends: bool,
        bandwidth: Bandwidth,
    ) -> Option<[[u8; 2]; 8]> {
        let frame = match self {
            Self::SameLevel => 0,
            Self::IncreaseOne => 6,
            Self::IncreaseTwo => 4,
            Self::IncreaseThree => 7,
            Self::IncreaseFour => 8,
            Self::DecreaseOne => 5,
            Self::NotDelivered => 1,
        };
        let seed = if responder_sends { 290 } else { 62 };
        let tail = answer_tail(bandwidth, called, seed, frame)?;
        let mut pairs = [[64, 67]; 8];
        pairs[1..].copy_from_slice(&tail);
        Some(pairs)
    }

    pub fn synthesize(self, called: &str, responder_sends: bool) -> Option<Vec<f32>> {
        self.synthesize_on(called, responder_sends, Bandwidth::Hz500)
    }

    pub fn synthesize_on(
        self,
        called: &str,
        responder_sends: bool,
        bandwidth: Bandwidth,
    ) -> Option<Vec<f32>> {
        Some(render(self.tones_on(called, responder_sends, bandwidth)?))
    }

    /// Match an expected reply in a caller-supplied receive window.
    pub fn find(
        self,
        called: &str,
        responder_sends: bool,
        audio: &[f32],
        estimated_start: usize,
    ) -> Option<usize> {
        self.find_on(
            called,
            responder_sends,
            audio,
            estimated_start,
            Bandwidth::Hz500,
        )
    }

    pub fn find_on(
        self,
        called: &str,
        responder_sends: bool,
        audio: &[f32],
        estimated_start: usize,
        bandwidth: Bandwidth,
    ) -> Option<usize> {
        find_pairs(
            &self.tones_on(called, responder_sends, bandwidth)?,
            audio,
            estimated_start,
        )
    }
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
    if bandwidth != Bandwidth::Hz500 {
        return find_pairs(&pairs, audio, estimated_start);
    }
    let (first, through) = pair_timing(&pairs, audio, estimated_start)?;
    if let Some(start) = search_pairs(&pairs, audio, first..=through, 32, 0.0, 0.70, 0.08) {
        return Some(start);
    }
    for hz in [-8.0, 8.0, -15.0, 15.0, -22.0, 22.0] {
        let Some(start) = search_pairs(&pairs[..4], audio, first..=through, 256, hz, 0.64, 0.08)
        else {
            continue;
        };
        let near = start.saturating_sub(256).max(first)..=(start + 256).min(through);
        if let Some(start) = search_pairs(&pairs, audio, near, 32, hz, 0.70, 0.08) {
            return Some(start);
        }
    }
    None
}

/// Renders the BW500 responder's caller-keyed reply to a post-connect poll.
pub fn synthesize_bw500_poll_ack(caller: &str) -> Option<Vec<f32>> {
    poll_ack(Bandwidth::Hz500, caller)
}

/// Matches the BW500 poll reply, including its state-dependent tail.
pub fn find_bw500_poll_ack(caller: &str, audio: &[f32], estimated_start: usize) -> Option<usize> {
    find_poll_ack(Bandwidth::Hz500, caller, audio, estimated_start)
}

/// Renders the BW500 responder's called-station-keyed grant of the transmit turn.
pub fn synthesize_bw500_turn_grant(called: &str) -> Option<Vec<f32>> {
    turn_grant(Bandwidth::Hz500, called, true)
}

/// Matches the BW500 transmit-turn grant with its called-station key.
pub fn find_bw500_turn_grant(called: &str, audio: &[f32], estimated_start: usize) -> Option<usize> {
    find_turn_grant(Bandwidth::Hz500, called, true, audio, estimated_start)
}

/// Builds the caller's reply to a responder's turn request.
pub fn synthesize_bw500_caller_turn_grant(called: &str) -> Option<Vec<f32>> {
    turn_grant(Bandwidth::Hz500, called, false)
}

pub fn find_bw500_caller_turn_grant(called: &str, audio: &[f32], start: usize) -> Option<usize> {
    find_turn_grant(Bandwidth::Hz500, called, false, audio, start)
}

/// Builds the caller's acknowledgment of a closing DATA frame.
pub fn synthesize_bw500_caller_data_ack(caller: &str) -> Option<Vec<f32>> {
    caller_data_ack(Bandwidth::Hz500, caller)
}

pub fn find_bw500_caller_data_ack(caller: &str, audio: &[f32], start: usize) -> Option<usize> {
    find_caller_data_ack(Bandwidth::Hz500, caller, audio, start)
}

/// Render or match the post-confirmation poll reply at the selected bandwidth.
pub fn poll_ack(bandwidth: Bandwidth, caller: &str) -> Option<Vec<f32>> {
    Some(render(responder_ack_tones(bandwidth, caller, 1)?))
}

pub fn find_poll_ack(
    bandwidth: Bandwidth,
    caller: &str,
    audio: &[f32],
    start: usize,
) -> Option<usize> {
    find_pairs(&responder_ack_tones(bandwidth, caller, 1)?, audio, start)
}

/// Render or match the grant from either station.
pub fn turn_grant(bandwidth: Bandwidth, called: &str, responder: bool) -> Option<Vec<f32>> {
    Some(render(answer_pairs(
        bandwidth,
        called,
        if responder { 290 } else { 62 },
        13,
    )?))
}

pub fn find_turn_grant(
    bandwidth: Bandwidth,
    called: &str,
    responder: bool,
    audio: &[f32],
    start: usize,
) -> Option<usize> {
    find_pairs(
        &answer_pairs(bandwidth, called, if responder { 290 } else { 62 }, 13)?,
        audio,
        start,
    )
}

pub fn caller_data_ack(bandwidth: Bandwidth, caller: &str) -> Option<Vec<f32>> {
    Some(render(answer_pairs(bandwidth, caller, 852, 0)?))
}

pub fn find_caller_data_ack(
    bandwidth: Bandwidth,
    caller: &str,
    audio: &[f32],
    start: usize,
) -> Option<usize> {
    find_pairs(&answer_pairs(bandwidth, caller, 852, 0)?, audio, start)
}

fn pair_timing(pairs: &[[u8; 2]], audio: &[f32], estimated_start: usize) -> Option<(usize, usize)> {
    let last = audio.len().checked_sub(pairs.len() * SYMBOL_SAMPLES)?;
    Some((
        estimated_start.saturating_sub(SYMBOL_SAMPLES),
        estimated_start.saturating_add(SYMBOL_SAMPLES).min(last),
    ))
}

fn find_pairs(pairs: &[[u8; 2]], audio: &[f32], estimated_start: usize) -> Option<usize> {
    let (first, through) = pair_timing(pairs, audio, estimated_start)?;
    search_pairs(pairs, audio, first..=through, 32, 0.0, 0.80, 0.15)
}

fn search_pairs(
    pairs: &[[u8; 2]],
    audio: &[f32],
    timing: std::ops::RangeInclusive<usize>,
    step: usize,
    hz: f64,
    mean: f64,
    weakest_floor: f64,
) -> Option<usize> {
    let bins = hz * SYMBOL_SAMPLES as f64 / 48_000.0;
    let tones: Vec<_> = pairs
        .iter()
        .map(|pair| pair.map(|bin| Tone::at_bin(f64::from(bin) + bins, SYMBOL_SAMPLES)))
        .collect();
    timing
        .step_by(step)
        .filter_map(|start| {
            let mut total = 0.0;
            let mut weakest = 1.0_f64;
            for (pair, column) in tones
                .iter()
                .zip(audio[start..].chunks_exact(SYMBOL_SAMPLES))
            {
                let scale = (energy(column) * SYMBOL_SAMPLES as f64 / 2.0).max(f64::EPSILON);
                for tone in pair {
                    let power = tone.power(column) / scale;
                    total += power;
                    weakest = weakest.min(power);
                }
            }
            (total >= mean * tones.len() as f64 && weakest >= weakest_floor)
                .then_some((start, total))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(start, _)| start)
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
    fn accepts_stock_bw500_response_variants_with_the_full_keyed_tail() {
        for tail in [
            [60, 50, 66, 74, 62, 66, 76, 64, 76, 60, 56, 74, 56, 54, 58],
            [58, 70, 64, 76, 54, 52, 60, 62, 60, 74, 74, 70, 58, 68, 72],
        ] {
            let tones = RESPONSE_PREAMBLE.iter().copied().chain(tail);
            let audio = render(tones.map(|bin| [bin]));
            let response = Message::ConnectResponse(Bandwidth::Hz500);
            assert_eq!(response.find("W1A", &audio, 0), Some(0));
            assert_eq!(response.find("W1B", &audio, 0), None);
        }
    }

    #[test]
    fn accepts_low_snr_stock_bw500_confirmation_with_the_full_keyed_tail() {
        let bins = [
            62, 56, 62, 52, 58, 66, 60, 60, 54, 72, 74, 58, 74, 54, 66, 64,
        ];
        let audio = render(bins.map(|bin| [bin]));
        assert_eq!(Message::Bw500Confirm.find("W1A", &audio, 0), Some(0));
        assert_eq!(Message::Bw500Confirm.find("W1B", &audio, 0), None);
    }

    #[test]
    fn accepts_stock_bw500_poll_after_level_three_setup() {
        let bins = [
            74, 68, 58, 66, 74, 54, 66, 66, 52, 68, 60, 60, 60, 54, 72, 70, 50, 76, 64, 60, 62, 64,
            54, 72, 74, 56, 68, 64, 54, 52, 74, 50,
        ];
        let audio = render(bins.map(|bin| [bin]));
        assert_eq!(Message::Bw500KeepaliveA.find("W1A", &audio, 0), Some(0));
        assert_eq!(Message::Bw500KeepaliveA.find("W1B", &audio, 0), None);
    }

    #[test]
    fn bw500_controls_acquire_offset_and_reject_the_wrong_key() {
        for bin_shift in [-15.0, 15.0] {
            let render_shifted = |columns: Vec<Vec<u8>>| -> Vec<f32> {
                columns
                    .into_iter()
                    .flat_map(|bins| {
                        let tones: Vec<_> = bins
                            .into_iter()
                            .map(|bin| {
                                Tone::at_bin(
                                    f64::from(bin) + bin_shift * SYMBOL_SAMPLES as f64 / 48_000.0,
                                    SYMBOL_SAMPLES,
                                )
                            })
                            .collect();
                        (0..SYMBOL_SAMPLES).map(move |n| {
                            tones
                                .iter()
                                .map(|tone| tone.quadrature[n].1 as f32 * 0.20)
                                .sum::<f32>()
                        })
                    })
                    .collect()
            };
            let bandwidth = Bandwidth::Hz500;
            let request = Message::ConnectRequest(bandwidth);
            let samples = render_shifted(
                request
                    .tones("W1AW")
                    .unwrap()
                    .into_iter()
                    .map(|bin| vec![bin])
                    .collect(),
            );
            assert_eq!(request.find("W1AW", &samples, 0), Some(0));
            assert_eq!(request.find("W1AX", &samples, 0), None);
            let samples = render_shifted(
                connected_ack_tones(bandwidth, "W1AW")
                    .unwrap()
                    .into_iter()
                    .map(|pair| pair.to_vec())
                    .collect(),
            );
            assert_eq!(find_connected_ack(bandwidth, "W1AW", &samples, 0), Some(0));
            assert_eq!(find_connected_ack(bandwidth, "W1AX", &samples, 0), None);
        }
    }

    #[test]
    fn matches_stock_bw500_post_connect_vectors() {
        // Windows VARA HF 4.9.1, 2026-11-11 virtual audio matrix.
        assert_eq!(
            Message::Bw500KeepaliveA.tones("W1AW").unwrap(),
            [
                74, 68, 62, 72, 66, 68, 60, 72, 72, 54, 58, 52, 50, 68, 52, 56, 68, 76, 58, 50, 68,
                58, 70, 52, 56, 74, 76, 72, 74, 58, 52, 50,
            ]
        );
        assert_eq!(
            responder_ack_tones(Bandwidth::Hz500, "W1AW", 1).unwrap(),
            [
                [64, 67],
                [56, 74],
                [64, 69],
                [68, 78],
                [62, 66],
                [52, 68],
                [50, 64],
                [50, 76],
                [52, 72],
                [62, 64],
                [56, 64],
            ]
        );
        assert_eq!(
            Message::Bw500CallerTurnRequest.tones("KN6MQS").unwrap(),
            [
                74, 60, 60, 74, 62, 52, 64, 76, 72, 58, 54, 70, 66, 76, 74, 54, 68, 74, 54, 66, 72,
                66, 50, 50, 60, 72, 54, 54, 76, 60, 56, 54,
            ]
        );
        assert_eq!(
            answer_pairs(Bandwidth::Hz500, "KN6MQS", 290, 13).unwrap(),
            [
                [64, 67],
                [56, 74],
                [64, 69],
                [68, 78],
                [54, 64],
                [58, 68],
                [50, 76],
                [50, 64],
                [52, 74],
                [58, 76],
                [54, 66],
            ]
        );
        assert_eq!(
            Message::Bw500SetupNak.tones("KN6MQS").unwrap(),
            [
                62, 56, 56, 68, 66, 76, 56, 52, 66, 76, 56, 54, 56, 62, 52, 66
            ]
        );
        let audio = synthesize_bw500_poll_ack("W1AW").unwrap();
        assert_eq!(find_bw500_poll_ack("W1AW", &audio, 0), Some(0));
        assert_eq!(
            find_connected_ack(Bandwidth::Hz500, "W1AW", &audio, 0),
            None
        );
    }

    #[test]
    fn matches_published_bw500_intermediate_answer_vectors() {
        use Bw500IntermediateReply::{DecreaseOne, SameLevel};
        assert_eq!(
            SameLevel.tones("BBBB2", true).unwrap(),
            [
                [64, 67],
                [60, 64],
                [52, 68],
                [56, 64],
                [58, 70],
                [54, 68],
                [60, 74],
                [50, 70],
            ]
        );
        assert_eq!(
            SameLevel.tones("W1AW", false).unwrap(),
            [
                [64, 67],
                [50, 66],
                [60, 64],
                [50, 68],
                [58, 76],
                [52, 76],
                [58, 76],
                [60, 68],
            ]
        );
        let down = [
            [64, 67],
            [62, 76],
            [58, 76],
            [50, 68],
            [50, 72],
            [60, 74],
            [50, 68],
            [52, 76],
        ];
        assert_eq!(DecreaseOne.tones("W1AW", false).unwrap(), down);
        let audio = render(down);
        assert!(DecreaseOne.find("W1AW", false, &audio, 0).is_some());
        assert!(SameLevel.find("W1AW", false, &audio, 0).is_none());
        assert!(DecreaseOne.find("W1AW", true, &audio, 0).is_none());
    }

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
    fn matches_stock_bw2300_intermediate_acknowledgment() {
        assert_eq!(
            Bw500IntermediateReply::SameLevel
                .tones_on("W1A", true, Bandwidth::Hz2300)
                .unwrap(),
            [
                [64, 67],
                [82, 88],
                [49, 95],
                [60, 80],
                [81, 87],
                [84, 94],
                [51, 93],
                [42, 84],
            ],
        );
    }

    #[test]
    fn matches_stock_wide_turn_releases() {
        use Bandwidth::{Hz2300, Hz2750};
        use Message::{Bw500CallerTurnRelease, Bw500ResponderTurnRelease};
        for (message, bandwidth, tones) in [
            (
                Bw500ResponderTurnRelease,
                Hz2300,
                [
                    62, 67, 32, 31, 42, 93, 42, 97, 44, 93, 66, 75, 32, 47, 66, 95, 76,
                ],
            ),
            (
                Bw500ResponderTurnRelease,
                Hz2750,
                [
                    62, 67, 25, 24, 35, 100, 35, 90, 37, 86, 59, 82, 25, 54, 73, 88, 69,
                ],
            ),
            (
                Bw500CallerTurnRelease,
                Hz2300,
                [
                    62, 67, 88, 95, 70, 47, 32, 39, 54, 39, 50, 71, 80, 73, 96, 83, 46,
                ],
            ),
            (
                Bw500CallerTurnRelease,
                Hz2750,
                [
                    62, 67, 81, 88, 77, 40, 25, 32, 61, 32, 43, 64, 87, 66, 103, 90, 53,
                ],
            ),
        ] {
            assert_eq!(message.tones_on("W1A", bandwidth).unwrap(), tones);
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
