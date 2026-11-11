//! Foreground modem simulation and loopback host harness.
#[path = "linkwise-modemd/bench_host.rs"]
mod bench_host;
use linkwise::modem::{
    arq::Answer,
    runtime::{
        Admission, Limits, Runtime, State,
        device::{SimulatedDevice, Transmission},
        runner::{self, Handle},
    },
};
use std::{num::NonZeroUsize, time::Duration};
use uuid::Uuid;

type Modem = Handle<SimulatedDevice>;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] || args == ["-h"] {
        println!(
            "Usage: linkwise-modemd --simulate | --simulate-audio PROFILE | --serve-bench CALL [CONTROL_PORT DATA_PORT]\n\nRuns two clock-driven session owners through establishment, binary DATA,\na lost answer and a state query. Uses simulated protocol/device events only;\naudio, radio and Winlink devices remain outside this simulation.\n\n--simulate-audio runs the two-way BW500 audio bench with a measured profile.\n--serve-bench serves one local TCP command/data pair with a simulated echo peer."
        );
        return;
    }
    if let [mode, path] = args.as_slice() {
        if mode == "--simulate-audio" {
            if let Err(error) = simulate_audio(path) {
                eprintln!("Audio simulation failed: {error}");
                std::process::exit(1);
            }
            return;
        }
    }
    if args.first().is_some_and(|arg| arg == "--serve-bench") {
        if let Err(error) = bench_host::serve(&args[1..]).await {
            eprintln!("Bench host failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    if args != ["--simulate"] {
        eprintln!(
            "Usage: linkwise-modemd --simulate | --simulate-audio PROFILE | --serve-bench CALL [CONTROL_PORT DATA_PORT] (or --help)"
        );
        std::process::exit(2);
    }
    if let Err(error) = simulate().await {
        eprintln!("Simulation failed: {error}");
        std::process::exit(1);
    }
}

async fn drain(modem: &Modem) -> Result<(), String> {
    while modem
        .pop_event()
        .await
        .map_err(|e| format!("{e:?}"))?
        .is_some()
    {}
    Ok(())
}

async fn complete(
    modem: &Modem,
) -> Result<(linkwise::modem::runtime::TransmissionId, Transmission), String> {
    let tx = modem
        .complete_simulated_transmission()
        .await
        .map_err(|e| format!("{e:?}"))?
        .map_err(|e| format!("{e:?}"))?
        .ok_or("no pending transmission")?;
    drain(modem).await?;
    Ok(tx)
}

async fn simulate() -> Result<(), String> {
    let limits = Limits {
        answer_timeout: Duration::from_millis(100),
        ..Limits::default()
    };
    let pair = SimulatedPair::connect(limits, "W9SSJ", "W1AW").await?;
    let caller = &pair.caller;
    let a = pair.caller_session;
    let owner = pair.owner;
    let bytes: Vec<u8> = (0..=255).collect();
    caller
        .enqueue(owner, a, &bytes)
        .await
        .map_err(|e| format!("{e:?}"))?
        .map_err(|e| format!("{e:?}"))?;
    let (_, Transmission::Data(received)) = complete(caller).await? else {
        return Err("expected DATA intent".into());
    };
    if bytes != received {
        return Err("binary payload mismatch".into());
    }
    // The bench peer accepts this intent but deliberately drops its first
    // answer. This is synthetic evidence, not a wire reply decoder.
    // Wait for the runner's own timer to publish the query transmission.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(linkwise::modem::runtime::Event::Transmitting(_)) =
                caller.pop_event().await.map_err(|e| format!("{e:?}"))?
            {
                return Ok::<(), String>(());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .map_err(|_| "runner did not schedule a state query")??;
    let (query, Transmission::StateQuery) = complete(caller).await? else {
        return Err("missing answer did not produce a state query".into());
    };
    caller
        .answer(query, Answer::Delivered)
        .await
        .map_err(|e| format!("{e:?}"))?
        .map_err(|e| format!("{e:?}"))?;
    let snapshot = caller.snapshot().await.map_err(|e| format!("{e:?}"))?;
    if !snapshot.bytes.balanced()
        || snapshot.bytes.acknowledged != 256
        || snapshot.bytes.transmit_attempts != 1
    {
        return Err("byte accounting failed".into());
    }
    println!(
        "SIMULATION ONLY: two-role establishment; 256 binary bytes; lost answer recovered by state query."
    );
    println!("{snapshot:#?}");
    pair.close().await
}

struct SimulatedPair {
    caller: Modem,
    responder: Modem,
    owner: Uuid,
    caller_session: Uuid,
    responder_session: Uuid,
    caller_task: tokio::task::JoinHandle<Runtime<SimulatedDevice>>,
    responder_task: tokio::task::JoinHandle<Runtime<SimulatedDevice>>,
}

impl SimulatedPair {
    async fn connect(limits: Limits, local: &str, remote: &str) -> Result<Self, String> {
        let owner = Uuid::new_v4();
        let capacity = NonZeroUsize::new(8).unwrap();
        let (caller, caller_task) = runner::spawn(limits, SimulatedDevice::default(), capacity)
            .map_err(|e| format!("{e:?}"))?;
        let (responder, responder_task) =
            runner::spawn(limits, SimulatedDevice::default(), capacity)
                .map_err(|e| format!("{e:?}"))?;
        let admission = |local: &str, remote: Option<&str>| Admission {
            owner,
            operation: Uuid::new_v4(),
            local_call: local.into(),
            remote_call: remote.map(str::to_owned),
            lease_expires: Duration::from_secs(60),
        };
        let a = caller
            .begin(admission(local, Some(remote)))
            .await
            .map_err(|e| format!("{e:?}"))?
            .map_err(|e| format!("{e:?}"))?;
        let b = responder
            .begin(admission(remote, None))
            .await
            .map_err(|e| format!("{e:?}"))?
            .map_err(|e| format!("{e:?}"))?;
        for caller_turn in [true, false, true, false, true] {
            let (from, to, session) = if caller_turn {
                (&caller, &responder, b)
            } else {
                (&responder, &caller, a)
            };
            let (_, Transmission::Establishment(burst)) = complete(from).await? else {
                return Err("expected establishment burst".into());
            };
            to.receive_handshake(session, burst)
                .await
                .map_err(|e| format!("{e:?}"))?
                .map_err(|e| format!("{e:?}"))?;
            drain(to).await?;
        }
        if caller.snapshot().await.map_err(|e| format!("{e:?}"))?.state != State::Connected
            || responder
                .snapshot()
                .await
                .map_err(|e| format!("{e:?}"))?
                .state
                != State::Connected
        {
            return Err("establishment did not complete".into());
        }

        Ok(Self {
            caller,
            responder,
            owner,
            caller_session: a,
            responder_session: b,
            caller_task,
            responder_task,
        })
    }

    async fn close(self) -> Result<(), String> {
        let Self {
            caller,
            responder,
            owner,
            caller_session,
            responder_session,
            caller_task,
            responder_task,
        } = self;
        let a = caller.abort(owner, caller_session).await;
        let b = responder.abort(owner, responder_session).await;
        drop((caller, responder));
        let (caller, responder) = tokio::join!(caller_task, responder_task);
        for runtime in [caller, responder] {
            let runtime = runtime.map_err(|e| e.to_string())?;
            if runtime.snapshot().release_pending {
                return Err("simulated release still pending".into());
            }
        }
        for result in [a, b] {
            result
                .map_err(|e| format!("{e:?}"))?
                .map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    }
}

fn simulate_audio(path: &str) -> Result<(), String> {
    use linkwise::modem::{bw500_waveform::Waveform, runtime::audio_loopback};
    let profile = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let waveform = Waveform::from_json(&profile)?;
    let outgoing: Vec<u8> = (0..79).map(|n| (n * 19) as u8).collect();
    let incoming: Vec<u8> = (0..70).map(|n| (255 - n * 11) as u8).collect();
    let report = audio_loopback::run(&waveform, &outgoing, &incoming)?;
    println!(
        "BW500 AUDIO BENCH: {} bytes to responder, {} bytes to caller. DATA attempts {} / {}, {} queries, {} duplicate deliveries suppressed.",
        report.responder_received.len(),
        report.caller_received.len(),
        report.caller_data_attempts,
        report.responder_data_attempts,
        report.state_queries,
        report.duplicate_frames
    );
    Ok(())
}
