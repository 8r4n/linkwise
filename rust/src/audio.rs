//! Optional CPAL input capture. DSP runs on a worker, outside the callback.

#[cfg(feature = "audio-device")]
pub mod device {
    use std::sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
    };
    use std::time::Duration;

    use cpal::{
        SizedSample,
        traits::{DeviceTrait, HostTrait, StreamTrait},
    };

    const BLOCK_SAMPLES: usize = 1024;

    pub struct Block {
        pub samples: [f32; BLOCK_SAMPLES],
    }

    #[derive(Debug, thiserror::Error)]
    pub enum CaptureError {
        #[error("audio input queue overflowed; capture stopped to avoid splicing samples")]
        Overflow,
        #[error("audio device stream failed or disconnected")]
        DeviceFailed,
    }

    pub struct Capture {
        _stream: cpal::Stream,
        blocks: Receiver<Block>,
        fault: Arc<AtomicU8>,
    }

    pub fn input_devices() -> Result<Vec<String>, Box<dyn std::error::Error>> {
        cpal::default_host()
            .input_devices()?
            .map(|device| device.name().map_err(Into::into))
            .collect()
    }

    impl Capture {
        /// Select an exact device name or the default device and a zero-based
        /// channel. Requires native 48 kHz support; no implicit resampling.
        pub fn start(name: Option<&str>, channel: u16) -> Result<Self, Box<dyn std::error::Error>> {
            let host = cpal::default_host();
            let device = if let Some(name) = name {
                host.input_devices()?
                    .find(|device| device.name().ok().as_deref() == Some(name))
            } else {
                host.default_input_device()
            }
            .ok_or("requested audio input device is unavailable")?;
            let supported = device
                .supported_input_configs()?
                .find(|config| {
                    config.channels() > channel
                        && config.min_sample_rate().0 <= 48_000
                        && config.max_sample_rate().0 >= 48_000
                        && matches!(
                            config.sample_format(),
                            cpal::SampleFormat::F32
                                | cpal::SampleFormat::I16
                                | cpal::SampleFormat::U16
                        )
                })
                .ok_or("input device needs a supported f32/i16/u16 channel at 48 kHz")?
                .with_sample_rate(cpal::SampleRate(48_000));
            let (send, blocks) = mpsc::sync_channel(64);
            let fault = Arc::new(AtomicU8::new(0));
            let config = supported.config();
            let stream = match supported.sample_format() {
                cpal::SampleFormat::F32 => {
                    stream::<f32>(&device, &config, channel, send, fault.clone())?
                }
                cpal::SampleFormat::I16 => {
                    stream::<i16>(&device, &config, channel, send, fault.clone())?
                }
                cpal::SampleFormat::U16 => {
                    stream::<u16>(&device, &config, channel, send, fault.clone())?
                }
                _ => unreachable!("filtered sample formats"),
            };
            stream.play()?;
            Ok(Self {
                _stream: stream,
                blocks,
                fault,
            })
        }

        /// A timeout lets the worker check its own deadline. Any capture gap
        /// is terminal: callers must create a fresh receiver and capture.
        pub fn read(&self, timeout: Duration) -> Result<Option<Block>, CaptureError> {
            self.check_fault()?;
            let block = match self.blocks.recv_timeout(timeout) {
                Ok(block) => Some(block),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(CaptureError::DeviceFailed);
                }
            };
            self.check_fault()?;
            Ok(block)
        }

        fn check_fault(&self) -> Result<(), CaptureError> {
            match self.fault.load(Ordering::Acquire) {
                0 => Ok(()),
                1 => Err(CaptureError::Overflow),
                _ => Err(CaptureError::DeviceFailed),
            }
        }
    }

    fn stream<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        channel: u16,
        send: SyncSender<Block>,
        fault: Arc<AtomicU8>,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: SizedSample,
        f32: cpal::FromSample<T>,
    {
        let mut input = InputBuffer {
            channels: usize::from(config.channels),
            channel: usize::from(channel),
            samples: [0.0; BLOCK_SAMPLES],
            filled: 0,
            send,
            fault: fault.clone(),
        };
        device.build_input_stream(
            config,
            move |data: &[T], _| input.push(data),
            move |_| {
                fault.store(2, Ordering::Release);
            },
            None,
        )
    }
    struct InputBuffer {
        channels: usize,
        channel: usize,
        samples: [f32; BLOCK_SAMPLES],
        filled: usize,
        send: SyncSender<Block>,
        fault: Arc<AtomicU8>,
    }

    impl InputBuffer {
        fn push<T: SizedSample>(&mut self, data: &[T])
        where
            f32: cpal::FromSample<T>,
        {
            if self.fault.load(Ordering::Relaxed) != 0 {
                return;
            }
            if !data.len().is_multiple_of(self.channels) {
                self.fault.store(2, Ordering::Release);
                return;
            }
            for frame in data.chunks_exact(self.channels) {
                self.samples[self.filled] = frame[self.channel].to_sample::<f32>();
                self.filled += 1;
                if self.filled == BLOCK_SAMPLES {
                    if self
                        .send
                        .try_send(Block {
                            samples: self.samples,
                        })
                        .is_err()
                    {
                        self.fault.store(1, Ordering::Release);
                        return;
                    }
                    self.filled = 0;
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn input(capacity: usize) -> (InputBuffer, Receiver<Block>) {
            let (send, receive) = mpsc::sync_channel(capacity);
            (
                InputBuffer {
                    channels: 2,
                    channel: 1,
                    samples: [0.0; BLOCK_SAMPLES],
                    filled: 0,
                    send,
                    fault: Arc::new(AtomicU8::new(0)),
                },
                receive,
            )
        }

        #[test]
        fn selects_one_channel_and_preserves_partial_blocks() {
            let (mut input, receive) = input(2);
            let audio: Vec<i16> = (0..BLOCK_SAMPLES).flat_map(|_| [0, 16384]).collect();
            input.push(&audio[..126]);
            assert!(receive.try_recv().is_err());
            input.push(&audio[126..]);
            assert!(
                receive
                    .try_recv()
                    .unwrap()
                    .samples
                    .iter()
                    .all(|&sample| sample == 0.5)
            );
            input.push(&vec![32768u16; BLOCK_SAMPLES * 2]);
            assert!(
                receive
                    .try_recv()
                    .unwrap()
                    .samples
                    .iter()
                    .all(|&sample| sample == 0.0)
            );
        }

        #[test]
        fn overflow_and_malformed_frames_latch_a_terminal_fault() {
            let (mut input, receive) = input(1);
            input.push(&vec![0.5f32; BLOCK_SAMPLES * 4]);
            assert_eq!(input.fault.load(Ordering::Acquire), 1);
            receive.try_recv().unwrap();
            input.push(&vec![0.5f32; BLOCK_SAMPLES * 2]);
            assert!(receive.try_recv().is_err());
            let (mut input, _) = self::input(1);
            input.push(&[0.0f32]);
            assert_eq!(input.fault.load(Ordering::Acquire), 2);
        }
    }
}
