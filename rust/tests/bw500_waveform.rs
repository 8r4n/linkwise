use linkwise::modem::{
    bw500::{self, Frame, Level},
    bw500_waveform::Waveform,
    data,
    setup::{LinkSetup, Profile},
};

// Emit bit 0, shift right, insert the parity of the taps, and pack MSB-first.
fn packed_prbs(degree: u32, taps: u32, seed: u32, bytes: usize) -> Vec<u8> {
    let mut state = seed;
    (0..bytes)
        .map(|_| {
            let mut byte = 0;
            for _ in 0..8 {
                byte = (byte << 1) | (state & 1) as u8;
                let feedback = (state & taps).count_ones() & 1;
                state = (state >> 1) | (feedback << (degree - 1));
            }
            byte
        })
        .collect()
}

fn demo_prbs9(bytes: usize) -> Vec<u8> {
    // Same polynomial and initial phase as the Linkwise Winlink demo.
    packed_prbs(9, 0b011010001, 0x1ff, bytes)
}

#[test]
fn packed_prbs9_matches_the_winlink_demo_attachment() {
    // One 511-bit period followed by the first bit to fill the last byte.
    let actual: String = demo_prbs9(64)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        actual,
        concat!(
            "ff94c59b819974a3c6a968def9f117149b3cd7d8216d348c950248741e8805ea7",
            "564155cef225fddcb185099c9ed6a2bc3431dbaecadf836c4e11cc071f4fd61",
        )
    );
}

#[test]
fn bw500_data_patterns_survive_attenuation_and_a_fixed_echo() {
    const PREFIX: usize = 3217;
    const ECHO_DELAY: usize = 48; // 1 ms at the modem's 48 kHz sample rate.
    for (level, size) in [(Level::One, 9), (Level::Two, 22), (Level::Three, 34)] {
        let patterns = [
            ("PRBS9", demo_prbs9(size)),
            // x^15 + x^14 + 1 in the right-shifting convention above.
            ("PRBS15", packed_prbs(15, (1 << 14) | 1, 0x7fff, size)),
            ("dotting", vec![0xaa; size]),
            ("zeros", vec![0; size]),
            ("ones", vec![0xff; size]),
        ];
        let frames: Vec<_> = patterns
            .iter()
            .map(|(_, payload)| Frame {
                payload: payload.clone(),
                control: 0x99,
            })
            .collect();
        let burst = bw500::synthesize(level, &frames).unwrap();
        let mut audio = vec![0.0; PREFIX + burst.len() + ECHO_DELAY];
        for (i, sample) in burst.into_iter().enumerate() {
            audio[PREFIX + i] += 0.5 * sample;
            audio[PREFIX + i + ECHO_DELAY] += 0.05 * sample;
        }
        let received = bw500::decode_burst(&audio, PREFIX - 97..audio.len());
        assert_eq!(received.len(), patterns.len(), "{level:?}");
        for (received, ((pattern, _), expected)) in received.iter().zip(patterns.iter().zip(frames))
        {
            assert_eq!(received.level, level, "{pattern}");
            assert_eq!(received.frame, expected, "{level:?} {pattern}");
        }
    }
}

#[test]
#[ignore = "requires the verified external level-4 measurement profile"]
fn demo_prbs9_round_trips_through_bw500_full_and_closing_data_audio() {
    let path = std::env::var("LINKWISE_BW500_PROFILE").expect("set LINKWISE_BW500_PROFILE");
    let waveform = Waveform::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
    let payload = demo_prbs9(64);
    let mut received = Vec::new();
    let (full, closing) = payload.split_at(43);
    let frames = [
        (data::full(4, full, 0x99).unwrap(), None),
        (data::short(4, closing, "W9SSJ", 4).unwrap(), Some(4)),
    ];
    for (frame, next_level) in frames {
        let frame = frame.try_into().unwrap();
        let mut audio = vec![0.0; 1603];
        audio.extend(
            waveform
                .synthesize(&frame)
                .unwrap()
                .into_iter()
                .map(|s| s * 0.35),
        );
        let end = audio.len();
        audio.extend([0.0; 960]);
        let decoded = waveform
            .decode(&audio, 5920..end)
            .expect("level-4 DATA decode");
        assert_eq!(decoded.frame, frame);
        let decoded = data::decode(4, &decoded.frame, "W9SSJ").unwrap();
        assert_eq!(decoded.next_receive_level, next_level);
        received.extend(decoded.payload);
    }
    assert_eq!(received, payload);
}

#[test]
#[ignore = "requires the verified external level-4 measurement profile"]
fn setup_frame_round_trips_through_level4_audio() {
    let path = std::env::var("LINKWISE_BW500_PROFILE").expect("set LINKWISE_BW500_PROFILE");
    let profile = std::fs::read_to_string(path).unwrap();
    let waveform = Waveform::from_json(&profile).unwrap();
    let frame: [u8; 46] = LinkSetup {
        caller: "W9SSJ".into(),
        high_speed: false,
        requested_level: 4,
    }
    .encode(Profile::Bw500)
    .unwrap()
    .try_into()
    .unwrap();
    let synthesized = waveform.synthesize(&frame).unwrap();
    let mut audio = vec![0.0; 1603];
    audio.extend(synthesized.iter().map(|sample| sample * 0.35));
    audio.extend([0.0; 960]);
    let decoded = waveform
        .decode(&audio, 5920..audio.len() - 960)
        .expect("level-4 setup decode");
    assert_eq!(decoded.frame, frame);
    let setup = LinkSetup::decode(Profile::Bw500, &decoded.frame).unwrap();
    assert_eq!(setup.caller, "W9SSJ");
    let mut malformed: serde_json::Value = serde_json::from_str(&profile).unwrap();
    malformed["grid"].as_array_mut().unwrap().pop();
    assert!(Waveform::from_json(&malformed.to_string()).is_err());
}

#[test]
#[ignore = "requires the verified external level-4 measurement profile"]
fn two_roles_establish_only_after_audio_confirm_and_transmit_completion() {
    use linkwise::modem::{
        audio_handshake::{Action, AudioHandshake},
        handshake::Event,
    };
    use std::time::Duration;
    let path = std::env::var("LINKWISE_BW500_PROFILE").unwrap();
    let profile = std::fs::read_to_string(path).unwrap();
    let station = |call: &str, listen| {
        AudioHandshake::new(
            LinkSetup {
                caller: call.into(),
                high_speed: false,
                requested_level: 4,
            },
            listen,
            Duration::from_secs(10),
            Waveform::from_json(&profile).unwrap(),
        )
        .unwrap()
    };
    let mut stations = [station("W9SSJ", false), station("W1AW", true)];
    let mut action = stations[0]
        .handle(
            Duration::ZERO,
            Event::Call {
                called: "W1AW".into(),
            },
        )
        .unwrap();
    let mut sender = 0;
    let mut now = Duration::ZERO;
    let mut established = [false; 2];
    for step in 0..5 {
        let Action::Transmit { token, audio } = action else {
            panic!("expected transmission at step {step}");
        };
        now += Duration::from_secs_f64(audio.len() as f64 / 48_000.0);
        if let Some(completion) = stations[sender].handle(
            now,
            Event::Transmitted {
                token,
                success: true,
            },
        ) {
            assert_eq!(step, 4);
            assert!(matches!(completion, Action::Established(_)));
            established[sender] = true;
        }
        now += Duration::from_millis(85);
        let mut padded = vec![0.0; 73];
        padded.extend(audio);
        padded.extend([0.0; 480]);
        let received = stations[1 - sender]
            .receive(now, &padded, 0..padded.len())
            .expect("expected audio response");
        if step == 4 {
            assert!(matches!(received, Action::Established(_)));
            established[1 - sender] = true;
            assert!(established.into_iter().all(|value| value));
            return;
        }
        assert_eq!(established, [false; 2]);
        action = received;
        sender = 1 - sender;
    }
    panic!("confirm did not establish the session");
}

#[test]
#[ignore = "requires the verified external level-4 measurement profile"]
fn level4_streaming_receiver_preserves_data_and_rejects_corrupted_audio() {
    use linkwise::modem::{data, receiver::Receiver};
    let path = std::env::var("LINKWISE_BW500_PROFILE").unwrap();
    let profile = std::fs::read_to_string(path).unwrap();
    let waveform = Waveform::from_json(&profile).unwrap();
    let wire: [u8; 46] = data::full(4, &(0..43).collect::<Vec<_>>(), 0x95)
        .unwrap()
        .try_into()
        .unwrap();
    let clean = waveform.synthesize(&wire).unwrap();
    let mut receiver = Receiver::with_level4(Waveform::from_json(&profile).unwrap());
    let mut events = Vec::new();
    for chunk in clean.chunks(127) {
        receiver.push(chunk, |event| events.push(event)).unwrap();
    }
    receiver.finish(|event| events.push(event));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].frames.len(), 1);
    assert_eq!(events[0].frames[0].level, 4);
    assert_eq!(
        events[0].frames[0].frame.payload,
        (0..43).collect::<Vec<_>>()
    );
    assert_eq!(events[0].frames[0].frame.control, 0x95);
    let mut corrupted = clean;
    corrupted[20_000..160_000].fill(0.0);
    assert!(waveform.decode(&corrupted, 4320..corrupted.len()).is_none());
}

#[test]
#[ignore = "requires the verified external level-4 measurement profile"]
fn two_runtimes_exchange_audio_and_recover_erased_answer_and_data() {
    use linkwise::modem::runtime::audio_loopback;

    let profile =
        std::fs::read_to_string(std::env::var("LINKWISE_BW500_PROFILE").unwrap()).unwrap();
    let waveform = Waveform::from_json(&profile).unwrap();
    let outgoing: Vec<u8> = (0..79).map(|n| (n * 19) as u8).collect();
    let incoming: Vec<u8> = (0..70).map(|n| (255 - n * 11) as u8).collect();
    let report = audio_loopback::run(&waveform, &outgoing, &incoming).unwrap();
    assert_eq!(report.responder_received, outgoing);
    assert_eq!(report.caller_received, incoming);
    assert_eq!(report.caller_data_attempts, 3);
    assert_eq!(report.responder_data_attempts, 4);
    assert_eq!(report.state_queries, 2);
    assert_eq!(report.duplicate_frames, 2);
}
