//! Single-threaded WebAssembly interface to the native BW500 codec and channel.

mod channel;

#[allow(dead_code)]
mod modem {
    include!(concat!(env!("OUT_DIR"), "/modem/mod.rs"));
}

use std::cell::RefCell;

use channel::{Channel, DELAY_SAMPLES};
use modem::{
    bw500::{self, Frame, Level},
    control::Bandwidth,
    data,
    receiver::{Receiver, Reception},
    waveform::BurstDetector,
    wide_level4,
};

const BW500_PAYLOAD_BYTES: usize = 9;
const WIDE_PAYLOAD_BYTES: usize = data::WIDE_SHORT_BYTES;
const MAX_SAMPLES: usize = 524_288;
const STAGING: usize = 0;
const NOISE: usize = 2;
const BENCH_CALLER: &str = "SIM001";

struct Buffers {
    payload: [u8; WIDE_PAYLOAD_BYTES],
    decoded_length: usize,
    samples: [Box<[f32]>; 3],
    channel: Channel,
    mode: Bandwidth,
}

thread_local! {
    static BUFFERS: RefCell<Buffers> = RefCell::new(Buffers {
        payload: [0; WIDE_PAYLOAD_BYTES],
        decoded_length: 0,
        samples: std::array::from_fn(|_| vec![0.0; MAX_SAMPLES].into_boxed_slice()),
        channel: Channel::new(),
        mode: Bandwidth::Hz500,
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_sample_rate() -> u32 {
    bw500::SAMPLE_RATE_HZ
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_max_samples() -> usize {
    MAX_SAMPLES
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_set_mode(bandwidth_hz: u32) -> i32 {
    let bandwidth = match bandwidth_hz {
        500 => Bandwidth::Hz500,
        2300 => Bandwidth::Hz2300,
        2750 => Bandwidth::Hz2750,
        _ => return 0,
    };
    BUFFERS.with_borrow_mut(|state| {
        state.mode = bandwidth;
        state.decoded_length = 0;
    });
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_payload_capacity() -> usize {
    BUFFERS.with_borrow(|state| {
        if state.mode == Bandwidth::Hz500 {
            BW500_PAYLOAD_BYTES
        } else {
            WIDE_PAYLOAD_BYTES
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_decoded_len() -> usize {
    BUFFERS.with_borrow(|state| state.decoded_length)
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_payload_ptr() -> *mut u8 {
    BUFFERS.with_borrow_mut(|state| state.payload.as_mut_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_buffer_ptr(kind: usize) -> *mut f32 {
    BUFFERS.with_borrow_mut(|state| {
        state
            .samples
            .get_mut(kind)
            .map_or(std::ptr::null_mut(), |samples| samples.as_mut_ptr())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_encode() -> usize {
    lw_encode_len(BW500_PAYLOAD_BYTES)
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_encode_len(length: usize) -> usize {
    BUFFERS.with_borrow_mut(|state| {
        let Some(payload) = state.payload.get(..length) else {
            return 0;
        };
        let encoded = if state.mode == Bandwidth::Hz500 {
            if length != BW500_PAYLOAD_BYTES {
                return 0;
            }
            let frame = Frame {
                payload: payload.to_vec(),
                control: 0,
            };
            bw500::synthesize(Level::One, &[frame])
        } else {
            if length == 0 {
                return 0;
            }
            encode_wide(state.mode, payload)
        };
        let Some(encoded) = encoded else {
            return 0;
        };
        state.samples[STAGING][..encoded.len()].copy_from_slice(&encoded);
        encoded.len()
    })
}

fn encode_wide(bandwidth: Bandwidth, payload: &[u8]) -> Option<Vec<f32>> {
    let waveform = wide_level4::Waveform::new(bandwidth)?;
    let frame = data::wide_short(payload, BENCH_CALLER)?;
    let mut lead = waveform.caller_data_lead(BENCH_CALLER)?;
    lead.begin_delivery();
    waveform.synthesize(&frame, &mut lead)
}

fn decode_bw500(samples: &[f32]) -> Option<Vec<u8>> {
    let mut receiver = Receiver::new().with_pilot_acquisition();
    let mut frame_count = 0;
    let mut payload = None;
    let mut accept = |reception: Reception| {
        for frame in reception.frames {
            frame_count += 1;
            if frame.level == 1 && frame.frame.payload.len() == BW500_PAYLOAD_BYTES {
                payload = Some(frame.frame.payload);
            }
        }
    };
    for chunk in samples.chunks(1024) {
        receiver.push(chunk, &mut accept).ok()?;
    }
    receiver.finish(accept);
    if frame_count == 1 { payload } else { None }
}

fn decode_wide(bandwidth: Bandwidth, samples: &[f32]) -> Option<Vec<u8>> {
    let waveform = wide_level4::Waveform::new(bandwidth)?;
    let mut detector = BurstDetector::new(bw500::SAMPLE_RATE_HZ);
    let mut candidates = Vec::new();
    detector.push(samples, |candidate| candidates.push(candidate));
    detector.finish(|candidate| candidates.push(candidate));
    let mut frames = candidates
        .into_iter()
        .filter(|candidate| candidate.end - candidate.start >= 100_000)
        .filter_map(|candidate| {
            waveform.decode(samples, candidate.start as usize..candidate.end as usize)
        });
    let frame = frames.next()?;
    if frames.next().is_some() {
        return None;
    }
    let decoded = data::decode_wide(&frame, BENCH_CALLER)?.payload;
    (1..=WIDE_PAYLOAD_BYTES)
        .contains(&decoded.len())
        .then_some(decoded)
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_decode(count: usize) -> i32 {
    BUFFERS.with_borrow_mut(|state| {
        state.decoded_length = 0;
        if !(1..=MAX_SAMPLES).contains(&count) {
            return -1;
        }
        let samples = &state.samples[STAGING][..count];
        if samples.iter().any(|sample| !sample.is_finite()) {
            return -1;
        }
        let payload = if state.mode == Bandwidth::Hz500 {
            decode_bw500(samples)
        } else {
            decode_wide(state.mode, samples)
        };
        if let Some(payload) = payload {
            state.payload[..payload.len()].copy_from_slice(&payload);
            state.decoded_length = payload.len();
            return 1;
        }
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_propagate(
    count: usize,
    noise_seed: u32,
    path_seed: u32,
    start_sample: f64,
    snr_db: f64,
    include_noise: u32,
) -> usize {
    if !(1..=MAX_SAMPLES - DELAY_SAMPLES).contains(&count)
        || !(-100.0..=100.0).contains(&snr_db)
        || !(0.0..=9_007_199_254_740_991.0).contains(&start_sample)
        || start_sample.fract() != 0.0
        || include_noise > 1
    {
        return 0;
    }
    BUFFERS.with_borrow_mut(|state| {
        let [samples, signal, noise] = &mut state.samples;
        let input = &samples[..count];
        if input.iter().any(|sample| !sample.is_finite()) {
            return 0;
        }
        let length = count + DELAY_SAMPLES;
        state
            .channel
            .propagate(input, path_seed, start_sample as u64, &mut signal[..length]);
        let noise = &mut noise[..length];
        if include_noise == 1 {
            state.channel.add_noise(input, noise_seed, snr_db, noise);
        } else {
            noise.fill(0.0);
        }
        length
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn lw_reference_power(kind: usize, count: usize) -> f64 {
    if kind > NOISE || !(1..=MAX_SAMPLES).contains(&count) {
        return f64::NAN;
    }
    BUFFERS.with_borrow_mut(|state| {
        let samples = &state.samples[kind][..count];
        if samples.iter().any(|sample| !sample.is_finite()) {
            return f64::NAN;
        }
        state.channel.reference_power(samples)
    })
}
