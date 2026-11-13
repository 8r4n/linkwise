//! Play the known BW500 DATA fixture into an explicitly selected virtual cable.
#[cfg(feature = "audio-device")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use linkwise::{
        audio::playback::{Playback, Progress},
        modem::bw500::{Frame, Level, synthesize},
    };
    use std::time::{Duration, Instant};

    let output = std::env::args()
        .nth(1)
        .ok_or("Specify the VB-Audio playback endpoint")?;
    if !output.contains("(VB-Audio Virtual Cable)") {
        return Err("Select a VB-Audio virtual endpoint".into());
    }
    let frame = Frame {
        payload: (0..22).collect(),
        control: 0x99,
    };
    let mut samples = vec![0.0; 48_000];
    samples.extend(synthesize(Level::Two, &[frame]).ok_or("Could not synthesize fixture")?);
    samples.resize(samples.len() + 96_000, 0.0);
    let mut playback = Playback::start(&output, 0, samples)?;
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        if playback.progress()? == Progress::EstimateElapsed {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err("Playback timed out".into())
}

#[cfg(not(feature = "audio-device"))]
fn main() {
    eprintln!("Build with --features audio-device");
    std::process::exit(2);
}
