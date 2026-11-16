//! Mutually authenticated, forward-secret TLS over framed chat DATA.
use std::{
    io::{self, Read, Write},
    sync::Arc,
};

use rustls::{
    CertificateError, CipherSuite, ClientConfig, ClientConnection, Connection,
    DigitallySignedStruct, DistinguishedName, Error, NamedGroup, ProtocolVersion, ServerConfig,
    ServerConnection, SignatureScheme,
    client::{
        Resumption,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::{WebPkiSupportedAlgorithms, verify_tls13_signature},
    pki_types::{
        CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime, pem::PemObject,
    },
    server::danger::{ClientCertVerified, ClientCertVerifier},
};

use super::{ChatKeyExchange, PEM_LIMIT, Pki, other, pem_objects, read_bounded};

#[cfg(feature = "pqc-chat")]
mod pqc;

pub(super) fn check_pqc_provider() -> io::Result<()> {
    #[cfg(feature = "pqc-chat")]
    return pqc::check_provider();
    #[cfg(not(feature = "pqc-chat"))]
    Err(other(
        "PQC chat requires a pqc-chat build and OpenSSL FIPS provider 3.5.9",
    ))
}

const ALPN: &[u8] = b"linkwise-chat/1";
const CALLER_BINDING: &[u8] = b"EXPORTER-Linkwise-chat-caller";
const RESPONDER_BINDING: &[u8] = b"EXPORTER-Linkwise-chat-responder";
const WIRE_CHUNK: usize = 2304;
const MAX_PENDING_BYTES: usize = 32 * 1024;

#[derive(Debug)]
struct VerifiedPeer {
    certificate: CertificateDer<'static>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl VerifiedPeer {
    // Pki has already checked the entire configured chain, callsign, CA pin and CRLs.
    fn verify_leaf(&self, presented: &CertificateDer<'_>) -> Result<(), Error> {
        if self.certificate == *presented {
            Ok(())
        } else {
            Err(Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        if signature.scheme != SignatureScheme::ECDSA_NISTP256_SHA256 {
            return Err(Error::General(
                "unexpected Linkwise TLS signature scheme".into(),
            ));
        }
        verify_tls13_signature(message, certificate, signature, &self.algorithms)
    }
}

impl ServerCertVerifier for VerifiedPeer {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        self.verify_leaf(end_entity)
            .map(|()| ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("Linkwise chat requires TLS 1.3".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.signature(message, certificate, signature)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ECDSA_NISTP256_SHA256]
    }
}

impl ClientCertVerifier for VerifiedPeer {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        self.verify_leaf(end_entity)
            .map(|()| ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("Linkwise chat requires TLS 1.3".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.signature(message, certificate, signature)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ECDSA_NISTP256_SHA256]
    }
}

trait WireTls: Send {
    fn next(&mut self) -> io::Result<Option<Vec<u8>>>;
    fn wants_write(&self) -> bool;
    fn is_handshaking(&self) -> bool;
    fn write_plain(&mut self, plaintext: &[u8]) -> io::Result<()>;
    fn export(&self, label: &[u8], context: &[u8]) -> io::Result<[u8; 32]>;
    fn receive(&mut self, wire: &[u8]) -> io::Result<Vec<u8>>;
    fn check_profile(&self) -> io::Result<()>;
    fn key_exchange(&self) -> &'static str;
}

impl WireTls for Connection {
    fn next(&mut self) -> io::Result<Option<Vec<u8>>> {
        if !self.wants_write() {
            return Ok(None);
        }
        let mut bytes = vec![0; WIRE_CHUNK];
        let count = self.write_tls(&mut &mut bytes[..])?;
        bytes.truncate(count);
        Ok(Some(bytes))
    }

    fn wants_write(&self) -> bool {
        std::ops::Deref::deref(self).wants_write()
    }

    fn is_handshaking(&self) -> bool {
        std::ops::Deref::deref(self).is_handshaking()
    }

    fn write_plain(&mut self, plaintext: &[u8]) -> io::Result<()> {
        self.writer().write_all(plaintext)
    }

    fn export(&self, label: &[u8], context: &[u8]) -> io::Result<[u8; 32]> {
        self.export_keying_material([0; 32], label, Some(context))
            .map_err(other)
    }

    fn receive(&mut self, mut wire: &[u8]) -> io::Result<Vec<u8>> {
        while !wire.is_empty() {
            if self.read_tls(&mut wire)? == 0 {
                return Err(other("chat TLS stream closed"));
            }
            self.process_new_packets().map_err(other)?;
        }
        let mut plaintext = Vec::new();
        let mut buffer = [0; 4096];
        let mut reader = self.reader();
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => plaintext.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        Ok(plaintext)
    }

    fn check_profile(&self) -> io::Result<()> {
        if self.protocol_version() != Some(ProtocolVersion::TLSv1_3)
            || self.negotiated_cipher_suite().map(|suite| suite.suite())
                != Some(CipherSuite::TLS13_AES_256_GCM_SHA384)
            || self
                .negotiated_key_exchange_group()
                .map(|group| group.name())
                != Some(NamedGroup::secp256r1)
            || self.alpn_protocol() != Some(ALPN)
            || self.peer_certificates().is_none()
        {
            return Err(other("chat TLS negotiated an unexpected security profile"));
        }
        Ok(())
    }

    fn key_exchange(&self) -> &'static str {
        "P-256"
    }
}

pub(crate) struct ChatTls {
    connection: Box<dyn WireTls>,
    transcript: Vec<u8>,
    caller: bool,
    expected_binding: Option<[u8; 32]>,
    received_binding: Vec<u8>,
    ready: bool,
}

impl ChatTls {
    pub fn new(pki: &Pki, peer: &str, caller: bool, transcript: Vec<u8>) -> io::Result<Self> {
        if !pki.chat_encryption_allowed() {
            return Err(other("encryption is disabled for this transport"));
        }
        let certificate = pki.peer_certificate(&pki.peer(peer)?.signing_certificate, peer, true)?;
        let local = pem_objects(&pki.directory.join(&pki.profile.signing.certificate))?;
        pki.validate_chain(&local, pki.callsign(), true)?;
        let key = read_bounded(
            &pki.directory.join(&pki.profile.signing.private_key),
            PEM_LIMIT,
        )?;
        let connection: Box<dyn WireTls> = match pki.profile.chat_key_exchange {
            ChatKeyExchange::P256 => Box::new(classical_connection(
                local,
                &key,
                certificate,
                peer,
                caller,
            )?),
            ChatKeyExchange::P256Mlkem768 => {
                #[cfg(feature = "pqc-chat")]
                {
                    Box::new(pqc::PqcTls::new(
                        local[0].as_ref(),
                        &key,
                        certificate.as_ref(),
                        caller,
                    )?)
                }
                #[cfg(not(feature = "pqc-chat"))]
                {
                    return Err(other(
                        "PQC chat requires a pqc-chat build and OpenSSL FIPS provider 3.5.9",
                    ));
                }
            }
        };
        Ok(Self {
            connection,
            transcript,
            caller,
            expected_binding: None,
            received_binding: Vec::new(),
            ready: false,
        })
    }

    pub fn next(&mut self) -> io::Result<Option<Vec<u8>>> {
        self.connection.next()
    }

    pub fn send(&mut self, plaintext: &[u8]) -> io::Result<()> {
        if !self.ready || self.connection.wants_write() {
            return Err(other("wait for encrypted chat to finish sending"));
        }
        self.connection.write_plain(plaintext)
    }

    pub fn receive(&mut self, wire: &[u8]) -> io::Result<Vec<u8>> {
        if wire.is_empty() || wire.len() > WIRE_CHUNK {
            return Err(other("invalid chat TLS chunk size"));
        }
        let mut plaintext = self.connection.receive(wire)?;
        if self.connection.is_handshaking() {
            return Ok(Vec::new());
        }
        if self.expected_binding.is_none() {
            self.connection.check_profile()?;
            let (local, peer) = if self.caller {
                (CALLER_BINDING, RESPONDER_BINDING)
            } else {
                (RESPONDER_BINDING, CALLER_BINDING)
            };
            let binding = self.connection.export(local, &self.transcript)?;
            self.connection.write_plain(&binding)?;
            self.expected_binding = Some(self.connection.export(peer, &self.transcript)?);
        }
        if !self.ready {
            let take = (32 - self.received_binding.len()).min(plaintext.len());
            self.received_binding.extend(plaintext.drain(..take));
            if self.received_binding.len() == 32 {
                if self
                    .expected_binding
                    .as_ref()
                    .map(|binding| binding.as_slice())
                    != Some(&self.received_binding)
                {
                    return Err(other("TLS binding does not match this BW500 chat"));
                }
                self.ready = true;
            }
        }
        Ok(plaintext)
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn key_exchange(&self) -> &'static str {
        self.connection.key_exchange()
    }
}

fn classical_connection(
    local: Vec<CertificateDer<'static>>,
    key: &[u8],
    certificate: CertificateDer<'static>,
    peer: &str,
    caller: bool,
) -> io::Result<Connection> {
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from_pem_slice(key).map_err(other)?);
    let provider = crate::secure_cli::fips_tls_provider();
    let verifier = Arc::new(VerifiedPeer {
        certificate,
        algorithms: provider.signature_verification_algorithms,
    });
    let mut connection = if caller {
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(other)?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_client_auth_cert(local, key)
            .map_err(other)?;
        config.resumption = Resumption::disabled();
        config.enable_early_data = false;
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.max_fragment_size = Some(1024);
        if !config.fips() {
            return Err(other("chat TLS client failed the FIPS provider check"));
        }
        let name = ServerName::try_from(format!(
            "{}.radio.linkwise.invalid",
            peer.to_ascii_lowercase()
        ))
        .map_err(other)?;
        Connection::Client(ClientConnection::new(Arc::new(config), name).map_err(other)?)
    } else {
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(other)?
            .with_client_cert_verifier(verifier)
            .with_single_cert(local, key)
            .map_err(other)?;
        config.send_tls13_tickets = 0;
        config.max_early_data_size = 0;
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.max_fragment_size = Some(1024);
        if !config.fips() {
            return Err(other("chat TLS server failed the FIPS provider check"));
        }
        Connection::Server(ServerConnection::new(Arc::new(config)).map_err(other)?)
    };
    connection.set_buffer_limit(Some(MAX_PENDING_BYTES));
    Ok(connection)
}
