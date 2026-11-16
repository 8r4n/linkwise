//! Shared station identity and trust policy for authenticated radio applications.
mod chat_tls;
mod enrollment;
pub(crate) use chat_tls::ChatTls;
pub(crate) use enrollment::{CertificateOffer, InspectedOffer};

use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
};

use aws_lc_rs::{
    digest::{SHA256, digest},
    rand::{SecureRandom, SystemRandom},
    signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use rustls::pki_types::{
    CertificateDer, CertificateRevocationListDer, PrivatePkcs8KeyDer, UnixTime, pem::PemObject,
};
use serde::{Deserialize, Serialize};
use webpki::{CertRevocationList, EndEntityCert, KeyUsage, RevocationOptionsBuilder};
use x509_cert::{
    Certificate,
    der::Decode,
    ext::pkix::{KeyUsage as CertificateKeyUsage, SubjectAltName, name::GeneralName},
};

const CONFIG_LIMIT: u64 = 64 * 1024;
const PEM_LIMIT: u64 = 1024 * 1024;
pub const MAIL_CONTEXT: &str = "linkwise/mail/1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    version: u8,
    callsign: String,
    signing: Credential,
    authorities: Vec<PathBuf>,
    #[serde(default)]
    peer_authorities: BTreeMap<String, String>,
    revocation_lists: Vec<PathBuf>,
    peers: BTreeMap<String, Peer>,
    #[serde(default)]
    allow_chat_encryption: bool,
    #[serde(default)]
    chat_key_exchange: ChatKeyExchange,
    #[serde(default)]
    encryption: Option<Encryption>,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ChatKeyExchange {
    #[default]
    P256,
    P256Mlkem768,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    certificate: PathBuf,
    private_key: PathBuf,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Peer {
    signing_certificate: CertificateSource,
    encryption_certificate: Option<CertificateSource>,
    #[serde(default)]
    require_encryption: bool,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
enum CertificateSource {
    File(PathBuf),
    Inline(InlineCertificate),
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct InlineCertificate {
    der: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Encryption {
    openssl: PathBuf,
    openssl_config: PathBuf,
    credential: Credential,
    #[serde(default)]
    allow_mail: bool,
    #[serde(default)]
    allow_chat: bool,
}

pub struct Pki {
    profile: Profile,
    profile_path: PathBuf,
    directory: PathBuf,
    anchors: Vec<Authority>,
    revocation: Vec<CertRevocationList<'static>>,
    signing: EcdsaKeyPair,
}

struct Authority {
    fingerprint: [u8; 32],
    anchor: rustls::pki_types::TrustAnchor<'static>,
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut result = [0; 32];
    result.copy_from_slice(digest(&SHA256, bytes).as_ref());
    result
}

fn ca_fingerprint(cert: &CertificateDer<'_>) -> io::Result<[u8; 32]> {
    use der::Encode;
    let parsed = Certificate::from_der(cert.as_ref()).map_err(other)?;
    Ok(sha256(
        &parsed
            .tbs_certificate
            .subject_public_key_info
            .to_der()
            .map_err(other)?,
    ))
}

fn display_fingerprint(hash: &[u8; 32]) -> String {
    hash.iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

pub(crate) fn print_fingerprint(path: &Path, ca_key: bool) -> io::Result<()> {
    use std::io::Write;
    aws_lc_rs::try_fips_mode().map_err(other)?;
    let certs = pem_objects::<CertificateDer<'static>>(path)?;
    let hash = if ca_key {
        ca_fingerprint(&certs[0])?
    } else {
        sha256(certs[0].as_ref())
    };
    writeln!(io::stdout().lock(), "{}", display_fingerprint(&hash))
}

pub(crate) fn parse_fingerprint(value: &str) -> io::Result<[u8; 32]> {
    let text = value.replace(':', "");
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(other(
            "enter a complete SHA-256 fingerprint (64 hex digits)",
        ));
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(other)?;
    }
    Ok(bytes)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signed {
    pub from: String,
    pub to: String,
    data: String,
    signature: String,
}

fn other(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

pub fn read_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("PKI input exceeds the configured limit"));
    }
    Ok(bytes)
}

fn pem_objects<T: PemObject>(path: &Path) -> io::Result<Vec<T>> {
    let pem = read_bounded(path, PEM_LIMIT)?;
    let entries = T::pem_reader_iter(BufReader::new(pem.as_slice()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(other)?;
    if entries.is_empty() {
        return Err(io::Error::other(format!(
            "no PEM objects in {}",
            path.display()
        )));
    }
    Ok(entries)
}

pub(crate) fn valid_call(call: &str) -> bool {
    !call.is_empty()
        && call.len() <= 12
        && call
            .bytes()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == b'-')
}

pub fn nonce() -> io::Result<[u8; 32]> {
    let mut value = [0; 32];
    SystemRandom::new().fill(&mut value).map_err(other)?;
    Ok(value)
}

fn signed_bytes(context: &str, from: &str, to: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = b"LINKWISE-PKI\0".to_vec();
    for field in [context.as_bytes(), from.as_bytes(), to.as_bytes(), payload] {
        out.extend_from_slice(&(field.len() as u64).to_be_bytes());
        out.extend_from_slice(field);
    }
    out
}

impl Pki {
    pub fn load(path: &Path) -> io::Result<Self> {
        aws_lc_rs::try_fips_mode().map_err(other)?;
        let profile_path = std::path::absolute(path)?;
        let profile: Profile = serde_json::from_slice(&read_bounded(&profile_path, CONFIG_LIMIT)?)?;
        if profile.version != 1
            || !valid_call(&profile.callsign)
            || profile.peers.contains_key(&profile.callsign)
            || profile.peers.keys().any(|call| !valid_call(call))
            || profile.authorities.is_empty()
            || profile.revocation_lists.is_empty()
        {
            return Err(io::Error::other(
                "invalid PKI version, station, peers, authorities, or CRLs",
            ));
        }
        let directory = path.parent().unwrap_or(Path::new(".")).to_owned();
        let mut anchors = Vec::new();
        for path in &profile.authorities {
            for ca in pem_objects::<CertificateDer<'static>>(&directory.join(path))? {
                let fingerprint = ca_fingerprint(&ca)?;
                anchors.push(Authority {
                    fingerprint,
                    anchor: webpki::anchor_from_trusted_cert(&ca)
                        .map_err(other)?
                        .to_owned(),
                });
            }
        }
        for (peer, pin) in &profile.peer_authorities {
            let pin = parse_fingerprint(pin)?;
            if !valid_call(peer)
                || peer == &profile.callsign
                || !anchors.iter().any(|ca| ca.fingerprint == pin)
            {
                return Err(other(format!("invalid or unavailable CA pin for {peer}")));
            }
        }
        let revocation = load_revocation(&directory, &profile.revocation_lists)?;
        let key_bytes = read_bounded(&directory.join(&profile.signing.private_key), PEM_LIMIT)?;
        let key = PrivatePkcs8KeyDer::from_pem_slice(&key_bytes).map_err(other)?;
        let signing =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, key.secret_pkcs8_der())
                .map_err(other)?;
        let pki = Self {
            profile,
            profile_path,
            directory,
            anchors,
            revocation,
            signing,
        };
        let local =
            pki.validated_certificate(&pki.profile.signing.certificate, pki.callsign(), true)?;
        let challenge = nonce()?;
        let proof = pki
            .signing
            .sign(&SystemRandom::new(), &challenge)
            .map_err(other)?;
        EndEntityCert::try_from(&local)
            .map_err(other)?
            .verify_signature(
                webpki::aws_lc_rs::ECDSA_P256_SHA256,
                &challenge,
                proof.as_ref(),
            )
            .map_err(|_| io::Error::other("station signing key does not match its certificate"))?;
        Ok(pki)
    }

    pub fn callsign(&self) -> &str {
        &self.profile.callsign
    }

    pub(crate) fn refresh_revocation(&mut self, peer: Option<&str>) -> io::Result<()> {
        self.revocation = load_revocation(&self.directory, &self.profile.revocation_lists)?;
        self.validated_certificate(&self.profile.signing.certificate, self.callsign(), true)?;
        if let Some(peer) = peer {
            self.peer_certificate(&self.peer(peer)?.signing_certificate, peer, true)?;
        }
        Ok(())
    }

    pub fn has_peer(&self, call: &str) -> bool {
        self.profile.peers.contains_key(call)
    }

    pub fn require_encryption(&self, call: &str) -> io::Result<bool> {
        Ok(self.peer(call)?.require_encryption)
    }

    fn peer(&self, call: &str) -> io::Result<&Peer> {
        self.profile
            .peers
            .get(call)
            .ok_or_else(|| io::Error::other(format!("unknown PKI peer {call}")))
    }

    fn validated_certificate(
        &self,
        path: &Path,
        call: &str,
        signing: bool,
    ) -> io::Result<CertificateDer<'static>> {
        let chain = pem_objects::<CertificateDer<'static>>(&self.directory.join(path))?;
        self.validate_chain(&chain, call, signing)
    }

    fn peer_certificate(
        &self,
        source: &CertificateSource,
        call: &str,
        signing: bool,
    ) -> io::Result<CertificateDer<'static>> {
        self.validate_chain(&self.read_chain(source)?, call, signing)
    }

    fn validate_chain(
        &self,
        chain: &[CertificateDer<'static>],
        call: &str,
        signing: bool,
    ) -> io::Result<CertificateDer<'static>> {
        let (leaf, intermediates) = chain
            .split_first()
            .ok_or_else(|| other("empty certificate chain"))?;
        let parsed = Certificate::from_der(leaf.as_ref()).map_err(other)?;
        let tbs = &parsed.tbs_certificate;
        let usage = tbs
            .get::<CertificateKeyUsage>()
            .map_err(other)?
            .map(|(_, usage)| usage);
        let expected_name = format!("{}.radio.linkwise.invalid", call.to_ascii_lowercase());
        let exact_name = tbs.get::<SubjectAltName>().map_err(other)?.is_some_and(|(_, names)| {
            names.0.iter().any(|name| matches!(name, GeneralName::DnsName(dns) if dns.as_str().eq_ignore_ascii_case(&expected_name)))
        });
        let algorithm = &tbs.subject_public_key_info.algorithm;
        if !exact_name
            || !usage.is_some_and(|usage| {
                if signing {
                    usage.digital_signature() && !usage.key_agreement()
                } else {
                    usage.key_agreement() && !usage.digital_signature()
                }
            })
            || algorithm.oid.to_string() != "1.2.840.10045.2.1"
            || algorithm
                .parameters
                .as_ref()
                .and_then(|value| value.decode_as::<der::asn1::ObjectIdentifier>().ok())
                != Some(der::asn1::ObjectIdentifier::new_unwrap(
                    "1.2.840.10045.3.1.7",
                ))
        {
            return Err(io::Error::other(format!(
                "certificate for {call} has the wrong callsign, key, or role"
            )));
        }
        let expected = if call == self.callsign() {
            None
        } else if let Some(pin) = self.profile.peer_authorities.get(call) {
            Some(parse_fingerprint(pin)?)
        } else {
            // Existing single-CA profiles have an unambiguous issuer.
            let first = self.anchors[0].fingerprint;
            if self.anchors.iter().any(|ca| ca.fingerprint != first) {
                return Err(other(format!(
                    "configure the CA key fingerprint for {call} in peer_authorities"
                )));
            }
            Some(first)
        };
        let anchors: Vec<_> = self
            .anchors
            .iter()
            .filter(|ca| expected.is_none_or(|pin| pin == ca.fingerprint))
            .map(|ca| ca.anchor.clone())
            .collect();
        let crls: Vec<_> = self.revocation.iter().collect();
        let revocation = RevocationOptionsBuilder::new(&crls)
            .map_err(|_| io::Error::other("no revocation lists configured"))?
            .with_status_policy(webpki::UnknownStatusPolicy::Deny)
            .with_expiration_policy(webpki::ExpirationPolicy::Enforce)
            .build();
        EndEntityCert::try_from(leaf)
            .map_err(other)?
            .verify_for_usage(
                &[webpki::aws_lc_rs::ECDSA_P256_SHA256],
                &anchors,
                intermediates,
                UnixTime::now(),
                KeyUsage::required_if_present(&[0x2b, 6, 1, 5, 5, 7, 3, 4]),
                Some(revocation),
                None,
            )
            .map_err(|error| {
                other(format!(
                    "certificate for {call} failed trust or revocation checks: {error}"
                ))
            })?;
        Ok(leaf.clone())
    }

    pub fn check(&self) -> io::Result<()> {
        if matches!(
            self.profile.chat_key_exchange,
            ChatKeyExchange::P256Mlkem768
        ) {
            chat_tls::check_pqc_provider()?;
        }
        for (call, peer) in &self.profile.peers {
            self.peer_certificate(&peer.signing_certificate, call, true)?;
            if peer.require_encryption
                && !self.chat_encryption_allowed()
                && (peer.encryption_certificate.is_none() || self.profile.encryption.is_none())
            {
                return Err(io::Error::other(format!(
                    "encryption is required for {call}, but its credentials are incomplete"
                )));
            }
            if let Some(path) = &peer.encryption_certificate {
                self.peer_certificate(path, call, false)?;
            }
        }
        if let Some(encryption) = &self.profile.encryption {
            let leaf = self.validated_certificate(
                &encryption.credential.certificate,
                self.callsign(),
                false,
            )?;
            let challenge = nonce()?;
            let ciphertext = self.encrypt_for(encryption, &leaf, &challenge)?;
            let plaintext = self.decrypt_with(encryption, &leaf, &ciphertext)?;
            if plaintext != challenge {
                return Err(io::Error::other(
                    "station encryption key does not match its certificate",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn chat_encryption_allowed(&self) -> bool {
        self.profile.allow_chat_encryption
            || self
                .profile
                .encryption
                .as_ref()
                .is_some_and(|config| config.allow_chat)
    }

    pub fn sign(&self, context: &str, to: &str, payload: &[u8]) -> io::Result<Signed> {
        let peer = self.peer(to)?;
        self.peer_certificate(&peer.signing_certificate, to, true)?;
        self.sign_to(context, to, payload)
    }

    fn sign_to(&self, context: &str, to: &str, payload: &[u8]) -> io::Result<Signed> {
        self.validated_certificate(&self.profile.signing.certificate, self.callsign(), true)?;
        let signature = self
            .signing
            .sign(
                &SystemRandom::new(),
                &signed_bytes(context, self.callsign(), to, payload),
            )
            .map_err(other)?;
        Ok(Signed {
            from: self.callsign().into(),
            to: to.into(),
            data: STANDARD.encode(payload),
            signature: STANDARD.encode(signature.as_ref()),
        })
    }

    pub fn verify(&self, context: &str, signed: &Signed, max: usize) -> io::Result<Vec<u8>> {
        let peer = self.peer(&signed.from)?;
        let certificate = self.peer_certificate(&peer.signing_certificate, &signed.from, true)?;
        self.verify_with_certificate(context, signed, max, &certificate)
    }

    fn verify_with_certificate(
        &self,
        context: &str,
        signed: &Signed,
        max: usize,
        certificate: &CertificateDer<'_>,
    ) -> io::Result<Vec<u8>> {
        if signed.to != self.callsign()
            || signed.data.len() > max.div_ceil(3) * 4
            || signed.signature.len() > 100
        {
            return Err(io::Error::other(
                "wrong signed recipient or oversized signed record",
            ));
        }
        let payload = STANDARD.decode(&signed.data).map_err(other)?;
        if payload.len() > max {
            return Err(io::Error::other("oversized signed payload"));
        }
        let signature = STANDARD.decode(&signed.signature).map_err(other)?;
        EndEntityCert::try_from(certificate)
            .map_err(other)?
            .verify_signature(
                webpki::aws_lc_rs::ECDSA_P256_SHA256,
                &signed_bytes(context, &signed.from, &signed.to, &payload),
                &signature,
            )
            .map_err(|_| io::Error::other("signature verification failed"))?;
        Ok(payload)
    }
}

fn load_revocation(
    directory: &Path,
    files: &[PathBuf],
) -> io::Result<Vec<CertRevocationList<'static>>> {
    let mut revocation = Vec::new();
    for path in files {
        for crl in pem_objects::<CertificateRevocationListDer<'static>>(&directory.join(path))? {
            let list = webpki::OwnedCertRevocationList::from_der(crl.as_ref()).map_err(other)?;
            revocation.push(CertRevocationList::Owned(list));
        }
    }
    Ok(revocation)
}

#[derive(x509_cert::der::Sequence)]
struct AuthEnvelope {
    version: u8,
    #[asn1(
        context_specific = "0",
        tag_mode = "IMPLICIT",
        constructed = "true",
        optional = "true"
    )]
    originator_info: Option<cms::enveloped_data::OriginatorInfo>,
    recipients: cms::enveloped_data::RecipientInfos,
    content: cms::enveloped_data::EncryptedContentInfo,
    #[asn1(
        context_specific = "1",
        tag_mode = "IMPLICIT",
        constructed = "true",
        optional = "true"
    )]
    attributes: Option<x509_cert::attr::Attributes>,
    mac: x509_cert::der::asn1::OctetString,
    #[asn1(
        context_specific = "2",
        tag_mode = "IMPLICIT",
        constructed = "true",
        optional = "true"
    )]
    unauthenticated: Option<x509_cert::attr::Attributes>,
}

#[derive(x509_cert::der::Sequence)]
struct GcmParameters {
    nonce: x509_cert::der::asn1::OctetString,
    #[asn1(default = "default_gcm_tag")]
    tag_len: u8,
}

fn default_gcm_tag() -> u8 {
    12
}

fn check_cms(input: &[u8]) -> io::Result<()> {
    use cms::{content_info::ContentInfo, enveloped_data::RecipientInfo};
    use x509_cert::spki::AlgorithmIdentifierOwned;
    let info = ContentInfo::from_der(input).map_err(other)?;
    if info.content_type.to_string() != "1.2.840.113549.1.9.16.1.23" {
        return Err(io::Error::other("expected CMS AuthEnvelopedData"));
    }
    let envelope: AuthEnvelope = info.content.decode_as().map_err(other)?;
    let encryption = &envelope.content.content_enc_alg;
    let [RecipientInfo::Kari(recipient)] = envelope.recipients.0.as_slice() else {
        return Err(io::Error::other("CMS requires one key-agreement recipient"));
    };
    let wrap: AlgorithmIdentifierOwned = recipient
        .key_enc_alg
        .parameters
        .as_ref()
        .ok_or_else(|| io::Error::other("missing CMS key wrapping algorithm"))?
        .decode_as()
        .map_err(other)?;
    let parameters: GcmParameters = encryption
        .parameters
        .as_ref()
        .ok_or_else(|| io::Error::other("missing AES-GCM parameters"))?
        .decode_as()
        .map_err(other)?;
    if envelope.version != 0
        || envelope.originator_info.is_some()
        || envelope.unauthenticated.is_some()
        || envelope.content.content_type.to_string() != "1.2.840.113549.1.7.1"
        || envelope.content.encrypted_content.is_none()
        || encryption.oid.to_string() != "2.16.840.1.101.3.4.1.46"
        || recipient.key_enc_alg.oid.to_string() != "1.3.132.1.11.1"
        || wrap.oid.to_string() != "2.16.840.1.101.3.4.1.45"
        || parameters.nonce.as_bytes().len() != 12
        || parameters.tag_len != 16
        || envelope.mac.as_bytes().len() != 16
        || recipient.recipient_enc_keys.len() != 1
    {
        return Err(io::Error::other(
            "CMS input does not match the Linkwise P-256/SHA-256/AES-256-GCM profile",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub enum Application {
    Mail,
}

impl Pki {
    fn cms_provider(&self, encryption: &Encryption) -> io::Result<std::process::Command> {
        if !encryption.openssl.is_absolute() {
            return Err(io::Error::other(
                "configure an absolute OpenSSL FIPS executable path",
            ));
        }
        let config = self.directory.join(&encryption.openssl_config);
        if !config.is_file() {
            return Err(io::Error::other("OpenSSL FIPS configuration is missing"));
        }
        let mut command = std::process::Command::new(&encryption.openssl);
        command
            .env("OPENSSL_CONF", config)
            .env_remove("OPENSSL_MODULES");
        command.args([
            "cms",
            "-binary",
            "-provider",
            "fips",
            "-provider",
            "base",
            "-propquery",
            "fips=yes",
        ]);
        Ok(command)
    }

    fn encrypt_policy(&self, application: Application) -> io::Result<&Encryption> {
        self.profile
            .encryption
            .as_ref()
            .filter(|encryption| match application {
                Application::Mail => encryption.allow_mail,
            })
            .ok_or_else(|| io::Error::other("encryption is disabled for this transport"))
    }

    pub fn encrypt(
        &self,
        application: Application,
        peer: &str,
        plaintext: &[u8],
    ) -> io::Result<Vec<u8>> {
        let encryption = self.encrypt_policy(application)?;
        let cert = self
            .peer(peer)?
            .encryption_certificate
            .as_ref()
            .ok_or_else(|| io::Error::other("peer has no encryption certificate"))?;
        let leaf = self.peer_certificate(cert, peer, false)?;
        self.encrypt_for(encryption, &leaf, plaintext)
    }

    fn encrypt_for(
        &self,
        encryption: &Encryption,
        leaf: &CertificateDer<'_>,
        plaintext: &[u8],
    ) -> io::Result<Vec<u8>> {
        let mut command = self.cms_provider(encryption)?;
        let mut recipient = tempfile::NamedTempFile::new()?;
        write_pem(&mut recipient, leaf.as_ref())?;
        command
            .args([
                "-encrypt",
                "-aes-256-gcm",
                "-aes256-wrap",
                "-outform",
                "DER",
                "-recip",
            ])
            .arg(recipient.path())
            .args(["-keyopt", "ecdh_kdf_md:sha256"]);
        let result = run_cms(command, plaintext)?;
        check_cms(&result)?;
        Ok(result)
    }

    pub fn decrypt(&self, application: Application, ciphertext: &[u8]) -> io::Result<Vec<u8>> {
        let encryption = self.encrypt_policy(application)?;
        let leaf =
            self.validated_certificate(&encryption.credential.certificate, self.callsign(), false)?;
        self.decrypt_with(encryption, &leaf, ciphertext)
    }

    fn decrypt_with(
        &self,
        encryption: &Encryption,
        leaf: &CertificateDer<'_>,
        ciphertext: &[u8],
    ) -> io::Result<Vec<u8>> {
        check_cms(ciphertext)?;
        let mut command = self.cms_provider(encryption)?;
        let mut recipient = tempfile::NamedTempFile::new()?;
        write_pem(&mut recipient, leaf.as_ref())?;
        command
            .args(["-decrypt", "-inform", "DER", "-recip"])
            .arg(recipient.path())
            .arg("-inkey")
            .arg(self.directory.join(&encryption.credential.private_key));
        run_cms(command, ciphertext)
    }
}

fn write_pem(mut output: impl io::Write, der: &[u8]) -> io::Result<()> {
    writeln!(output, "-----BEGIN CERTIFICATE-----")?;
    let encoded = STANDARD.encode(der);
    for line in encoded.as_bytes().chunks(64) {
        output.write_all(line)?;
        output.write_all(b"\n")?;
    }
    writeln!(output, "-----END CERTIFICATE-----")
}

fn run_cms(mut command: std::process::Command, input: &[u8]) -> io::Result<Vec<u8>> {
    use std::{
        io::Write,
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut source = child.stdin.take().expect("piped OpenSSL stdin");
    let destination = child.stdout.take().expect("piped OpenSSL stdout");
    let limit = input.len() + 4096;
    thread::scope(|threads| {
        let writer = threads.spawn(move || source.write_all(input));
        let reader = threads.spawn(move || {
            let mut output = Vec::new();
            destination
                .take(limit as u64 + 1)
                .read_to_end(&mut output)?;
            if output.len() > limit {
                return Err(other("OpenSSL FIPS CMS output exceeds the limit"));
            }
            Ok(output)
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if started.elapsed() < Duration::from_secs(30) => {
                    thread::sleep(Duration::from_millis(10));
                }
                outcome => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(outcome.err().unwrap_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::TimedOut,
                            "OpenSSL FIPS CMS operation timed out",
                        )
                    }));
                }
            }
        }?;
        if !status.success() {
            return Err(other("OpenSSL FIPS CMS operation failed"));
        }
        writer
            .join()
            .map_err(|_| other("OpenSSL input worker failed"))??;
        reader
            .join()
            .map_err(|_| other("OpenSSL output worker failed"))?
    })
}

pub fn verify_chat_proof(config: &Path, proof: &Path) -> io::Result<()> {
    use std::io::Write;
    let pki = Pki::load(config)?;
    let signed: Signed = serde_json::from_slice(&read_bounded(proof, 8192)?)?;
    let data = pki.verify(crate::chat_cli::pki_chat::MESSAGE, &signed, 4096)?;
    let message: serde_json::Value = serde_json::from_slice(&data)?;
    let result = serde_json::json!({
        "verified_sender": signed.from,
        "recipient": signed.to,
        "message": message,
    });
    let mut output = io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, &result)?;
    output.write_all(b"\n")
}
