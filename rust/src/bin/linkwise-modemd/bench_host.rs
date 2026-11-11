//! Foreground loopback host harness. The virtual peer echoes acknowledged DATA.
use std::{io, net::SocketAddr, time::Duration};

use async_trait::async_trait;
use linkwise::{
    api::Command,
    host::{Channel, ModemPort, Reply, tcp::Listener},
    modem::{
        arq::Answer,
        runtime::{Limits, State, device::Transmission},
        setup::{LinkSetup, Profile},
    },
};
use tokio::sync::{Mutex, mpsc};

use super::{SimulatedPair, complete};

struct BenchPort {
    local: String,
    pair: Mutex<Option<SimulatedPair>>,
    notices: mpsc::Sender<Vec<u8>>,
    received: mpsc::Sender<Vec<u8>>,
}

impl BenchPort {
    fn notice(&self, text: String) -> io::Result<()> {
        self.notices
            .try_send(text.into_bytes())
            .map_err(|e| io::Error::other(e.to_string()))
    }

    async fn stop(&self) -> Result<(), String> {
        if let Some(pair) = self.pair.lock().await.take() {
            pair.close().await?;
        }
        Ok(())
    }
}

#[async_trait]
impl ModemPort for BenchPort {
    async fn command(&self, command: Command) -> Reply {
        match command {
            Command::Version => Reply::Version("LINKWISE SIMULATED BENCH".into()),
            Command::MyCall(calls) if calls.as_slice() == [self.local.as_str()] => Reply::Ok,
            Command::Bw500 | Command::CompressionOff | Command::Listen(false) => Reply::Ok,
            Command::Connect {
                source,
                destination,
            } if source == self.local => {
                let mut guard = self.pair.lock().await;
                if guard.is_some() {
                    return Reply::Wrong;
                }
                let Ok(pair) =
                    SimulatedPair::connect(Limits::default(), &source, &destination).await
                else {
                    return Reply::Wrong;
                };
                if self
                    .notice(format!("CONNECTED {source} {destination} 500\r"))
                    .is_err()
                {
                    let _ = pair.close().await;
                    return Reply::Wrong;
                }
                *guard = Some(pair);
                Reply::Ok
            }
            Command::Abort | Command::Disconnect => {
                if self.stop().await.is_ok() && self.notice("DISCONNECTED\r".into()).is_ok() {
                    Reply::Ok
                } else {
                    Reply::Wrong
                }
            }
            Command::ClearTxBuffer if self.pair.lock().await.is_none() => Reply::Ok,
            _ => Reply::Wrong,
        }
    }

    async fn data(&self, bytes: &[u8]) -> io::Result<()> {
        let guard = self.pair.lock().await;
        let pair = guard.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "bench session is disconnected")
        })?;
        // Reserve delivery space before accepting bytes into the runtime. The
        // DATA stream drains queued output before calling us again.
        let delivery = self.received.try_reserve().map_err(io::Error::other)?;
        let before = pair.caller.snapshot().await.map_err(debug_error)?;
        if before.state != State::Connected {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "bench session expired",
            ));
        }
        pair.caller
            .enqueue(pair.owner, pair.caller_session, bytes)
            .await
            .map_err(debug_error)?
            .map_err(debug_error)?;
        self.notice(format!("BUFFER {}\r", bytes.len()))?;
        let (id, tx) = complete(&pair.caller).await.map_err(io::Error::other)?;
        let Transmission::Data(echo) = tx else {
            return Err(io::Error::other("bench expected a DATA transmission"));
        };
        pair.caller
            .answer(id, Answer::Delivered)
            .await
            .map_err(debug_error)?
            .map_err(debug_error)?;
        let after = pair.caller.snapshot().await.map_err(debug_error)?;
        if after
            .bytes
            .acknowledged
            .checked_sub(before.bytes.acknowledged)
            != Some(bytes.len())
        {
            return Err(io::Error::other("bench DATA was not acknowledged"));
        }
        self.notice("BUFFER 0\r".into())?;
        delivery.send(echo);
        Ok(())
    }

    async fn closed(&self, _: Channel) {
        if let Err(error) = self.stop().await {
            eprintln!("Bench cleanup: {error}");
        }
    }
}

fn debug_error(error: impl std::fmt::Debug) -> io::Error {
    io::Error::other(format!("{error:?}"))
}

pub async fn serve(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (call, control, data) = match args {
        [call] => (call, 8300, 8301),
        [call, control, data] => (call, control.parse()?, data.parse()?),
        _ => return Err("usage: --serve-bench CALL [CONTROL_PORT DATA_PORT]".into()),
    };
    let call = LinkSetup {
        caller: call.clone(),
        high_speed: false,
        requested_level: 4,
    };
    let local = call
        .encode(Profile::Bw500)
        .and_then(|frame| LinkSetup::decode(Profile::Bw500, &frame))
        .ok_or("invalid bench callsign")?
        .caller;
    let mut listener = Listener::bind(
        SocketAddr::from(([127, 0, 0, 1], control)),
        SocketAddr::from(([127, 0, 0, 1], data)),
        Duration::from_secs(5),
    )
    .await?;
    let (control, data) = listener.addresses()?;
    println!("SIMULATED BENCH {local}. Command {control}, DATA {data}.");
    loop {
        let (notices, control_rx) = mpsc::channel(32);
        let (received, data_rx) = mpsc::channel(8);
        let port = BenchPort {
            local: local.clone(),
            pair: Mutex::new(None),
            notices,
            received,
        };
        if let Err(error) = listener.serve_one(&port, control_rx, data_rx).await {
            eprintln!("Bench connection ended: {error}");
        }
    }
}
