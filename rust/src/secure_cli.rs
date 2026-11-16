//! Mutually authenticated file transfer over the modem's ordered DATA stream.
use std::{ffi::OsString, io, path::PathBuf, sync::Arc, time::Duration};

use aws_lc_rs::digest::{Context, SHA256};
use rustls::{
    CipherSuite, ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject},
    server::WebPkiClientVerifier,
};
use serde::Deserialize;
use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::{frame_cli::Failure, session_cli::Control};

const PROTOCOL: &[u8] = b"linkwise-transfer/2";
const CHAT_PROTOCOL: &[u8] = b"linkwise-transfer/2+gzip";
const REQUIRED_CIPHER: CipherSuite = CipherSuite::TLS13_AES_256_GCM_SHA384;
const CHUNK: usize = 512;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);
const IO_TIMEOUT: Duration = Duration::from_secs(180);

pub(crate) fn fips_tls_provider() -> Arc<rustls::crypto::CryptoProvider> {
    let mut provider = rustls::crypto::aws_lc_rs::default_provider();
    provider.cipher_suites =
        vec![rustls::crypto::aws_lc_rs::cipher_suite::TLS13_AES_256_GCM_SHA384];
    provider.kx_groups = vec![rustls::crypto::aws_lc_rs::kx_group::SECP256R1];
    Arc::new(provider)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default)]
    encryption: Encryption,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Encryption {
    #[serde(default)]
    enabled: bool,
    settings: Option<Settings>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    local_call: String,
    peer_call: String,
    peer_dns: String,
    peer_certificate: PathBuf,
    certificate: PathBuf,
    private_key: PathBuf,
    ca_certificate: PathBuf,
    command_port: u16,
    data_port: u16,
    max_bytes: u64,
}

impl Settings {
    fn validate(&self) -> io::Result<()> {
        let valid_call = |call: &str| {
            !call.is_empty()
                && call.len() <= 12
                && call
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
        };
        if !valid_call(&self.local_call)
            || !valid_call(&self.peer_call)
            || self.local_call == self.peer_call
            || self.command_port == 0
            || self.data_port == 0
            || self.command_port == self.data_port
            || self.max_bytes == 0
        {
            return Err(io::Error::other("invalid secure modem settings"));
        }
        Ok(())
    }

    fn tls(
        &self,
        protocol: &[u8],
    ) -> Result<(ClientConfig, ServerConfig), Box<dyn std::error::Error>> {
        let provider = fips_tls_provider();
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from_pem_file(&self.ca_certificate)?)?;
        let certificate = CertificateDer::from_pem_file(&self.certificate)?;
        let key = PrivateKeyDer::from_pem_file(&self.private_key)?;
        let verifier =
            WebPkiClientVerifier::builder_with_provider(Arc::new(roots.clone()), provider.clone())
                .build()?;
        let mut client = ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_root_certificates(roots)
            .with_client_auth_cert(vec![certificate.clone()], key.clone_key())?;
        let mut server = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![certificate], key)?;
        client.resumption = rustls::client::Resumption::disabled();
        server.send_tls13_tickets = 0;
        client.alpn_protocols = vec![protocol.to_vec()];
        server.alpn_protocols = vec![protocol.to_vec()];
        client.max_fragment_size = Some(CHUNK);
        server.max_fragment_size = Some(CHUNK);
        if !client.fips() || !server.fips() {
            return Err("TLS configuration did not pass the provider's FIPS check".into());
        }
        Ok((client, server))
    }
}

pub(crate) async fn run(args: &[OsString]) -> Result<(), Failure> {
    let [mode, config, path] = args else {
        return Err(Failure::usage(
            "secure send|receive CONFIG FILE (use - for stdin or stdout)",
        ));
    };
    let send = match mode.to_str() {
        Some("send") => true,
        Some("receive") => false,
        _ => return Err(Failure::usage("secure mode must be send or receive")),
    };
    tokio::select! {
        biased;
        event = linkwise::signals::stop() => Err(crate::frame_cli::interruption(
            event, "secure transfer interrupted; delivery may be unconfirmed",
        )),
        result = execute(send, config, path) => result.map_err(Failure::operation),
    }
}

fn verify_cipher(suite: Option<CipherSuite>) -> io::Result<()> {
    if suite != Some(REQUIRED_CIPHER) {
        return Err(io::Error::other(
            "secure transfer requires TLS_AES_256_GCM_SHA384",
        ));
    }
    Ok(())
}

fn verify_peer(
    certificates: Option<&[CertificateDer<'_>]>,
    alpn: Option<&[u8]>,
    expected: &CertificateDer<'_>,
    protocol: &[u8],
) -> io::Result<()> {
    if alpn != Some(protocol) || certificates.and_then(|certs| certs.first()) != Some(expected) {
        return Err(io::Error::other(
            "unexpected secure transfer protocol or peer certificate",
        ));
    }
    Ok(())
}

async fn execute(
    send: bool,
    config: &OsString,
    path: &OsString,
) -> Result<(), Box<dyn std::error::Error>> {
    transfer(send, load_settings(config)?, path).await
}

fn load_settings(config: &OsString) -> Result<Settings, Box<dyn std::error::Error>> {
    let config: Config = serde_json::from_reader(std::fs::File::open(config)?)?;
    if !config.encryption.enabled {
        return Err("encryption is disabled in the configuration".into());
    }
    let settings = config
        .encryption
        .settings
        .ok_or("missing encryption settings")?;
    settings.validate()?;
    Ok(settings)
}

pub(crate) struct ChatTransfer {
    send: bool,
    settings: Settings,
    path: OsString,
}

impl ChatTransfer {
    pub(crate) fn prepare(
        send: bool,
        config: &OsString,
        path: OsString,
        local: &str,
        peer: Option<&str>,
        ports: (u16, u16),
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let settings = load_settings(config)?;
        if settings.local_call != local
            || peer.is_some_and(|peer| peer != settings.peer_call)
            || (settings.command_port, settings.data_port) != ports
        {
            return Err(
                "secure configuration does not match the chat station, peer, or ports".into(),
            );
        }
        Ok(Self {
            send,
            settings,
            path,
        })
    }

    pub(crate) async fn run(
        self,
        control: &mut Control,
        data: &mut TcpStream,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.send {
            let limit = self.settings.max_bytes;
            let path = self.path;
            let gzip = tokio::task::spawn_blocking(move || gzip_input(path, limit)).await??;
            let compressed = gzip.path().as_os_str().to_owned();
            transfer_on_host(true, self.settings, &compressed, true, control, data).await
        } else {
            transfer_on_host(false, self.settings, &self.path, true, control, data).await
        }
    }
}

fn gzip_input(path: OsString, max: u64) -> io::Result<tempfile::NamedTempFile> {
    use std::io::{Read, Write};
    if !std::fs::metadata(&path)?.is_file() {
        return Err(io::Error::other("choose a regular file"));
    }
    let source = std::fs::File::open(path)?;
    let mut temp = tempfile::NamedTempFile::new()?;
    let mut gzip = flate2::write::GzEncoder::new(temp.as_file_mut(), flate2::Compression::fast());
    if io::copy(&mut source.take(max.saturating_add(1)), &mut gzip)? > max {
        return Err(io::Error::other("file exceeds the configured size limit"));
    }
    gzip.finish()?.flush()?;
    Ok(temp)
}

fn publish_gzip(
    compressed: tempfile::TempPath,
    destination: PathBuf,
    directory: PathBuf,
    max: u64,
) -> io::Result<()> {
    use std::io::{BufRead, Read};
    let file = std::fs::File::open(&compressed)?;
    let mut gzip = flate2::bufread::GzDecoder::new(io::BufReader::new(file));
    let mut output = tempfile::NamedTempFile::new_in(directory)?;
    if io::copy(&mut (&mut gzip).take(max.saturating_add(1)), &mut output)? > max
        || !gzip.get_mut().fill_buf()?.is_empty()
    {
        return Err(io::Error::other(
            "expanded file exceeds the limit or has trailing data",
        ));
    }
    output.as_file().sync_all()?;
    output
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    Ok(())
}

async fn transfer(
    send: bool,
    settings: Settings,
    path: &OsString,
) -> Result<(), Box<dyn std::error::Error>> {
    let localhost = std::net::Ipv4Addr::LOCALHOST;
    let (control, mut data) = timeout(Duration::from_secs(5), async {
        tokio::try_join!(
            TcpStream::connect((localhost, settings.command_port)),
            TcpStream::connect((localhost, settings.data_port))
        )
    })
    .await??;
    transfer_on_host(
        send,
        settings,
        path,
        false,
        &mut Control::new(control),
        &mut data,
    )
    .await
}

async fn transfer_on_host(
    send: bool,
    settings: Settings,
    path: &OsString,
    gzip: bool,
    control: &mut Control,
    data: &mut TcpStream,
) -> Result<(), Box<dyn std::error::Error>> {
    aws_lc_rs::try_fips_mode().map_err(io::Error::other)?;
    let expected = CertificateDer::from_pem_file(&settings.peer_certificate)?;
    let dns = ServerName::try_from(settings.peer_dns.clone())?;
    let protocol = if gzip { CHAT_PROTOCOL } else { PROTOCOL };
    let limit = if gzip {
        settings
            .max_bytes
            .saturating_add(settings.max_bytes / 64)
            .saturating_add(128)
    } else {
        settings.max_bytes
    };
    let (client, server) = settings.tls(protocol)?;
    let (source, destination) = if send {
        (&settings.local_call, &settings.peer_call)
    } else {
        (&settings.peer_call, &settings.local_call)
    };
    timeout(CONNECT_TIMEOUT, async {
        control
            .command(&format!("MYCALL {}", settings.local_call))
            .await?;
        control.command("COMPRESSION OFF").await?;
        if send {
            control.command("P2P SESSION").await?;
            control
                .command(&format!("CONNECT {source} {destination}"))
                .await?;
        } else {
            control.command("LISTEN ON").await?;
        }
        loop {
            let line = control.line().await?;
            let mut fields = line.split_ascii_whitespace();
            match fields.next() {
                Some("CONNECTED") => {
                    if fields.next() == Some(source) && fields.next() == Some(destination) {
                        return Ok(());
                    }
                    return Err(io::Error::other("unexpected modem peer"));
                }
                Some("DISCONNECTED") => {
                    return Err(io::Error::other("modem disconnected before TLS"));
                }
                _ => {}
            }
        }
    })
    .await??;

    let transfer = async {
        if send {
            let mut tls = timeout(
                IO_TIMEOUT,
                TlsConnector::from(Arc::new(client)).connect(dns, &mut *data),
            )
            .await??;
            verify_cipher(
                tls.get_ref()
                    .1
                    .negotiated_cipher_suite()
                    .map(|suite| suite.suite()),
            )?;
            verify_peer(
                tls.get_ref().1.peer_certificates(),
                tls.get_ref().1.alpn_protocol(),
                &expected,
                protocol,
            )?;
            if path == "-" {
                let (mut input, status) = stdin_stream();
                send_file(&mut tls, &mut input, limit, Some(status)).await?;
            } else {
                send_file(&mut tls, &mut File::open(path).await?, limit, None).await?;
            }
            tls.get_mut().1.send_close_notify();
            timeout(IO_TIMEOUT, tls.flush()).await??;
            // The peer's close-notify confirms that ours crossed the modem.
            wait_for_tls_close(&mut tls).await?;
        } else {
            let mut tls = timeout(
                IO_TIMEOUT,
                TlsAcceptor::from(Arc::new(server)).accept(&mut *data),
            )
            .await??;
            verify_cipher(
                tls.get_ref()
                    .1
                    .negotiated_cipher_suite()
                    .map(|suite| suite.suite()),
            )?;
            verify_peer(
                tls.get_ref().1.peer_certificates(),
                tls.get_ref().1.alpn_protocol(),
                &expected,
                protocol,
            )?;
            if path == "-" {
                let digest = receive_file(&mut tls, &mut tokio::io::stdout(), limit).await?;
                receipt(&mut tls, &digest).await?;
            } else {
                let destination = PathBuf::from(path);
                let directory = destination
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(std::path::Path::new("."))
                    .to_owned();
                let (file, temporary) = tempfile::NamedTempFile::new_in(&directory)?.into_parts();
                let mut file = File::from_std(file);
                let digest = receive_file(&mut tls, &mut file, limit).await?;
                file.sync_all().await?;
                drop(file);
                if gzip {
                    let max = settings.max_bytes;
                    tokio::task::spawn_blocking(move || {
                        publish_gzip(temporary, destination, directory, max)
                    })
                    .await
                    .map_err(io::Error::other)??;
                } else {
                    temporary.persist_noclobber(&destination)?;
                }
                receipt(&mut tls, &digest).await?;
            }
            wait_for_tls_close(&mut tls).await?;
            tls.get_mut().1.send_close_notify();
            timeout(IO_TIMEOUT, tls.flush()).await??;
        }
        Ok::<_, io::Error>(())
    };
    // Drain asynchronous modem notices while TLS is active. A receiver may see
    // DISCONNECTED at the same time as the final TLS bytes, so DATA owns EOF.
    let mut disconnected = false;
    let result = tokio::select! {
        biased;
        result = transfer => result,
        result = async {
            while control.line().await? != "DISCONNECTED" {}
            disconnected = true;
            // Control and DATA are separate TCP streams. Allow final DATA
            // already delivered by the modem to reach the TLS reader.
            tokio::time::sleep(Duration::from_secs(2)).await;
            Err::<(), _>(io::Error::other("modem disconnected before secure transfer completed"))
        } => result,
    };
    if let Err(error) = result {
        if !disconnected {
            let _ = timeout(Duration::from_secs(30), async {
                control.request("DISCONNECT").await?;
                while control.line().await? != "DISCONNECTED" {}
                Ok::<_, io::Error>(())
            })
            .await;
        }
        return Err(error.into());
    }
    timeout(CONNECT_TIMEOUT, async {
        if send && !disconnected {
            control.request("DISCONNECT").await?;
        }
        while !disconnected {
            match control.line().await?.as_str() {
                "DISCONNECTED" => disconnected = true,
                "WRONG" => return Err(io::Error::other("modem rejected disconnect")),
                _ => {}
            }
        }
        Ok::<_, io::Error>(())
    })
    .await??;
    Ok(())
}

async fn wait_for_tls_close(reader: &mut (impl AsyncRead + Unpin)) -> io::Result<()> {
    let mut unexpected = [0];
    if timeout(IO_TIMEOUT, reader.read(&mut unexpected)).await?? != 0 {
        return Err(io::Error::other("unexpected data after receipt"));
    }
    Ok(())
}

fn stdin_stream() -> (
    tokio::io::DuplexStream,
    tokio::sync::oneshot::Receiver<io::Result<()>>,
) {
    use std::io::Read;

    let (reader, mut writer) = tokio::io::duplex(CHUNK);
    let runtime = tokio::runtime::Handle::current();
    let (status, result) = tokio::sync::oneshot::channel();
    // A blocked terminal read must not keep the Tokio runtime alive on shutdown.
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        let mut buffer = [0; CHUNK];
        loop {
            match input.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    if runtime.block_on(writer.write_all(&buffer[..size])).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    let _ = status.send(Err(error));
                    return;
                }
            }
        }
        let _ = status.send(Ok(()));
    });
    (reader, result)
}

async fn send_file(
    wire: &mut (impl AsyncRead + AsyncWrite + Unpin),
    input: &mut (impl AsyncRead + Unpin),
    max: u64,
    input_status: Option<tokio::sync::oneshot::Receiver<io::Result<()>>>,
) -> io::Result<()> {
    let mut hash = Context::new(&SHA256);
    let mut total = 0u64;
    let mut frame = [0u8; CHUNK + 2];
    loop {
        let size = timeout(IO_TIMEOUT, input.read(&mut frame[2..])).await??;
        if size == 0 {
            break;
        }
        total = checked_size(total, size, max)?;
        hash.update(&frame[2..size + 2]);
        frame[..2].copy_from_slice(&(size as u16).to_be_bytes());
        timeout(IO_TIMEOUT, wire.write_all(&frame[..size + 2])).await??;
        timeout(IO_TIMEOUT, wire.flush()).await??;
        let acknowledgment = timeout(IO_TIMEOUT, wire.read_u8()).await??;
        if acknowledgment != 1 {
            return Err(io::Error::other("invalid chunk acknowledgment"));
        }
    }
    if let Some(status) = input_status {
        status.await.map_err(io::Error::other)??;
    }
    let digest = hash.finish();
    let mut end = [0u8; 42];
    end[2..10].copy_from_slice(&total.to_be_bytes());
    end[10..].copy_from_slice(digest.as_ref());
    timeout(IO_TIMEOUT, wire.write_all(&end)).await??;
    timeout(IO_TIMEOUT, wire.flush()).await??;
    let mut confirmation = [0; 32];
    timeout(IO_TIMEOUT, wire.read_exact(&mut confirmation))
        .await?
        .map_err(|error| io::Error::other(format!("delivery unconfirmed: {error}")))?;
    if confirmation != digest.as_ref() {
        return Err(io::Error::other(
            "delivery unconfirmed: receipt digest mismatch",
        ));
    }
    Ok(())
}

fn checked_size(total: u64, chunk: usize, max: u64) -> io::Result<u64> {
    total
        .checked_add(chunk as u64)
        .filter(|size| *size <= max)
        .ok_or_else(|| io::Error::other("secure transfer exceeds configured size limit"))
}

async fn receive_file(
    wire: &mut (impl AsyncRead + AsyncWrite + Unpin),
    output: &mut (impl AsyncWrite + Unpin),
    max: u64,
) -> io::Result<[u8; 32]> {
    let mut hash = Context::new(&SHA256);
    let mut total = 0u64;
    let mut data = [0; CHUNK];
    loop {
        let size = timeout(IO_TIMEOUT, wire.read_u16()).await?? as usize;
        if size == 0 {
            let mut end = [0; 40];
            timeout(IO_TIMEOUT, wire.read_exact(&mut end)).await??;
            let digest = hash.finish();
            if end[..8] != total.to_be_bytes() || end[8..] != *digest.as_ref() {
                return Err(io::Error::other(
                    "secure transfer length or digest mismatch",
                ));
            }
            timeout(IO_TIMEOUT, output.flush()).await??;
            return Ok(end[8..].try_into().expect("SHA-256 digest length"));
        }
        if size > CHUNK {
            return Err(io::Error::other("oversized secure transfer record"));
        }
        total = checked_size(total, size, max)?;
        timeout(IO_TIMEOUT, wire.read_exact(&mut data[..size])).await??;
        hash.update(&data[..size]);
        timeout(IO_TIMEOUT, output.write_all(&data[..size])).await??;
        timeout(IO_TIMEOUT, wire.write_u8(1)).await??;
        timeout(IO_TIMEOUT, wire.flush()).await??;
    }
}

async fn receipt(wire: &mut (impl AsyncWrite + Unpin), digest: &[u8; 32]) -> io::Result<()> {
    timeout(IO_TIMEOUT, wire.write_all(digest)).await??;
    timeout(IO_TIMEOUT, wire.flush()).await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[test]
    fn configuration_is_opt_in_and_strict() {
        for json in [
            "{}",
            r#"{"encryption":{}}"#,
            r#"{"encryption":{"enabled":false}}"#,
        ] {
            let config: Config = serde_json::from_str(json).unwrap();
            assert!(!config.encryption.enabled);
        }
        assert!(serde_json::from_str::<Config>(r#"{"encryption":{"enable":true}}"#).is_err());
        assert!(serde_json::from_str::<Config>(r#"{"encryption":{"enabled":"true"}}"#).is_err());
    }

    #[test]
    fn rejects_aes_128_and_requires_a_negotiated_aes_256_suite() {
        assert!(verify_cipher(Some(CipherSuite::TLS13_AES_256_GCM_SHA384)).is_ok());
        assert!(verify_cipher(Some(CipherSuite::TLS13_AES_128_GCM_SHA256)).is_err());
        assert!(verify_cipher(None).is_err());
    }

    #[test]
    fn peer_pin_and_protocol_are_both_required() {
        let pin = CertificateDer::from(&b"expected"[..]);
        let other = CertificateDer::from(&b"other"[..]);
        assert!(verify_peer(Some(&[pin.clone()]), Some(PROTOCOL), &pin, PROTOCOL).is_ok());
        assert!(verify_peer(Some(&[other]), Some(PROTOCOL), &pin, PROTOCOL).is_err());
        assert!(verify_peer(Some(&[pin.clone()]), None, &pin, PROTOCOL).is_err());
        assert!(verify_peer(None, Some(PROTOCOL), &pin, PROTOCOL).is_err());
        assert!(verify_peer(Some(&[pin.clone()]), Some(PROTOCOL), &pin, CHAT_PROTOCOL).is_err());
    }

    #[tokio::test]
    async fn exact_receipt_for_empty_and_multirecord_binary_input() {
        for size in [0, 1, CHUNK, CHUNK * 3 + 17] {
            let bytes: Vec<u8> = (0..size).map(|i| i as u8).collect();
            let (mut sender, mut receiver) = duplex(97);
            let mut input = bytes.as_slice();
            let mut output = Vec::new();
            let (sent, received) = tokio::join!(
                send_file(&mut sender, &mut input, size as u64, None),
                async {
                    let digest = receive_file(&mut receiver, &mut output, size as u64).await?;
                    receipt(&mut receiver, &digest).await
                }
            );
            sent.unwrap();
            received.unwrap();
            assert_eq!(output, bytes);
        }
    }

    #[tokio::test]
    async fn malformed_oversized_truncated_and_over_limit_records_fail() {
        let oversized = ((CHUNK + 1) as u16).to_be_bytes();
        let invalid_end = [0; 42];
        for (data, max, expected) in [
            (&oversized[..], 512, "oversized"),
            (&[0, 2, 1, 2][..], 1, "size limit"),
            (&[0, 3, 1][..], 512, "early eof"),
            (&invalid_end[..], 512, "digest mismatch"),
        ] {
            let (mut sender, mut receiver) = duplex(128);
            sender.write_all(data).await.unwrap();
            sender.shutdown().await.unwrap();
            let mut output = Vec::new();
            let error = receive_file(&mut receiver, &mut output, max)
                .await
                .unwrap_err();
            assert!(
                error.to_string().to_lowercase().contains(expected),
                "{error}"
            );
            assert!(output.is_empty());
        }
        assert!(checked_size(u64::MAX, 1, u64::MAX).is_err());
    }

    #[tokio::test]
    async fn sender_rejects_forged_or_missing_receipt() {
        for forged in [true, false] {
            let (mut sender, mut receiver) = duplex(128);
            let mut input = &[][..];
            let (sent, _) = tokio::join!(send_file(&mut sender, &mut input, 10, None), async {
                receiver.read_exact(&mut [0; 42]).await.unwrap();
                if forged {
                    receiver.write_all(&[0; 32]).await.unwrap();
                }
                receiver.shutdown().await.unwrap();
            });
            assert!(
                sent.unwrap_err()
                    .to_string()
                    .contains("delivery unconfirmed")
            );
        }
    }

    #[tokio::test]
    async fn sender_rejects_invalid_ack_and_input_beyond_limit() {
        let (mut sender, mut receiver) = duplex(32);
        receiver.write_u8(9).await.unwrap();
        let error = send_file(&mut sender, &mut &b"a"[..], 1, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("invalid chunk acknowledgment"));

        let (mut sender, _receiver) = duplex(32);
        let error = send_file(&mut sender, &mut &b"ab"[..], 1, None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("size limit"));
    }

    #[tokio::test(start_paused = true)]
    async fn stalled_transfer_times_out() {
        let (_sender, mut receiver) = duplex(128);
        let result = receive_file(&mut receiver, &mut Vec::new(), 10).await;
        assert!(result.is_err());
    }
}
