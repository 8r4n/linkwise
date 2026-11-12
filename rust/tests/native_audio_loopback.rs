#![cfg(feature = "audio-device")]

use linkwise::{
    audio::{
        device::Capture,
        playback::{Playback, Progress},
    },
    modem::{
        control::{Bandwidth, Message},
        waveform::BurstDetector,
    },
};
use std::{
    error::Error,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires explicitly configured virtual capture and playback devices"]
fn decodes_bw500_request_through_native_audio() -> Result<(), Box<dyn Error>> {
    let input = std::env::var("LINKWISE_LOOPBACK_INPUT")?;
    let output = std::env::var("LINKWISE_LOOPBACK_OUTPUT")?;
    let request = Message::ConnectRequest(Bandwidth::Hz500);
    let mut stimulus = vec![0.0; 12_000];
    stimulus.extend(request.synthesize("W1AW").expect("valid fixture callsign"));
    stimulus.resize(stimulus.len() + 12_000, 0.0);

    let capture = Capture::start(Some(&input), 0)?;
    let mut playback = Playback::start(&output, 0, stimulus)?;
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut detector = BurstDetector::new(48_000);
    let mut recording = Vec::new();
    let mut candidates = Vec::new();

    while Instant::now() < deadline {
        if let Some(block) = capture.read(Duration::from_millis(100))? {
            recording.extend_from_slice(&block.samples);
            detector.push(&block.samples, |range| candidates.push(range));
        }
        if playback.progress()? == Progress::EstimateElapsed && !candidates.is_empty() {
            break;
        }
    }

    assert_eq!(playback.progress()?, Progress::EstimateElapsed);
    assert!(
        candidates.iter().any(|range| {
            request
                .find("W1AW", &recording, range.start as usize)
                .is_some()
        }),
        "no BW500 request decoded from {output:?} to {input:?}: {candidates:?}"
    );
    Ok(())
}
