use std::{env, fs, path::PathBuf};

use linkwise::modem::{
    control::Bandwidth,
    setup::{LinkSetup, Profile},
    waveform::BurstDetector,
    wide_level4::Waveform,
};

#[test]
#[ignore = "requires the local stock VARA BW2300 and BW2750 WAV captures"]
fn decodes_stock_level4_setup_in_both_wide_modes() {
    let root = PathBuf::from(
        env::var_os("LINKWISE_STOCK_WIDE_CAPTURE_DIR")
            .expect("set LINKWISE_STOCK_WIDE_CAPTURE_DIR"),
    );
    let expected = LinkSetup {
        caller: "KN6MQS".into(),
        high_speed: false,
        requested_level: 4,
    };
    for (mode, bandwidth) in [(2300, Bandwidth::Hz2300), (2750, Bandwidth::Hz2750)] {
        let waveform = Waveform::new(bandwidth).unwrap();
        for platform in ["macos", "ubuntu", "fedora"] {
            let case = root.join(platform).join(format!("responder-{mode}"));
            let captures: Vec<_> = fs::read_dir(&case)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.to_string_lossy()
                        .ends_with("-wide-reply-from-vara.wav")
                })
                .collect();
            assert_eq!(captures.len(), 1, "{}", case.display());
            let mut wav = hound::WavReader::open(&captures[0]).unwrap();
            let spec = wav.spec();
            assert_eq!(spec.sample_rate, 48_000);
            let audio: Vec<_> = wav
                .samples::<f32>()
                .step_by(usize::from(spec.channels))
                .collect::<Result<_, _>>()
                .unwrap();
            let mut detector = BurstDetector::new(spec.sample_rate);
            let mut candidates = Vec::new();
            detector.push(&audio, |range| candidates.push(range));
            detector.finish(|range| candidates.push(range));
            let decoded = candidates.into_iter().find_map(|range| {
                let frame = waveform.decode(&audio, range.start as usize..range.end as usize)?;
                LinkSetup::decode(Profile::Wide, &frame)
            });
            assert_eq!(decoded.as_ref(), Some(&expected), "{}", case.display());
        }
    }
}
