//! Bounded, chunk-independent BW500 level 1–3 receive worker.
//!
//! This energy-gated bench receiver emits CRC-valid frames, not session or
//! link-up events. Run it outside a real-time audio callback. Frequency offset,
//! fading, AGC and level-4 reception are not implemented here.

use std::{collections::VecDeque, ops::Range};

use super::{bw500, bw500_waveform::Waveform, waveform::BurstDetector};

const WINDOW: usize = bw500::SAMPLE_RATE_HZ as usize / 100;
const CAPACITY: usize = bw500::SAMPLE_RATE_HZ as usize * 30;

#[derive(Debug)]
pub struct ReceivedFrame {
    pub level: u8,
    /// Absolute, half-open positions in the input stream.
    pub samples: Range<u64>,
    pub frame: bw500::Frame,
}

#[derive(Debug)]
pub struct Reception {
    pub candidate: Range<u64>,
    /// The candidate began before retained audio. No partial payload is emitted.
    pub truncated: bool,
    pub frames: Vec<ReceivedFrame>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReceiveError {
    #[error("audio contains a non-finite sample")]
    NonFiniteSample,
    #[error("audio stream position overflow")]
    PositionOverflow,
}

/// Accepts normalized, 48 kHz mono audio. Memory is capped at 30 seconds.
/// Invalid chunks are rejected before any state is changed. Discontinuous
/// capture must start a new receiver rather than splice separated samples.
pub struct Receiver {
    detector: BurstDetector,
    audio: VecDeque<f32>,
    position: u64,
    capacity: usize,
    level4: Option<Waveform>,
}

impl Default for Receiver {
    fn default() -> Self {
        Self::new()
    }
}

impl Receiver {
    pub fn new() -> Self {
        Self {
            detector: BurstDetector::new(bw500::SAMPLE_RATE_HZ),
            audio: VecDeque::new(),
            position: 0,
            capacity: CAPACITY,
            level4: None,
        }
    }

    pub fn with_level4(waveform: Waveform) -> Self {
        Self {
            level4: Some(waveform),
            ..Self::new()
        }
    }

    pub fn push(
        &mut self,
        samples: &[f32],
        mut report: impl FnMut(Reception),
    ) -> Result<(), ReceiveError> {
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(ReceiveError::NonFiniteSample);
        }
        self.position
            .checked_add(samples.len() as u64)
            .ok_or(ReceiveError::PositionOverflow)?;
        // Bound each detector step so a large input chunk cannot evict a
        // completed burst before its callback is processed.
        for block in samples.chunks(WINDOW) {
            let discard = (self.audio.len() + block.len()).saturating_sub(self.capacity);
            self.audio.drain(..discard);
            self.audio.extend(block);
            self.position += block.len() as u64;
            let mut candidates = Vec::new();
            self.detector.push(block, |range| candidates.push(range));
            for candidate in candidates {
                report(self.decode(candidate));
            }
        }
        Ok(())
    }

    /// Flushes a final partial window and candidate, consuming this stream.
    pub fn finish(mut self, mut report: impl FnMut(Reception)) {
        let detector = std::mem::replace(
            &mut self.detector,
            BurstDetector::new(bw500::SAMPLE_RATE_HZ),
        );
        detector.finish(|candidate| report(self.decode(candidate)));
    }

    fn decode(&mut self, candidate: Range<u64>) -> Reception {
        let base = self.position - self.audio.len() as u64;
        let truncated = candidate.start < base;
        let frames = if truncated {
            Vec::new()
        } else {
            let range = (candidate.start - base) as usize..(candidate.end - base) as usize;
            let mut frames: Vec<_> =
                bw500::decode_burst(self.audio.make_contiguous(), range.clone())
                    .into_iter()
                    .map(|decoded| ReceivedFrame {
                        level: decoded.level.number(),
                        samples: base + decoded.samples.start as u64
                            ..base + decoded.samples.end as u64,
                        frame: decoded.frame,
                    })
                    .collect();
            if frames.is_empty()
                && let Some(waveform) = &self.level4
                && let Some(decoded) = waveform.decode(self.audio.make_contiguous(), range)
            {
                frames.push(ReceivedFrame {
                    level: 4,
                    samples: base + decoded.samples.start as u64..base + decoded.samples.end as u64,
                    frame: bw500::Frame {
                        payload: decoded.frame[..43].to_vec(),
                        control: decoded.frame[43],
                    },
                });
            }
            frames
        };
        Reception {
            candidate,
            truncated,
            frames,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_frames_keep_absolute_positions_across_chunk_boundaries() {
        let frame = bw500::Frame {
            payload: vec![0x5a; 9],
            control: 0x99,
        };
        let burst = bw500::synthesize(bw500::Level::One, std::slice::from_ref(&frame)).unwrap();
        let mut audio = vec![0.0; 3217];
        audio.extend(&burst);
        audio.extend(vec![0.0; 2048]);
        audio.extend(&burst);
        for size in [127, 480, 1024, audio.len()] {
            let mut receiver = Receiver::new();
            let mut receptions = Vec::new();
            for block in audio.chunks(size) {
                receiver
                    .push(block, |event| receptions.push(event))
                    .unwrap();
            }
            receiver.finish(|event| receptions.push(event));
            assert_eq!(receptions.len(), 2);
            for (i, reception) in receptions.iter().enumerate() {
                assert!(!reception.truncated);
                assert_eq!(reception.frames.len(), 1);
                assert_eq!(reception.frames[0].frame, frame);
                let expected = 3217 + i * (burst.len() + 2048) + 4096;
                assert!(reception.frames[0].samples.start.abs_diff(expected as u64) <= 32);
            }
        }
    }

    #[test]
    fn rejects_nonfinite_chunks_without_advancing_and_bounds_retention() {
        let mut receiver = Receiver::new();
        assert_eq!(
            receiver.push(&[0.0, f32::NAN], |_| panic!()),
            Err(ReceiveError::NonFiniteSample)
        );
        assert_eq!(receiver.position, 0);
        receiver.capacity = WINDOW * 4;
        let mut events = Vec::new();
        receiver
            .push(&vec![0.5; WINDOW * 10], |event| events.push(event))
            .unwrap();
        assert!(receiver.audio.len() <= receiver.capacity);
        receiver
            .push(&[0.0; WINDOW], |event| events.push(event))
            .unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].truncated);
        assert!(events[0].frames.is_empty());
    }

    #[test]
    fn recovers_after_retained_history_wraps_and_rejects_nonframes() {
        let frame = bw500::Frame {
            payload: vec![0xa5; 9],
            control: 0x99,
        };
        let burst = bw500::synthesize(bw500::Level::One, std::slice::from_ref(&frame)).unwrap();
        let mut receiver = Receiver::new();
        receiver.capacity = burst.len() + 4 * WINDOW;
        let mut events = Vec::new();
        let silence = vec![0.0; receiver.capacity * 2];
        receiver
            .push(&silence, |_| panic!("silence is not a frame"))
            .unwrap();
        receiver.push(&burst, |event| events.push(event)).unwrap();
        receiver
            .push(&[0.0; WINDOW * 2], |event| events.push(event))
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].frames.len(), 1);
        assert_eq!(events[0].frames[0].frame, frame);
        assert!(events[0].frames[0].samples.start >= silence.len() as u64);
        // Signal energy alone must never be reported as a decoded frame.
        receiver
            .push(&vec![0.25; burst.len()], |event| events.push(event))
            .unwrap();
        receiver.finish(|event| events.push(event));
        assert_eq!(events.len(), 2);
        assert!(events[1].frames.is_empty());
    }
}
