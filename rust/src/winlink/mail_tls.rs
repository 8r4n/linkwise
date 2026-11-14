//! Optional implicit TLS transport for the loopback mail bench.

use std::io;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};

pub(crate) trait MailStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> MailStream for T {}

#[derive(Clone, Default)]
pub enum Transport {
    #[default]
    Plain,
    #[cfg(feature = "mail-tls")]
    Tls(tokio_rustls::TlsAcceptor),
}

impl Transport {
    #[cfg(feature = "mail-tls")]
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

    pub(crate) async fn accept(&self, socket: TcpStream) -> io::Result<Box<dyn MailStream>> {
        match self {
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
