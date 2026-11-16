//! OpenSSL 3.5.9's validated FIPS provider for hybrid post-quantum chat.
use std::{
    collections::VecDeque,
    ffi::{CStr, c_char, c_int},
    io::{self, Read, Write},
    ptr,
    sync::OnceLock,
};

use foreign_types::{ForeignType, ForeignTypeRef};
use openssl::{
    pkey::PKey,
    provider::Provider,
    ssl::{
        AlpnError, ErrorCode, Ssl, SslContextBuilder, SslMethod, SslSessionCacheMode, SslStream,
        SslVerifyMode, SslVersion, select_next_proto,
    },
    x509::{X509, X509VerifyResult},
};
use openssl_sys as sys;

use super::{ALPN, MAX_PENDING_BYTES, WIRE_CHUNK, WireTls, other};

const PROVIDER_VERSION: &[u8] = b"3.5.9";
const TLS_GROUP: &str = "SecP256r1MLKEM768";
const TLS_CIPHER: &str = "TLS_AES_256_GCM_SHA384";
const VERIFY_GROUP: c_int = 134; // SSL_CTRL_GET_NEGOTIATED_GROUP from <openssl/ssl.h>.
const ALPN_WIRE: &[u8] = b"\x0flinkwise-chat/1";

// These OpenSSL 3 APIs are absent from the openssl-sys bindings used here.
unsafe extern "C" {
    fn OSSL_PROVIDER_get_params(
        provider: *const sys::OSSL_PROVIDER,
        params: *mut sys::OSSL_PARAM,
    ) -> c_int;
    fn OSSL_PARAM_construct_utf8_ptr(
        key: *const c_char,
        buffer: *mut *mut c_char,
        size: usize,
    ) -> sys::OSSL_PARAM;
    fn OSSL_PROVIDER_self_test(provider: *const sys::OSSL_PROVIDER) -> c_int;
    fn SSL_group_to_name(ssl: *mut sys::SSL, id: c_int) -> *const c_char;
}

pub(super) fn check_provider() -> io::Result<()> {
    static STATUS: OnceLock<Result<(), String>> = OnceLock::new();
    STATUS
        .get_or_init(|| check_installed_provider().map_err(|error| error.to_string()))
        .as_ref()
        .map(|()| ())
        .map_err(other)
}

fn check_installed_provider() -> io::Result<()> {
    openssl::init();
    if !openssl::version::version().starts_with("OpenSSL 3.5.9 ") {
        return Err(other("PQC chat requires OpenSSL 3.5.9 (CMVP #5586)"));
    }
    if unsafe { sys::EVP_default_properties_is_fips_enabled(ptr::null_mut()) } != 1 {
        return Err(other(
            "PQC chat requires an OpenSSL configuration with fips=yes",
        ));
    }
    let provider = Provider::load(None, "fips").map_err(other)?;
    // OpenSSL writes a provider-owned NUL-terminated version pointer. Keep the
    // provider live until we finish reading it and running its integrity tests.
    unsafe {
        let mut version = ptr::null_mut();
        let mut security_checks = 0;
        let mut params = [
            OSSL_PARAM_construct_utf8_ptr(c"version".as_ptr(), &mut version, 0),
            sys::OSSL_PARAM_construct_uint(c"security-checks".as_ptr(), &mut security_checks),
            sys::OSSL_PARAM_construct_end(),
        ];
        if OSSL_PROVIDER_get_params(provider.as_ptr(), params.as_mut_ptr()) != 1
            || version.is_null()
            || CStr::from_ptr(version).to_bytes() != PROVIDER_VERSION
            || security_checks != 1
            || OSSL_PROVIDER_self_test(provider.as_ptr()) != 1
        {
            return Err(other(
                "loaded FIPS provider is not a self-tested OpenSSL 3.5.9 module",
            ));
        }
    }
    Ok(())
}

#[derive(Default)]
struct Wire {
    incoming: VecDeque<u8>,
    outgoing: VecDeque<u8>,
}

impl Read for Wire {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.incoming.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        self.incoming.read(bytes)
    }
}

impl Write for Wire {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.outgoing.len() + bytes.len() > MAX_PENDING_BYTES {
            return Err(other("PQC TLS send buffer exceeds the limit"));
        }
        self.outgoing.extend(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) struct PqcTls {
    stream: SslStream<Wire>,
    handshaking: bool,
    // Keep both providers alive until after the SSL stream is dropped.
    _providers: (Provider, Provider),
}

impl PqcTls {
    pub fn new(local: &[u8], key: &[u8], peer: &[u8], caller: bool) -> io::Result<Self> {
        check_provider()?;
        let providers = (
            Provider::load(None, "fips").map_err(other)?,
            Provider::load(None, "base").map_err(other)?,
        );
        let mut config = SslContextBuilder::new(SslMethod::tls()).map_err(other)?;
        config
            .set_min_proto_version(Some(SslVersion::TLS1_3))
            .map_err(other)?;
        config
            .set_max_proto_version(Some(SslVersion::TLS1_3))
            .map_err(other)?;
        config.set_groups_list(TLS_GROUP).map_err(other)?;
        config.set_ciphersuites(TLS_CIPHER).map_err(other)?;
        config
            .set_sigalgs_list("ecdsa_secp256r1_sha256")
            .map_err(other)?;
        config.set_session_cache_mode(SslSessionCacheMode::OFF);
        config.set_num_tickets(0).map_err(other)?;
        config.set_max_early_data(0).map_err(other)?;
        let local = X509::from_der(local).map_err(other)?;
        let key = PKey::private_key_from_pem(key).map_err(other)?;
        config.set_certificate(&local).map_err(other)?;
        config.set_private_key(&key).map_err(other)?;
        config.check_private_key().map_err(other)?;

        // Linkwise has checked the configured chain, callsign, CA pin and CRLs.
        // Compare the leaf exactly and let TLS verify possession of its key.
        let expected = peer.to_vec();
        config.set_verify_callback(
            SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT,
            move |_, store| {
                let accepted = store.error_depth() != 0
                    || store
                        .current_cert()
                        .and_then(|certificate| certificate.to_der().ok())
                        .as_deref()
                        == Some(expected.as_slice());
                if accepted {
                    store.set_error(X509VerifyResult::OK);
                }
                accepted
            },
        );
        if caller {
            config.set_alpn_protos(ALPN_WIRE).map_err(other)?;
        } else {
            config.set_alpn_select_callback(|_, offered| {
                select_next_proto(ALPN_WIRE, offered).ok_or(AlpnError::ALERT_FATAL)
            });
        }
        let mut ssl = Ssl::new(&config.build()).map_err(other)?;
        if caller {
            ssl.set_connect_state();
        } else {
            ssl.set_accept_state();
        }
        let stream = SslStream::new(ssl, Wire::default()).map_err(other)?;
        let mut tls = Self {
            stream,
            handshaking: true,
            _providers: providers,
        };
        tls.advance_handshake()?;
        Ok(tls)
    }

    fn advance_handshake(&mut self) -> io::Result<()> {
        match self.stream.do_handshake() {
            Ok(()) => {
                self.handshaking = false;
                let ssl = self.stream.ssl();
                // Use the OpenSSL group name rather than the library-specific group NID.
                let group = unsafe {
                    let id = sys::SSL_ctrl(ssl.as_ptr(), VERIFY_GROUP, 0, ptr::null_mut());
                    let name = SSL_group_to_name(ssl.as_ptr(), id as c_int);
                    (!name.is_null()).then(|| CStr::from_ptr(name).to_bytes())
                };
                if group.is_none_or(|name| !name.eq_ignore_ascii_case(TLS_GROUP.as_bytes()))
                    || ssl.version2() != Some(SslVersion::TLS1_3)
                    || ssl.current_cipher().map(|cipher| cipher.name()) != Some(TLS_CIPHER)
                    || ssl.selected_alpn_protocol() != Some(ALPN)
                    || ssl.peer_certificate().is_none()
                    || ssl.session_reused()
                {
                    return Err(other(
                        "PQC chat TLS negotiated an unexpected security profile",
                    ));
                }
                Ok(())
            }
            Err(error) if error.code() == ErrorCode::WANT_READ => Ok(()),
            Err(error) => Err(other(error)),
        }
    }
}

impl WireTls for PqcTls {
    fn next(&mut self) -> io::Result<Option<Vec<u8>>> {
        let queue = &mut self.stream.get_mut().outgoing;
        Ok((!queue.is_empty()).then(|| queue.drain(..queue.len().min(WIRE_CHUNK)).collect()))
    }

    fn wants_write(&self) -> bool {
        !self.stream.get_ref().outgoing.is_empty()
    }

    fn is_handshaking(&self) -> bool {
        self.handshaking
    }

    fn write_plain(&mut self, input: &[u8]) -> io::Result<()> {
        self.stream.write_all(input)
    }

    fn export(&self, label: &[u8], context: &[u8]) -> io::Result<[u8; 32]> {
        let mut bytes = [0; 32];
        // The exporter label is ASCII and contains no interior NUL.
        let label = std::str::from_utf8(label).map_err(other)?;
        self.stream
            .ssl()
            .export_keying_material(&mut bytes, label, Some(context))
            .map_err(other)?;
        Ok(bytes)
    }

    fn receive(&mut self, wire: &[u8]) -> io::Result<Vec<u8>> {
        let incoming = &mut self.stream.get_mut().incoming;
        if incoming.len() + wire.len() > MAX_PENDING_BYTES {
            return Err(other("PQC TLS receive buffer exceeds the limit"));
        }
        incoming.extend(wire);
        if self.handshaking {
            self.advance_handshake()?;
        }
        let mut plaintext = Vec::new();
        if !self.handshaking {
            let mut buffer = [0; 4096];
            loop {
                match self.stream.ssl_read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => plaintext.extend_from_slice(&buffer[..count]),
                    Err(error) if error.code() == ErrorCode::WANT_READ => break,
                    Err(error) => return Err(other(error)),
                }
            }
        }
        Ok(plaintext)
    }
    fn check_profile(&self) -> io::Result<()> {
        // Checked when the OpenSSL handshake completed.
        Ok(())
    }

    fn key_exchange(&self) -> &'static str {
        "P-256 + ML-KEM-768"
    }
}
