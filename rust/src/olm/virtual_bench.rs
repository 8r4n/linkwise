//! One station-owned, radio-free B2F exchange through two real BW500 engines.
use super::{Error, Outcome, Result, inventory::VirtualBenchConfig};
use crate::{
    modem::{
        bw500_waveform::Waveform,
        setup::{LinkSetup, Profile},
    },
    winlink::{
        Attachment, Message,
        host_session::{Connection, Progress, exchange_attached},
        mailbox::DirectoryMailbox,
        session::{Role, Settings},
    },
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read},
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU8, Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    process::{Child, Command},
    sync::{oneshot, watch},
    time::Instant,
};
use uuid::Uuid;

const PROFILE_LIMIT: u64 = 128 * 1024;
pub(super) const CLEANUP_GRACE: Duration = Duration::from_secs(5);
const MESSAGE_LIMIT: usize = 4096;
const READY_LIMIT: u64 = 256;
const OUTBOUND: &[u8] = b"\x00\xff\x01\r\nLinkwise caller\x00\xfe\x02";
const INBOUND: &[u8] = b"\xfe\x00\x03\n\rLinkwise responder\xff\x00\x04";
const CALLER_CONNECTED: u8 = 1;
const LISTENER_CONNECTED: u8 = 2;
const BOTH_CONNECTED: u8 = CALLER_CONNECTED | LISTENER_CONNECTED;
const LISTENING: u8 = 4;
const CLEANING_UP: u8 = 8;

pub(super) struct VirtualBench {
    modem_path: PathBuf,
    local_call: String,
    profile: Vec<u8>,
    profile_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AttachmentProof {
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VirtualProof {
    pub caller_to_listener: AttachmentProof,
    pub listener_to_caller: AttachmentProof,
    pub waveform_profile_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VirtualPhase {
    Attaching,
    WaitingForPeer,
    Connected,
    CleaningUp,
}

#[derive(Default)]
pub(super) struct VirtualProgress(AtomicU8);

impl VirtualProgress {
    fn listening(&self) {
        self.0.fetch_or(LISTENING, Ordering::Relaxed);
    }

    fn cleaning_up(&self) {
        self.0.fetch_or(CLEANING_UP, Ordering::Relaxed);
    }

    pub(super) fn was_connected(&self) -> bool {
        self.0.load(Ordering::Relaxed) & BOTH_CONNECTED == BOTH_CONNECTED
    }

    fn connected(&self, role: Role) {
        self.0.fetch_or(
            match role {
                Role::Caller => CALLER_CONNECTED,
                Role::Listener => LISTENER_CONNECTED,
            },
            Ordering::Relaxed,
        );
    }

    pub(super) fn phase(&self) -> VirtualPhase {
        let flags = self.0.load(Ordering::Relaxed);
        if flags & CLEANING_UP != 0 {
            VirtualPhase::CleaningUp
        } else if flags & BOTH_CONNECTED == BOTH_CONNECTED {
            VirtualPhase::Connected
        } else if flags & LISTENING != 0 {
            VirtualPhase::WaitingForPeer
        } else {
            VirtualPhase::Attaching
        }
    }
}

pub(super) struct Report {
    pub outcome: Outcome,
    pub modem_connected: bool,
    pub proof: Option<VirtualProof>,
    pub cleanup_confirmed: bool,
    pub diagnostic: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Readiness {
    control_port: u16,
    data_port: u16,
    audio_port: u16,
}

struct Endpoint {
    host: Connection,
    audio: TcpStream,
}

struct MailFixture {
    directory: tempfile::TempDir,
    caller_box: PathBuf,
    listener_box: PathBuf,
    caller_mid: String,
    listener_mid: String,
    outbound: Vec<u8>,
    inbound: Vec<u8>,
}

impl MailFixture {
    fn open(
        id: Uuid,
        caller_call: &str,
        listener_call: &str,
    ) -> io::Result<(Self, DirectoryMailbox, DirectoryMailbox)> {
        let directory = tempfile::tempdir()?;
        let caller_box = directory.path().join("caller");
        let listener_box = directory.path().join("listener");
        let caller = DirectoryMailbox::open(&caller_box, MESSAGE_LIMIT)?;
        let listener = DirectoryMailbox::open(&listener_box, MESSAGE_LIMIT)?;
        let caller_mid = fixture_mid(id, 'A');
        let listener_mid = fixture_mid(id, 'B');
        let outbound = enqueue(&caller, &caller_mid, caller_call, listener_call, OUTBOUND)?;
        let inbound = enqueue(
            &listener,
            &listener_mid,
            listener_call,
            caller_call,
            INBOUND,
        )?;
        Ok((
            Self {
                directory,
                caller_box,
                listener_box,
                caller_mid,
                listener_mid,
                outbound,
                inbound,
            },
            caller,
            listener,
        ))
    }

    fn verify(&self, profile_sha256: &str) -> io::Result<VirtualProof> {
        let caller_received = DirectoryMailbox::open(&self.caller_box, MESSAGE_LIMIT)?
            .incoming(&self.listener_mid)?;
        let listener_received = DirectoryMailbox::open(&self.listener_box, MESSAGE_LIMIT)?
            .incoming(&self.caller_mid)?;
        if caller_received != self.inbound || listener_received != self.outbound {
            return Err(io::Error::other("managed B2F envelope comparison failed"));
        }
        let received = |envelope: &[u8]| -> io::Result<AttachmentProof> {
            let message = Message::decode(envelope, MESSAGE_LIMIT)?;
            let [attachment] = message.attachments.as_slice() else {
                return Err(io::Error::other("managed B2F attachment is missing"));
            };
            Ok(AttachmentProof {
                bytes: attachment.bytes.len(),
                sha256: digest(attachment.bytes),
            })
        };
        Ok(VirtualProof {
            caller_to_listener: received(&listener_received)?,
            listener_to_caller: received(&caller_received)?,
            waveform_profile_sha256: profile_sha256.into(),
        })
    }
}

impl VirtualBench {
    pub(super) fn open(config: &VirtualBenchConfig) -> Result<Self> {
        if !valid_call(&config.local_call) {
            return Err(Error::Invalid("invalid managed BW500 local callsign"));
        }
        let modem_path = config.modem_path.canonicalize()?;
        if !modem_path.is_file() || !fs::metadata(&config.waveform_profile)?.is_file() {
            return Err(Error::Invalid(
                "expected a modem executable and a waveform profile file",
            ));
        }
        let mut profile = Vec::new();
        File::open(&config.waveform_profile)?
            .take(PROFILE_LIMIT + 1)
            .read_to_end(&mut profile)?;
        if profile.len() as u64 > PROFILE_LIMIT {
            return Err(Error::Invalid("BW500 waveform profile exceeds 128 KiB"));
        }
        let json = std::str::from_utf8(&profile)
            .map_err(|_| Error::Invalid("BW500 waveform profile must be UTF-8"))?;
        Waveform::from_json(json).map_err(|_| Error::Invalid("invalid BW500 waveform profile"))?;
        let profile_sha256 = digest(&profile);
        Ok(Self {
            modem_path,
            local_call: config.local_call.clone(),
            profile,
            profile_sha256,
        })
    }

    pub(super) fn supports_peer(&self, peer: &str) -> bool {
        peer != self.local_call && valid_call(peer)
    }

    pub(super) fn profile_sha256(&self) -> &str {
        &self.profile_sha256
    }

    pub(super) async fn run(
        &self,
        id: Uuid,
        peer: &str,
        deadline: Instant,
        mut cancel: watch::Receiver<bool>,
        progress: &VirtualProgress,
    ) -> Report {
        let mut children = Vec::with_capacity(2);
        let completed = self
            .run_pair(id, peer, deadline, &mut cancel, &mut children, progress)
            .await;
        progress.cleaning_up();
        // Error/stop paths do not leave a process available to a subsequent admission.
        for child in &mut children {
            let _ = child.start_kill();
        }
        let reaped = tokio::time::timeout(CLEANUP_GRACE, async {
            let mut all_reaped = true;
            for child in &mut children {
                all_reaped &= child.wait().await.is_ok();
            }
            all_reaped
        })
        .await;
        let reap_failed = !matches!(reaped, Ok(true));
        let modem_connected = progress.was_connected();
        match completed {
            Ok(proof) => Report {
                outcome: if reap_failed {
                    Outcome::Failed
                } else {
                    Outcome::Completed
                },
                modem_connected,
                proof: Some(proof),
                cleanup_confirmed: !reap_failed,
                diagnostic: reap_failed.then(|| "managed modem process could not be reaped".into()),
            },
            Err(failure) => Report {
                outcome: failure.outcome,
                modem_connected,
                proof: None,
                cleanup_confirmed: !reap_failed,
                diagnostic: if reap_failed {
                    Some("managed modem process could not be reaped".into())
                } else {
                    failure.diagnostic
                },
            },
        }
    }

    async fn run_pair(
        &self,
        id: Uuid,
        peer: &str,
        deadline: Instant,
        cancel: &mut watch::Receiver<bool>,
        children: &mut Vec<Child>,
        progress: &VirtualProgress,
    ) -> std::result::Result<VirtualProof, Failure> {
        let stage = async {
            let (fixture, caller, listener) = MailFixture::open(id, &self.local_call, peer)?;
            let profile = fixture.directory.path().join("waveform.json");
            fs::write(&profile, &self.profile)?;
            let a = self
                .start_endpoint(&self.local_call, &profile, children)
                .await?;
            let b = self.start_endpoint(peer, &profile, children).await?;
            Ok::<_, io::Error>((fixture, caller, listener, a, b))
        };
        let (fixture, caller, listener, a, b) = tokio::select! {
            biased;
            _ = wait_cancel(cancel) => return Err(Failure::aborted()),
            _ = tokio::time::sleep_until(deadline) => return Err(Failure::timed_out()),
            result = stage => result?,
        };
        let Endpoint {
            host: caller_connection,
            audio: mut caller_audio,
        } = a;
        let Endpoint {
            host: listener_connection,
            audio: mut listener_audio,
        } = b;
        let relay = tokio::spawn(async move {
            tokio::io::copy_bidirectional(&mut caller_audio, &mut listener_audio).await
        });
        let (stop, stopped) = watch::channel(false);
        let mut caller_stop = stopped.clone();
        let (listening, ready) = oneshot::channel();
        let mut listening = Some(listening);
        let sessions = async {
            let listener = async {
                let result = exchange_attached(
                    listener_connection,
                    Role::Listener,
                    settings(peer, &self.local_call),
                    listener,
                    stopped,
                    |event| match event {
                        Progress::Listening => {
                            progress.listening();
                            if let Some(listening) = listening.take() {
                                let _ = listening.send(());
                            }
                        }
                        Progress::ModemConnected => progress.connected(Role::Listener),
                        Progress::Completed => {}
                    },
                )
                .await;
                if result.is_err() {
                    let _ = stop.send(true);
                }
                result
            };
            let caller = async {
                tokio::select! {
                    biased;
                    _ = caller_stop.wait_for(|stop| *stop) => return Err(io::Error::new(io::ErrorKind::Interrupted, "managed listener stopped")),
                    ready = ready => ready.map_err(|_| io::Error::other("managed listener did not arm"))?,
                }
                let result = exchange_attached(
                    caller_connection,
                    Role::Caller,
                    settings(&self.local_call, peer),
                    caller,
                    caller_stop,
                    |event| {
                        if matches!(event, Progress::ModemConnected) {
                            progress.connected(Role::Caller);
                        }
                    },
                )
                .await;
                if result.is_err() {
                    let _ = stop.send(true);
                }
                result
            };
            let (listener, caller) = tokio::join!(listener, caller);
            listener.and(caller).map(drop)
        };
        tokio::pin!(sessions);
        let completion = tokio::select! {
            biased;
            _ = wait_cancel(cancel) => Err(Failure::aborted()),
            _ = tokio::time::sleep_until(deadline) => Err(Failure::timed_out()),
            result = &mut sessions => {
                relay.abort();
                let _ = relay.await;
                result?;
                return Ok(fixture.verify(&self.profile_sha256)?);
            }
        };
        // Signal cancellation to both client transports and wait for their workers
        // before the private directory can be removed or station lease released.
        progress.cleaning_up();
        let _ = stop.send(true);
        relay.abort();
        let _ = relay.await;
        let _ = sessions.await;
        completion
    }

    async fn start_endpoint(
        &self,
        call: &str,
        profile: &Path,
        children: &mut Vec<Child>,
    ) -> io::Result<Endpoint> {
        let token = format!("{}{}\n", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        children.push(
            Command::new(&self.modem_path)
                .args(["--serve-managed-virtual-bw500", call])
                .arg(profile)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()?,
        );
        let child = children.last_mut().expect("managed child started");
        let mut input = child.stdin.take().expect("piped managed stdin");
        input.write_all(token.as_bytes()).await?;
        input.shutdown().await?;
        drop(input);
        let mut line = Vec::new();
        BufReader::new(
            child
                .stdout
                .take()
                .expect("piped managed stdout")
                .take(READY_LIMIT),
        )
        .read_until(b'\n', &mut line)
        .await?;
        if line.last() != Some(&b'\n') {
            return Err(io::Error::other(
                "managed modem readiness record unavailable",
            ));
        }
        let ready: Readiness = serde_json::from_slice(&line)
            .map_err(|_| io::Error::other("invalid managed modem readiness record"))?;
        let ports = [ready.control_port, ready.data_port, ready.audio_port];
        if ports.contains(&0)
            || ports[0] == ports[1]
            || ports[0] == ports[2]
            || ports[1] == ports[2]
        {
            return Err(io::Error::other("invalid managed modem endpoint ports"));
        }
        let (control, data, audio) = tokio::try_join!(
            attach(ports[0], &token),
            attach(ports[1], &token),
            attach(ports[2], &token)
        )?;
        Ok(Endpoint {
            host: Connection { control, data },
            audio,
        })
    }
}

async fn wait_cancel(cancel: &mut watch::Receiver<bool>) {
    let _ = cancel.wait_for(|stop| *stop).await;
}

async fn attach(port: u16, token: &str) -> io::Result<TcpStream> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await?;
    stream.set_nodelay(true)?;
    stream.write_all(token.as_bytes()).await?;
    Ok(stream)
}

struct Failure {
    outcome: Outcome,
    diagnostic: Option<String>,
}

impl Failure {
    fn aborted() -> Self {
        Self {
            outcome: Outcome::Aborted,
            diagnostic: None,
        }
    }
    fn timed_out() -> Self {
        Self {
            outcome: Outcome::TimedOut,
            diagnostic: None,
        }
    }
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Self {
            outcome: Outcome::Failed,
            diagnostic: Some(error.to_string()),
        }
    }
}

fn valid_call(call: &str) -> bool {
    let setup = LinkSetup {
        caller: call.into(),
        high_speed: false,
        requested_level: 4,
    };
    setup
        .encode(Profile::Bw500)
        .and_then(|frame| LinkSetup::decode(Profile::Bw500, &frame))
        .is_some_and(|decoded| decoded.caller == call)
}

fn fixture_mid(id: Uuid, side: char) -> String {
    format!("{}{}", &id.simple().to_string()[..10], side)
}

fn settings<'a>(local_call: &'a str, peer_call: &'a str) -> Settings<'a> {
    Settings {
        local_call,
        peer_call,
        max_message_bytes: MESSAGE_LIMIT,
        max_compressed_bytes: MESSAGE_LIMIT,
    }
}

fn enqueue(
    box_: &DirectoryMailbox,
    id: &str,
    from: &str,
    to: &str,
    bytes: &[u8],
) -> io::Result<Vec<u8>> {
    let date = Utc::now().format("%Y/%m/%d %H:%M").to_string();
    let wire = Message {
        id,
        date: &date,
        kind: "Private",
        from,
        to: vec![to],
        cc: vec![],
        subject: "Linkwise managed BW500 fixture",
        mbo: None,
        body: "Isolated B2F waveform exchange.\r\n",
        attachments: vec![Attachment {
            name: "probe.bin",
            bytes,
        }],
    }
    .encode(MESSAGE_LIMIT)?;
    box_.enqueue(&wire)?;
    Ok(wire)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
