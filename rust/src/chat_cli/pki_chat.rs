//! Signed VarAC DATA records for two PKI-enabled Linkwise clients.
use std::{collections::VecDeque, io, path::Path};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

use crate::pki::{self, CertificateOffer, ChatTls, InspectedOffer, Pki, Signed};

use super::{
    codec,
    terminal::{MessageProtection, format_message},
};

const TAG: &[u8] = b"~LWP1~";
const HELLO: &str = "linkwise/chat/hello/1";
const CHALLENGE: &str = "linkwise/chat/challenge/1";
const PROOF: &str = "linkwise/chat/proof/1";
const UPGRADE_READY: &str = "linkwise/chat/upgrade-ready/1";
pub(crate) const MESSAGE: &str = "linkwise/chat/message/1";

#[derive(Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "type",
    content = "value",
    rename_all = "snake_case"
)]
enum Record {
    Hello(Signed),
    Challenge(Signed),
    Proof(Signed),
    Text(Signed),
    Tls(String),
    UpgradeReady(Signed),
    EnrollmentHello([u8; 32]),
    CertificateOffer(CertificateOffer),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    nonce: [u8; 32],
    encrypted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transcript {
    caller: [u8; 32],
    responder: [u8; 32],
    encrypted: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Message {
    transcript: Transcript,
    sequence: u64,
    text: String,
}

enum State {
    Idle,
    AwaitingHello,
    AwaitingChallenge([u8; 32]),
    AwaitingProof(Transcript),
    Verified {
        transcript: Transcript,
        sent: u64,
        received: u64,
    },
}

#[derive(Clone, Copy)]
enum Upgrade {
    Idle,
    Requested,
    Offered,
    Confirming,
}

struct Enrollment {
    challenge: [u8; 32],
    peer_challenge: Option<[u8; 32]>,
    offer: Option<CertificateOffer>,
    inspected: Option<InspectedOffer>,
    offers_received: u8,
}

pub(super) struct Session {
    pki: Pki,
    force_encryption: bool,
    encrypted: bool,
    caller: bool,
    peer: String,
    state: State,
    upgrade: Upgrade,
    enrolling: bool,
    enrollment: Option<Enrollment>,
    outbox: VecDeque<Vec<u8>>,
    tls: Option<ChatTls>,
    protected_records: codec::Decoder,
    last_proof: Option<Signed>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) enum Failure {
    Verification(io::Error),
    Encryption(io::Error),
    Input(io::Error),
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Self::Verification(error)
    }
}

impl From<serde_json::Error> for Failure {
    fn from(error: serde_json::Error) -> Self {
        Self::Verification(error.into())
    }
}

impl Failure {
    pub fn format(&self, callsign: &str) -> String {
        let (status, label, error) = match self {
            Self::Verification(error) => (
                MessageProtection::VerificationFailed,
                "Verification failed",
                error,
            ),
            Self::Encryption(error) => (
                MessageProtection::EncryptionFailed,
                "Encryption failed",
                error,
            ),
            Self::Input(error) => return error.to_string(),
        };
        format_message(callsign, status, &format!("{label}: {error}"))
    }
}

impl Session {
    pub fn new(pki: Pki, encrypted: bool, enrolling: bool) -> Self {
        Self {
            pki,
            force_encryption: encrypted,
            encrypted,
            caller: false,
            peer: String::new(),
            state: State::Idle,
            upgrade: Upgrade::Idle,
            enrolling,
            enrollment: None,
            outbox: VecDeque::new(),
            tls: None,
            protected_records: codec::Decoder::default(),
            last_proof: None,
        }
    }

    pub fn reset(&mut self) {
        self.peer.clear();
        self.state = State::Idle;
        self.upgrade = Upgrade::Idle;
        self.enrollment = None;
        self.outbox.clear();
        self.tls = None;
        self.protected_records = codec::Decoder::default();
    }

    pub fn start(&mut self, peer: &str, caller: bool) -> Result<(), Failure> {
        self.reset();
        self.caller = caller;
        self.pki
            .refresh_revocation((!self.enrolling).then_some(peer))?;
        if self.enrolling {
            self.peer = peer.into();
            let challenge = pki::nonce()?;
            self.enrollment = Some(Enrollment {
                challenge,
                peer_challenge: None,
                offer: None,
                inspected: None,
                offers_received: 0,
            });
            if caller {
                self.queue(Record::EnrollmentHello(challenge))?;
            }
            return Ok(());
        }
        if !self.pki.has_peer(peer) {
            return Err(invalid("connected peer is absent from shared PKI configuration").into());
        }
        self.encrypted = self.force_encryption || self.pki.require_encryption(peer)?;
        if self.encrypted {
            self.check_chat_encryption()?;
        }
        self.peer = peer.into();
        if caller {
            let nonce = pki::nonce()?;
            let signed = self.pki.sign(
                HELLO,
                peer,
                &serde_json::to_vec(&Hello {
                    nonce,
                    encrypted: self.encrypted,
                })?,
            )?;
            self.queue(Record::Hello(signed))?;
            self.state = State::AwaitingChallenge(nonce);
        } else {
            self.state = State::AwaitingHello;
        }
        Ok(())
    }

    pub fn status(&self) -> &str {
        if self.enrolling {
            return "certificate enrollment";
        }
        if self.upgrading() {
            return "encryption upgrade pending";
        }
        match self.state {
            State::Idle => "PKI ready",
            State::Verified { .. } if self.encrypted => "verified, encrypted",
            State::Verified { .. } => "verified, signed",
            _ => "verifying peer",
        }
    }

    pub fn next(&mut self) -> Result<Option<Vec<u8>>, Failure> {
        if let Some(record) = self.outbox.pop_front() {
            return Ok(Some(record));
        }
        let Some(channel) = &mut self.tls else {
            return Ok(None);
        };
        let bytes = channel.next().map_err(Failure::Encryption)?;
        bytes
            .map(|chunk| Self::encode_wire(Record::Tls(STANDARD.encode(chunk))))
            .transpose()
            .map_err(Failure::Encryption)
    }

    pub fn verified(&self) -> bool {
        matches!(self.state, State::Verified { .. })
    }

    pub fn refresh_revocation(&mut self) -> Result<(), Failure> {
        self.pki
            .refresh_revocation((!self.enrolling).then_some(self.peer.as_str()))?;
        Ok(())
    }

    pub fn upgrading(&self) -> bool {
        !matches!(self.upgrade, Upgrade::Idle)
    }

    pub fn enrollment_ready(&self) -> bool {
        self.enrollment
            .as_ref()
            .is_some_and(|state| state.peer_challenge.is_some())
    }

    pub fn offer_certificate(&mut self, include_encryption: bool) -> io::Result<()> {
        let challenge = self
            .enrollment
            .as_ref()
            .ok_or_else(|| invalid("start both clients with --pki FILE --enroll"))?
            .peer_challenge
            .ok_or_else(|| invalid("wait for the peer's enrollment challenge"))?;
        let offer = self
            .pki
            .certificate_offer(&self.peer, &challenge, include_encryption)?;
        self.queue(Record::CertificateOffer(offer))
    }

    pub fn inspect_certificate(&mut self) -> io::Result<Vec<String>> {
        let state = self
            .enrollment
            .as_mut()
            .ok_or_else(|| invalid("start both clients with --pki FILE --enroll"))?;
        let offer = state
            .offer
            .as_ref()
            .ok_or_else(|| invalid("no certificate offer received"))?;
        let inspected = self
            .pki
            .inspect_certificate_offer(&self.peer, &state.challenge, offer)?;
        let details = inspected.details.clone();
        state.inspected = Some(inspected);
        Ok(details)
    }

    pub fn accept_certificate(&mut self, replace: bool, fingerprints: &[&str]) -> io::Result<()> {
        let state = self
            .enrollment
            .as_mut()
            .ok_or_else(|| invalid("start both clients with --pki FILE --enroll"))?;
        let (Some(offer), Some(inspected)) = (&state.offer, &state.inspected) else {
            return Err(invalid(
                "use /cert inspect and compare fingerprints before accepting",
            ));
        };
        self.pki.accept_certificate_offer(
            &self.peer,
            &state.challenge,
            offer,
            inspected,
            replace,
            fingerprints,
        )?;
        state.offer = None;
        state.inspected = None;
        Ok(())
    }

    fn receive_enrollment(&mut self, record: Record) -> Result<Option<String>, Failure> {
        let state = self
            .enrollment
            .as_mut()
            .ok_or_else(|| invalid("no active enrollment call"))?;
        match record {
            Record::EnrollmentHello(challenge) if state.peer_challenge.is_none() => {
                state.peer_challenge = Some(challenge);
                let reply = state.challenge;
                if !self.caller {
                    self.queue(Record::EnrollmentHello(reply))?;
                }
                Ok(Some(
                    "Enrollment ready. Use /cert offer to send your public certificate.".into(),
                ))
            }
            Record::CertificateOffer(offer) if state.peer_challenge.is_some() => {
                if state.offers_received == 4 {
                    return Err(invalid("too many certificate offers on this call").into());
                }
                state.offers_received += 1;
                self.pki
                    .verify_certificate_offer(&self.peer, &state.challenge, &offer)?;
                state.offer = Some(offer);
                state.inspected = None;
                Ok(Some(format!(
                    "Certificate offer from {}. Use /cert inspect.",
                    self.peer
                )))
            }
            _ => Err(invalid("unexpected record on a certificate enrollment call").into()),
        }
    }

    pub fn request_encryption(&mut self) -> Result<&'static str, Failure> {
        let State::Verified { transcript, .. } = self.state else {
            return Err(Failure::Input(invalid("wait for peer verification")));
        };
        if self.encrypted {
            return Err(Failure::Input(invalid("this call is already encrypted")));
        }
        if matches!(self.upgrade, Upgrade::Requested | Upgrade::Confirming) {
            return Err(Failure::Input(invalid(
                "encryption upgrade is already pending",
            )));
        }
        self.check_chat_encryption()?;
        let ready = self
            .pki
            .sign(UPGRADE_READY, &self.peer, &serde_json::to_vec(&transcript)?)?;
        self.queue(Record::UpgradeReady(ready))?;
        if matches!(self.upgrade, Upgrade::Offered) {
            self.confirm_encryption(transcript)?;
            Ok("Encryption accepted. Confirming with peer.")
        } else {
            self.upgrade = Upgrade::Requested;
            Ok("Encryption requested. Waiting for the peer to enter /encrypt.")
        }
    }

    fn check_chat_encryption(&self) -> Result<(), Failure> {
        if !self.pki.chat_encryption_allowed() {
            return Err(Failure::Encryption(invalid(
                "encryption is disabled for this transport",
            )));
        }
        Ok(())
    }

    fn confirm_encryption(&mut self, transcript: Transcript) -> Result<(), Failure> {
        self.tls = Some(
            ChatTls::new(
                &self.pki,
                &self.peer,
                self.caller,
                serde_json::to_vec(&transcript)?,
            )
            .map_err(Failure::Encryption)?,
        );
        self.upgrade = Upgrade::Confirming;
        Ok(())
    }

    fn receive_tls(&mut self, encoded: &str) -> Result<Vec<String>, Failure> {
        let channel = self.tls.as_mut().ok_or_else(|| {
            Failure::Encryption(invalid("unexpected TLS data before agreeing to encryption"))
        })?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|error| Failure::Encryption(io::Error::other(error)))?;
        let plaintext = channel.receive(&bytes).map_err(Failure::Encryption)?;
        let mut messages = Vec::new();
        if matches!(self.upgrade, Upgrade::Confirming) && channel.ready() {
            let State::Verified { transcript, .. } = &mut self.state else {
                return Err(invalid("TLS established before verified chat").into());
            };
            transcript.encrypted = true;
            self.encrypted = true;
            self.upgrade = Upgrade::Idle;
            messages.push(format!(
                "Encryption enabled. TLS 1.3 with {}.",
                channel.key_exchange()
            ));
        }
        let mut records = Vec::new();
        self.protected_records.push(&plaintext, |record| {
            records.push(serde_json::from_slice(record))
        })?;
        for record in records {
            if let Some(message) = self.verify_message(record?)? {
                messages.push(message);
            }
        }
        Ok(messages)
    }

    pub fn format_message(&self, callsign: &str, text: &str) -> String {
        let protection = if self.encrypted {
            MessageProtection::Encrypted
        } else {
            MessageProtection::Signed
        };
        format_message(callsign, protection, text)
    }

    fn encode_wire(record: Record) -> io::Result<Vec<u8>> {
        let mut body = TAG.to_vec();
        body.extend_from_slice(&serde_json::to_vec(&record)?);
        let wire = codec::encode_record(&body)?;
        if wire.len() > 4096 {
            return Err(invalid("PKI chat record exceeds the modem limit"));
        }
        Ok(wire)
    }

    fn queue(&mut self, record: Record) -> io::Result<()> {
        if self.outbox.len() == 2 {
            return Err(invalid("PKI chat queue exceeds the modem limit"));
        }
        self.outbox.push_back(Self::encode_wire(record)?);
        Ok(())
    }

    pub fn send_text(&mut self, text: &str) -> Result<(), Failure> {
        codec::encode(text).map_err(Failure::Input)?;
        if self.enrolling {
            return Err(Failure::Input(invalid(
                "This call is for certificate enrollment. Use /cert.",
            )));
        }
        if self.upgrading() {
            return Err(Failure::Input(invalid(
                "Text kept as draft. Wait for the encryption upgrade.",
            )));
        }
        let State::Verified {
            transcript, sent, ..
        } = self.state
        else {
            return Err(Failure::Input(invalid("wait for peer verification")));
        };
        let sequence = sent
            .checked_add(1)
            .ok_or_else(|| invalid("signed sequence exhausted"))?;
        let payload = serde_json::to_vec(&Message {
            transcript,
            sequence,
            text: text.into(),
        })?;
        let message = self.pki.sign(MESSAGE, &self.peer, &payload)?;
        if let Some(channel) = &mut self.tls {
            let frame = codec::encode_record(&serde_json::to_vec(&message)?)?;
            channel.send(&frame).map_err(Failure::Input)?;
        } else {
            self.queue(Record::Text(message)).map_err(Failure::Input)?;
        }
        if let State::Verified { sent, .. } = &mut self.state {
            *sent = sequence;
        }
        Ok(())
    }

    fn verify<T: serde::de::DeserializeOwned>(
        &self,
        domain: &str,
        signed: &Signed,
    ) -> io::Result<T> {
        if self.peer != signed.from {
            return Err(invalid("signed record is from the wrong station"));
        }
        let payload = self.pki.verify(domain, signed, 8192)?;
        Ok(serde_json::from_slice(&payload)?)
    }

    pub fn export_proof(&self, peer: &str, path: &Path) -> io::Result<()> {
        use std::io::Write;
        let proof = self
            .last_proof
            .as_ref()
            .filter(|proof| proof.from == peer)
            .ok_or_else(|| invalid("no verified message to export for this peer"))?;
        let mut output = tempfile::NamedTempFile::new_in(
            path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?;
        serde_json::to_writer(&mut output, proof)?;
        output.write_all(b"\n")?;
        output.as_file().sync_all()?;
        output
            .persist_noclobber(path)
            .map_err(|error| error.error)?;
        Ok(())
    }

    fn verify_message(&mut self, signed: Signed) -> io::Result<Option<String>> {
        let message: Message = self.verify(MESSAGE, &signed)?;
        let State::Verified {
            transcript,
            received,
            ..
        } = &mut self.state
        else {
            return Err(invalid("received a message before peer verification"));
        };
        if message.transcript != *transcript || received.checked_add(1) != Some(message.sequence) {
            return Err(invalid("replayed or out-of-order signed message"));
        }
        codec::encode(&message.text)?;
        *received = message.sequence;
        self.last_proof = Some(signed);
        Ok(Some(self.format_message(&self.peer, &message.text)))
    }

    pub fn receive(&mut self, wire: &[u8]) -> Result<Vec<String>, Failure> {
        let body = wire
            .strip_prefix(TAG)
            .ok_or_else(|| invalid("peer sent an unsigned record in a PKI session"))?;
        if body.len() > 4096 {
            return Err(invalid("oversized PKI chat record").into());
        }
        let record = serde_json::from_slice(body)?;
        match record {
            record if self.enrolling => self
                .receive_enrollment(record)
                .map(|message| message.into_iter().collect()),
            Record::Tls(encoded) => self.receive_tls(&encoded),
            record => self
                .receive_signed(record)
                .map(|message| message.into_iter().collect()),
        }
    }

    fn receive_signed(&mut self, record: Record) -> Result<Option<String>, Failure> {
        match (record, &self.state) {
            (Record::Hello(signed), State::AwaitingHello) => {
                let hello: Hello = self.verify(HELLO, &signed)?;
                if hello.encrypted != self.encrypted {
                    return Err(Failure::Encryption(invalid(
                        "peer requested a different chat protection mode",
                    )));
                }
                let transcript = Transcript {
                    caller: hello.nonce,
                    responder: pki::nonce()?,
                    encrypted: self.encrypted,
                };
                let challenge =
                    self.pki
                        .sign(CHALLENGE, &self.peer, &serde_json::to_vec(&transcript)?)?;
                self.queue(Record::Challenge(challenge))?;
                self.state = State::AwaitingProof(transcript);
                Ok(None)
            }
            (Record::Challenge(signed), State::AwaitingChallenge(nonce)) => {
                let transcript: Transcript = self.verify(CHALLENGE, &signed)?;
                if transcript.caller != *nonce {
                    return Err(invalid("peer challenge does not bind this chat").into());
                }
                if transcript.encrypted != self.encrypted {
                    return Err(Failure::Encryption(invalid(
                        "peer requested a different chat protection mode",
                    )));
                }
                let proof = self
                    .pki
                    .sign(PROOF, &self.peer, &serde_json::to_vec(&transcript)?)?;
                self.queue(Record::Proof(proof))?;
                self.state = State::Verified {
                    transcript,
                    sent: 0,
                    received: 0,
                };
                if self.encrypted {
                    self.confirm_encryption(transcript)?;
                }
                Ok(Some(format!("Verified peer: {}", self.peer)))
            }
            (Record::Proof(signed), State::AwaitingProof(expected)) => {
                let transcript: Transcript = self.verify(PROOF, &signed)?;
                if transcript != *expected {
                    return Err(invalid("peer proof does not bind this chat").into());
                }
                self.state = State::Verified {
                    transcript,
                    sent: 0,
                    received: 0,
                };
                if self.encrypted {
                    self.confirm_encryption(transcript)?;
                }
                Ok(Some(format!("Verified peer: {}", self.peer)))
            }
            (Record::UpgradeReady(signed), State::Verified { transcript, .. })
                if !self.encrypted =>
            {
                let offered: Transcript = self.verify(UPGRADE_READY, &signed)?;
                if offered != *transcript {
                    return Err(invalid("encryption request does not bind this chat").into());
                }
                match self.upgrade {
                    Upgrade::Idle => {
                        self.upgrade = Upgrade::Offered;
                        Ok(Some(
                            "Peer requested encryption. Enter /encrypt to accept.".into(),
                        ))
                    }
                    Upgrade::Requested => {
                        self.confirm_encryption(*transcript)?;
                        Ok(None)
                    }
                    _ => Err(invalid("repeated encryption request").into()),
                }
            }
            (Record::Text(signed), State::Verified { .. })
                if !self.encrypted
                    && matches!(self.upgrade, Upgrade::Idle | Upgrade::Requested) =>
            {
                Ok(self.verify_message(signed)?)
            }
            (Record::Text(_), State::Verified { .. }) => Err(Failure::Encryption(invalid(
                "received a message in the wrong protection mode",
            ))),
            _ => Err(invalid("unexpected PKI record").into()),
        }
    }
}
