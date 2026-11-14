//! TLS transports for authenticated SMTP and IMAP.

use std::io;
use tokio::io::{AsyncRead, AsyncWrite};

pub(crate) trait MailStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> MailStream for T {}

#[derive(Clone, Default)]
pub enum Transport {
    #[default]
    Plain,
    #[cfg(feature = "mail-tls")]
    Tls(tokio_rustls::TlsAcceptor),
    #[cfg(all(target_os = "linux", feature = "rocky-mail-tls"))]
    RockyTls(openssl::ssl::SslAcceptor),
}

impl Transport {
    #[cfg(all(
        feature = "mail-tls",
        not(all(target_os = "linux", feature = "rocky-mail-tls"))
    ))]
    pub fn from_pem(cert: &std::path::Path, key: &std::path::Path) -> io::Result<Self> {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
        use std::sync::Arc;
        let config = (|| {
            let chain = CertificateDer::pem_file_iter(cert)?.collect::<Result<Vec<_>, _>>()?;
            let key = PrivateKeyDer::from_pem_file(key)?;
            let config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(chain, key)?;
            Ok::<_, Box<dyn std::error::Error>>(config)
        })()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        Ok(Self::Tls(tokio_rustls::TlsAcceptor::from(Arc::new(config))))
    }

    #[cfg(all(target_os = "linux", feature = "rocky-mail-tls"))]
    pub fn from_pem(cert: &std::path::Path, key: &std::path::Path) -> io::Result<Self> {
        use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod, SslVersion};

        rocky_fips::check()?;
        let mut acceptor = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server())
            .map_err(io::Error::other)?;
        acceptor
            .set_min_proto_version(Some(SslVersion::TLS1_2))
            .map_err(io::Error::other)?;
        acceptor
            .set_certificate_chain_file(cert)
            .map_err(io::Error::other)?;
        acceptor
            .set_private_key_file(key, SslFiletype::PEM)
            .map_err(io::Error::other)?;
        acceptor.check_private_key().map_err(io::Error::other)?;
        Ok(Self::RockyTls(acceptor.build()))
    }

    pub(crate) async fn accept(
        &self,
        socket: impl MailStream + 'static,
    ) -> io::Result<Box<dyn MailStream>> {
        match self {
            #[cfg(all(target_os = "linux", feature = "rocky-mail-tls"))]
            Self::RockyTls(acceptor) => {
                let ssl = openssl::ssl::Ssl::new(acceptor.context()).map_err(io::Error::other)?;
                let mut stream =
                    tokio_openssl::SslStream::new(ssl, socket).map_err(io::Error::other)?;
                tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    std::pin::Pin::new(&mut stream).accept(),
                )
                .await?
                .map_err(io::Error::other)?;
                Ok(Box::new(stream))
            }
            Self::Plain => Ok(Box::new(socket)),
            #[cfg(feature = "mail-tls")]
            Self::Tls(acceptor) => {
                let stream = tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    acceptor.accept(socket),
                )
                .await??;
                Ok(Box::new(stream))
            }
        }
    }
}

#[cfg(all(target_os = "linux", feature = "rocky-mail-tls"))]
mod rocky_fips {
    use openssl::{error::ErrorStack, provider::Provider};
    use std::{
        ffi::{c_int, c_void},
        io, ptr,
        sync::OnceLock,
    };

    #[link(name = "crypto")]
    unsafe extern "C" {
        fn EVP_default_properties_is_fips_enabled(ctx: *mut c_void) -> c_int;
        fn EVP_set_default_properties(ctx: *mut c_void, query: *const std::ffi::c_char) -> c_int;
    }

    pub(super) fn check() -> io::Result<()> {
        static PROVIDERS: OnceLock<Result<(Provider, Provider), String>> = OnceLock::new();
        PROVIDERS
            .get_or_init(|| initialize().map_err(|error| error.to_string()))
            .as_ref()
            .map(|_| ())
            .map_err(|error| io::Error::other(error.clone()))
    }

    fn initialize() -> io::Result<(Provider, Provider)> {
        let required = || {
            io::Error::other(
                "Rocky mail TLS requires kernel FIPS mode and the system OpenSSL FIPS policy",
            )
        };
        if std::fs::read_to_string("/proc/sys/crypto/fips_enabled")
            .map_err(|_| required())?
            .trim()
            != "1"
        {
            return Err(required());
        }
        openssl::init();
        // A null context is OpenSSL's process-default context, also used by SslAcceptor.
        if unsafe { EVP_default_properties_is_fips_enabled(ptr::null_mut()) } != 1 {
            return Err(required());
        }
        let fips = Provider::load(None, "fips").map_err(io::Error::other)?;
        let base = Provider::load(None, "base").map_err(io::Error::other)?;
        // Prefer the FIPS provider for crypto while allowing base-provider support operations.
        if unsafe {
            EVP_set_default_properties(ptr::null_mut(), c"fips=yes,?provider=fips".as_ptr())
        } != 1
        {
            return Err(io::Error::other(ErrorStack::get()));
        }
        Ok((fips, base))
    }
}
