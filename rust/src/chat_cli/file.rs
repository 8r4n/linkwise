//! Linkwise gzip file records with FIPS-provider SHA-256.
use std::{
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::{Compression, bufread::GzDecoder, write::GzEncoder};
use tokio::time::Instant;

use super::codec;

const SUFFIX: &str = ".VarAC_gzip";
pub(super) const MAX_FILE: usize = 32 * 1024;
const MAX_GZIP: usize = MAX_FILE + 128;
const WAIT: Duration = Duration::from_secs(180);
pub(super) const MODEM_CHUNK: usize = 768;

const READY: &[u8] = b"<SFRD>";
const OK: &[u8] = b"<SFOK>";
const REJECTED: &[u8] = b"<SFAB>";

fn require_fips() -> io::Result<()> {
    #[cfg(feature = "secure-transfer")]
    return aws_lc_rs::try_fips_mode().map_err(io::Error::other);
    #[cfg(not(feature = "secure-transfer"))]
    Err(io::Error::other(
        "file transfer requires the secure-transfer build feature",
    ))
}

fn checksum(bytes: &[u8]) -> io::Result<[u8; 32]> {
    require_fips()?;
    #[cfg(feature = "secure-transfer")]
    {
        use aws_lc_rs::digest::{SHA256, digest};
        Ok(digest(&SHA256, bytes)
            .as_ref()
            .try_into()
            .expect("SHA-256 digest has 32 bytes"))
    }
    #[cfg(not(feature = "secure-transfer"))]
    {
        let _ = bytes;
        unreachable!("the feature check rejects this build before hashing")
    }
}

fn parse_checksum(hex: &str) -> io::Result<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(io::Error::other("file offer requires a SHA-256 checksum"));
    }
    let mut output = [0; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).map_err(io::Error::other)?;
    }
    Ok(output)
}

struct Offer {
    name: String,
    size: usize,
    digest: [u8; 32],
}

impl Offer {
    fn parse(record: &[u8]) -> io::Result<Self> {
        let text = std::str::from_utf8(record).map_err(io::Error::other)?;
        let [name, size, digest] = text.split('|').collect::<Vec<_>>()[..] else {
            return Err(io::Error::other("invalid file offer"));
        };
        let name = name
            .strip_suffix(SUFFIX)
            .ok_or_else(|| io::Error::other("unsupported VarAC file format"))?;
        validate_name(name)?;
        let size = size.parse::<usize>().map_err(io::Error::other)?;
        if !(1..=MAX_GZIP).contains(&size) {
            return Err(io::Error::other("file offer exceeds the limit"));
        }
        Ok(Self {
            name: name.into(),
            size,
            digest: parse_checksum(digest)?,
        })
    }

    fn record(&self) -> io::Result<Vec<u8>> {
        let digest = self
            .digest
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        codec::encode_record(
            format!("<SF:{}{SUFFIX}|{}|{digest}>", self.name, self.size).as_bytes(),
        )
    }

    fn save(&self, body: &[u8], path: &Path) -> io::Result<()> {
        let expected_base64 = self.size.div_ceil(3) * 4;
        if body.len() != expected_base64 {
            return Err(io::Error::other(
                "file Base64 length does not match the offer",
            ));
        }
        let bytes = STANDARD.decode(body).map_err(io::Error::other)?;
        if bytes.len() != self.size || checksum(&bytes)? != self.digest {
            return Err(io::Error::other(
                "compressed file size or checksum does not match the offer",
            ));
        }
        let mut gzip = GzDecoder::new(bytes.as_slice());
        let mut output = Vec::new();
        (&mut gzip)
            .take(MAX_FILE as u64 + 1)
            .read_to_end(&mut output)?;
        if output.len() > MAX_FILE || !gzip.get_ref().is_empty() {
            return Err(io::Error::other(
                "expanded file exceeds the limit or has trailing data",
            ));
        }
        let folder = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(folder)?;
        temp.write_all(&output)?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(path).map_err(|error| error.error)?;
        Ok(())
    }
}

fn validate_name(name: &str) -> io::Result<()> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if name.is_empty()
        || name.len() > 128
        || reserved
        || name.starts_with(['.', ' '])
        || name.ends_with(['.', ' '])
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" ._-".contains(&b))
    {
        return Err(io::Error::other(
            "file name must be a portable ASCII basename (letters, digits, spaces, dot, underscore, hyphen)",
        ));
    }
    Ok(())
}

enum Active {
    Outgoing { name: String, record: Vec<u8> },
    WaitingReceipt(String),
    Incoming(Offer),
    Receiving { offer: Offer, path: PathBuf },
}

#[derive(Default)]
pub(super) struct Transfer {
    active: Option<Active>,
    deadline: Option<Instant>,
    wire: Vec<u8>,
    offset: usize,
    abort_required: bool,
}

impl Transfer {
    pub fn busy(&self) -> bool {
        self.active.is_some() || self.offset < self.wire.len()
    }

    pub fn expired(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    pub fn progress(&mut self) {
        if self.busy() {
            self.deadline = Some(Instant::now() + WAIT);
        }
    }

    pub fn received_bytes(&mut self) {
        if matches!(self.active, Some(Active::Receiving { .. })) {
            self.progress();
        }
    }

    pub fn acknowledged(&mut self) {
        if !self.busy() {
            self.deadline = None;
            self.wire.clear();
            self.offset = 0;
        }
    }

    pub fn status(&self) -> String {
        match &self.active {
            Some(Active::Outgoing { name, .. }) => format!("File 1 awaiting approval: {name}"),
            Some(Active::WaitingReceipt(name)) => format!(
                "File 1: {name} | {} / {} record bytes queued, awaiting receipt",
                self.offset,
                self.wire.len()
            ),
            Some(Active::Incoming(offer)) => format!(
                "File 1 offered: {} ({} gzip bytes, SHA-256). /accept 1 or /reject 1",
                offer.name, offer.size
            ),
            Some(Active::Receiving { offer, .. }) => format!("Receiving file 1: {}", offer.name),
            None => "No active file transfer".into(),
        }
    }

    pub fn offer(&mut self, path: &Path) -> io::Result<String> {
        require_fips()?;
        if self.busy() {
            return Err(io::Error::other("a file transfer is already active"));
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| io::Error::other("missing file name"))?;
        validate_name(name)?;
        if !path.metadata()?.is_file() {
            return Err(io::Error::other("choose a regular file"));
        }
        let mut data = Vec::new();
        File::open(path)?
            .take(MAX_FILE as u64 + 1)
            .read_to_end(&mut data)?;
        if data.len() > MAX_FILE {
            return Err(io::Error::other("file exceeds the 32 KiB limit"));
        }
        let mut gzip = GzEncoder::new(Vec::new(), Compression::fast());
        gzip.write_all(&data)?;
        let bytes = gzip.finish()?;
        let offer = Offer {
            name: name.into(),
            size: bytes.len(),
            digest: checksum(&bytes)?,
        };
        if offer.size > MAX_GZIP {
            return Err(io::Error::other(
                "compressed file exceeds the wire size limit",
            ));
        }
        let record = codec::encode_record(format!("<SFB>{}", STANDARD.encode(&bytes)).as_bytes())?;
        self.queue(offer.record()?);
        self.active = Some(Active::Outgoing {
            name: offer.name,
            record,
        });
        self.progress();
        Ok(self.status())
    }

    pub fn accept(&mut self, destination: Option<&Path>) -> io::Result<String> {
        require_fips()?;
        let Some(Active::Incoming(offer)) = &self.active else {
            return Err(io::Error::other("there is no incoming file offer"));
        };
        let path = destination.unwrap_or(Path::new(&offer.name)).to_owned();
        match path.symlink_metadata() {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "destination already exists",
                ));
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            Err(_) => {}
        }
        self.queue(codec::encode_record(READY)?);
        let Some(Active::Incoming(offer)) = self.active.take() else {
            unreachable!()
        };
        self.active = Some(Active::Receiving { offer, path });
        self.progress();
        Ok(self.status())
    }

    pub fn reject(&mut self) -> io::Result<String> {
        if !matches!(self.active, Some(Active::Incoming(_))) {
            return Err(io::Error::other("there is no incoming file offer"));
        }
        self.active = None;
        self.queue(codec::encode_record(REJECTED)?);
        self.progress();
        Ok("File 1 rejected.".into())
    }

    pub fn cancel_at_record_boundary(&mut self) -> Option<String> {
        if self.active.is_none() || (self.offset != 0 && self.offset < self.wire.len()) {
            return None;
        }
        self.active = None;
        self.queue(codec::encode_record(REJECTED).expect("valid reply"));
        self.progress();
        Some("File 1 cancelled. Delivery may be unconfirmed.".into())
    }

    pub fn needs_abort(&self) -> bool {
        self.abort_required
    }

    fn queue(&mut self, wire: Vec<u8>) {
        self.wire = wire;
        self.offset = 0;
    }

    pub fn next_chunk(&mut self) -> Option<&[u8]> {
        if self.offset == self.wire.len() {
            return None;
        }
        let start = self.offset;
        self.offset = (self.offset + MODEM_CHUNK).min(self.wire.len());
        Some(&self.wire[start..self.offset])
    }

    pub fn receive(&mut self, record: &[u8]) -> Option<io::Result<String>> {
        if let Some(offer) = record
            .strip_prefix(b"<SF:")
            .and_then(|b| b.strip_suffix(b">"))
        {
            if self.busy() {
                return Some(Err(io::Error::other(
                    "ignored file offer while a transfer is active",
                )));
            }
            return Some(match require_fips().and_then(|()| Offer::parse(offer)) {
                Ok(offer) => {
                    self.active = Some(Active::Incoming(offer));
                    self.progress();
                    Ok(self.status())
                }
                Err(error) => {
                    self.queue(codec::encode_record(REJECTED).expect("valid reply"));
                    self.progress();
                    Err(error)
                }
            });
        }
        if record == REJECTED {
            if matches!(self.active, Some(Active::WaitingReceipt(_)))
                && self.offset > 0
                && self.offset < self.wire.len()
            {
                // The peer decoder is in the middle of the outer VarAC record.
                self.abort_required = true;
            } else {
                *self = Self::default();
            }
            return Some(Ok("Peer rejected or aborted file 1.".into()));
        }
        if record == READY
            && matches!(self.active, Some(Active::Outgoing { .. }))
            && self.offset == self.wire.len()
        {
            let Some(Active::Outgoing { name, record }) = self.active.take() else {
                unreachable!()
            };
            self.queue(record);
            self.active = Some(Active::WaitingReceipt(name));
            self.progress();
            return Some(Ok(self.status()));
        }
        if let Some(payload) = record.strip_prefix(b"<SFB>") {
            let Some(Active::Receiving { offer, path }) = self
                .active
                .take_if(|state| matches!(state, Active::Receiving { .. }))
            else {
                return Some(Err(io::Error::other(
                    "ignored a file body without an accepted offer",
                )));
            };
            self.deadline = None;
            let saved = offer.save(payload, &path);
            return Some(
                codec::encode_record(if saved.is_ok() { OK } else { REJECTED }).and_then(|reply| {
                    self.queue(reply);
                    self.progress();
                    saved.map(|()| format!("Received file 1: {}", path.display()))
                }),
            );
        }
        if record == OK
            && let Some(Active::WaitingReceipt(_)) = &self.active
            && self.offset == self.wire.len()
        {
            let Some(Active::WaitingReceipt(name)) = self.active.take() else {
                unreachable!()
            };
            self.deadline = None;
            return Some(Ok(format!("File 1 confirmed by peer: {name}")));
        }
        if matches!(record, READY | OK) {
            return Some(Err(io::Error::other("ignored an unexpected file reply")));
        }
        None
    }
}

#[cfg(all(test, feature = "secure-transfer"))]
mod tests {
    use super::*;

    const STOCK_OFFER: &[u8] = b"<SF:sample.bin.VarAC_gzip|28|9BCE18C5112051FF758CE44DD401AEA1>";
    const BODY: &[u8] = b"<SFB>H4sIAAAAAAAEAGNgZGKub/j3HwDyh2m8CAAAAA==";
    const SHA_OFFER: &[u8] = b"<SF:sample.bin.VarAC_gzip|28|9B64B1BDE671D6BBFF2FC9F8604B0FA67B13D4EFD63C161D7F73D4A07B68D8EA>";

    #[test]
    fn stock_md5_offer_is_declined_and_sha256_is_verified_before_publishing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("received.bin");
        let mut session = Transfer::default();
        assert!(session.receive(STOCK_OFFER).unwrap().is_err());
        assert_eq!(session.next_chunk(), Some(b"6 <SFAB>".as_slice()));
        session.acknowledged();
        session.receive(SHA_OFFER).unwrap().unwrap();
        session.accept(Some(&path)).unwrap();
        assert_eq!(session.next_chunk(), Some(b"6 <SFRD>".as_slice()));

        let mut wrong = BODY.to_vec();
        wrong[12] = b'Z';
        assert!(session.receive(&wrong).unwrap().is_err());
        assert!(!path.exists());
        assert_eq!(session.next_chunk(), Some(b"6 <SFAB>".as_slice()));
        session.acknowledged();

        session.receive(SHA_OFFER).unwrap().unwrap();
        session.accept(Some(&path)).unwrap();
        assert_eq!(session.next_chunk(), Some(b"6 <SFRD>".as_slice()));
        session.receive(BODY).unwrap().unwrap();
        assert_eq!(
            std::fs::read(path).unwrap(),
            [0, 1, 2, 3, 127, 128, 254, 255]
        );
        assert_eq!(session.next_chunk(), Some(b"6 <SFOK>".as_slice()));
        session.acknowledged();
        assert!(!session.expired(Instant::now() + WAIT));
    }

    #[test]
    fn rejects_nonportable_names_expansion_and_destination_collision() {
        for name in [
            "../file",
            "dir/file",
            r"dir\file",
            "C:file",
            "CON.txt",
            "notes.",
        ] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("file");
        let mut gzip = GzEncoder::new(Vec::new(), Compression::fast());
        gzip.write_all(&[0; MAX_FILE + 1]).unwrap();
        let bytes = gzip.finish().unwrap();
        let offer = Offer {
            name: "file".into(),
            size: bytes.len(),
            digest: checksum(&bytes).unwrap(),
        };
        assert!(
            offer
                .save(STANDARD.encode(&bytes).as_bytes(), &output)
                .is_err()
        );
        assert!(!output.exists());

        std::fs::write(&output, b"existing").unwrap();
        let offer = Offer::parse(&SHA_OFFER[4..SHA_OFFER.len() - 1]).unwrap();
        assert!(offer.save(&BODY[5..], &output).is_err());
        assert_eq!(std::fs::read(output).unwrap(), b"existing");
    }
}
