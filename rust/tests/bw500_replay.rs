use std::{env, path::PathBuf, process::Command};

use serde_json::Value;

#[test]
#[ignore = "requires the externally downloaded and verified BW500 WAV excerpts"]
fn decodes_published_bw500_payloads() {
    let fixture_dir =
        PathBuf::from(env::var_os("LINKWISE_BW500_FIXTURES").expect("set LINKWISE_BW500_FIXTURES"));
    let fixtures = [
        ("bw500-level1-arm1", 263_369, 1, &["524d53205472696d6f"][..]),
        ("bw500-level1-arm2", 263_355, 1, &["524d53205472696d6f"][..]),
        (
            "bw500-level2-arm1",
            242_941,
            2,
            &["646520312e342e332e302057616473776f7274682049"][..],
        ),
        (
            "bw500-level2-arm2",
            242_923,
            2,
            &["646520312e342e332e302057616473776f7274682049"][..],
        ),
        (
            "bw500-level1-two-frames",
            518_199,
            1,
            &["323520666565742077", "697468203530207761"][..],
        ),
    ];

    for (fixture, expected_end, level, payloads) in fixtures {
        let output = Command::new(env!("CARGO_BIN_EXE_linkwise"))
            .arg("decode-wav")
            .arg(fixture_dir.join(format!("{fixture}.wav")))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{fixture}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["sample_rate_hz"], 48_000, "{fixture}");
        let candidates = report["burst_candidates"].as_array().unwrap();
        assert_eq!(candidates.len(), 1, "{fixture}: {candidates:?}");
        let start = candidates[0]["start"].as_u64().unwrap();
        let end = candidates[0]["end"].as_u64().unwrap();
        const WINDOW: u64 = 480;
        assert!(start.abs_diff(4800) <= WINDOW, "{fixture} start: {start}");
        assert!(end.abs_diff(expected_end) <= WINDOW, "{fixture} end: {end}");

        let frames = report["bw500_data_frames"].as_array().unwrap();
        assert_eq!(frames.len(), payloads.len(), "{fixture}: {frames:?}");
        for (frame, payload) in frames.iter().zip(payloads) {
            assert_eq!(frame["level"], level, "{fixture}");
            assert_eq!(frame["control"], 0x99, "{fixture}");
            assert_eq!(frame["payload_hex"], *payload, "{fixture}");
        }
    }
}

#[test]
#[ignore = "requires the local stock VARA multi-frame and retry captures"]
fn decodes_stock_bw500_delivery_and_level_fallback() {
    use linkwise::modem::{
        bw500_waveform::Waveform,
        data,
        receiver::{Receiver, Reception},
    };

    let root = PathBuf::from(
        env::var_os("LINKWISE_STOCK_BW500_CAPTURE_DIR")
            .expect("set LINKWISE_STOCK_BW500_CAPTURE_DIR"),
    );
    let profile = std::fs::read_to_string(root.join("harness/profile.json")).unwrap();
    let expected: Vec<u8> = (0..200).collect();

    for (role, rejected_first, expected_levels) in [
        ("macos/responder", false, &[4, 4, 4, 4, 4][..]),
        ("macos/recover", true, &[3, 3, 3, 3, 3, 3][..]),
        ("ubuntu/responder-initial", false, &[4, 4, 4, 4, 4][..]),
    ] {
        let mut captures: Vec<_> = root
            .join("cases")
            .join(role)
            .read_dir()
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .ends_with("-receive-data-from-vara.wav")
            })
            .collect();
        captures.sort();
        let mut bytes = Vec::new();
        let mut levels = Vec::new();
        let mut rejected = false;
        for path in captures {
            let mut wav = hound::WavReader::open(path).unwrap();
            assert_eq!(wav.spec().sample_rate, 48_000);
            let channels = usize::from(wav.spec().channels);
            let audio: Vec<f32> = wav
                .samples::<f32>()
                .step_by(channels)
                .map(Result::unwrap)
                .collect();
            let mut receiver = Receiver::with_level4(Waveform::from_json(&profile).unwrap());
            let mut collect = |reception: Reception| {
                for received in reception.frames {
                    let level = received.level;
                    let Some(frame) = data::decode_received(level, &received.frame, "KN6MQS")
                    else {
                        continue;
                    };
                    if rejected_first && !rejected {
                        assert_eq!(level, 4);
                        assert_eq!(frame.payload, expected[..43]);
                        rejected = true;
                        continue;
                    }
                    levels.push(level);
                    bytes.extend(frame.payload);
                }
            };
            receiver.push(&audio, &mut collect).unwrap();
            receiver.finish(collect);
        }
        assert_eq!(rejected, rejected_first, "{role}");
        assert_eq!(levels, expected_levels, "{role}");
        assert_eq!(bytes, expected, "{role}");
    }
}
