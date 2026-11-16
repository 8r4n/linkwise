//! Printable public certificate postcards and offline import from phone photos.
mod card;

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Datelike, Utc};
use flate2::{Compression, bufread::GzDecoder, write::GzEncoder};
use image::{ImageFormat, ImageReader, Limits};
use qrcode::{EcLevel, QrCode};

use self::card::{CardSize, Contact};
use crate::pki::{CertificateOffer, Pki};

pub(crate) const USAGE: &str = "Usage:
  linkwise qsl card PROFILE TO PDF --utc TIMESTAMP --freq-khz KHZ --mode MODE --rst RST
                   [--grid GRID] [--notes TEXT] [--size usps|4x6] [--signing-only]
  linkwise qsl inspect PHOTO
  linkwise qsl import PROFILE FROM PHOTO SIGNING_SHA256 [ENCRYPTION_SHA256] [--replace]
  linkwise qsl receive PROFILE FROM DIRECTORY SIGNING_SHA256 [ENCRYPTION_SHA256]

Use a JPEG or PNG photo, or a text file containing the scanned QR payload.
Receive watches for a newly shared file and exits after one validated import.";
const MARKER: &str = "LINKWISE-QSL1:";
const SCAN_LIMIT: u64 = 24 * 1024 * 1024;
const OFFER_LIMIT: u64 = 16 * 1024;
const MAX_QR_TEXT: usize = 2331; // Version 40, error correction M, byte mode.

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn option_value<'a>(options: &'a BTreeMap<&str, &str>, name: &str) -> io::Result<&'a str> {
    options
        .get(name)
        .copied()
        .ok_or_else(|| invalid(format!("missing {name}")))
}

fn print_field(value: &str, name: &str, max: usize) -> io::Result<String> {
    if value.trim().is_empty()
        || value.len() > max
        || !value.bytes().all(|byte| (b' '..=b'~').contains(&byte))
    {
        return Err(invalid(format!(
            "{name} requires 1 to {max} printable ASCII characters"
        )));
    }
    Ok(value.to_owned())
}

fn parse_contact(args: &[OsString]) -> io::Result<(Contact, CardSize, bool)> {
    let mut options = BTreeMap::new();
    let mut signing_only = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.to_str() {
            Some("--signing-only") if !signing_only => signing_only = true,
            Some(
                key @ ("--utc" | "--freq-khz" | "--mode" | "--rst" | "--grid" | "--notes"
                | "--size"),
            ) => {
                let value = iter
                    .next()
                    .and_then(|arg| arg.to_str())
                    .ok_or_else(|| invalid(format!("missing value for {key}")))?;
                if options.insert(key, value).is_some() {
                    return Err(invalid(format!("duplicate {key}")));
                }
            }
            _ => return Err(invalid(USAGE)),
        }
    }
    let utc = DateTime::parse_from_rfc3339(option_value(&options, "--utc")?)
        .map_err(|_| invalid("--utc requires an RFC 3339 timestamp such as 2026-11-15T19:30:00Z"))?
        .with_timezone(&Utc);
    if !(1900..=9999).contains(&utc.year()) {
        return Err(invalid("--utc requires a year between 1900 and 9999"));
    }
    let frequency = option_value(&options, "--freq-khz")?;
    if frequency.len() > 10
        || !frequency
            .bytes()
            .all(|ch| ch.is_ascii_digit() || ch == b'.')
        || !frequency
            .parse::<f64>()
            .is_ok_and(|value| value > 0. && value < 1_000_000_000.)
    {
        return Err(invalid(
            "--freq-khz requires a positive decimal frequency in kHz",
        ));
    }
    let mode = print_field(option_value(&options, "--mode")?, "--mode", 8)?;
    let rst = option_value(&options, "--rst")?;
    if !(2..=3).contains(&rst.len()) || !rst.bytes().all(|ch| (b'1'..=b'9').contains(&ch)) {
        return Err(invalid(
            "--rst requires a two- or three-digit signal report",
        ));
    }
    if rst.as_bytes()[0] > b'5' {
        return Err(invalid("RST readability must be between 1 and 5"));
    }
    let grid = options
        .get("--grid")
        .map(|value| {
            let grid = value.to_ascii_uppercase();
            match grid.as_bytes() {
                [b'A'..=b'R', b'A'..=b'R', b'0'..=b'9', b'0'..=b'9']
                | [
                    b'A'..=b'R',
                    b'A'..=b'R',
                    b'0'..=b'9',
                    b'0'..=b'9',
                    b'A'..=b'X',
                    b'A'..=b'X',
                ] => Ok(grid),
                _ => Err(invalid(
                    "--grid requires a four- or six-character Maidenhead locator",
                )),
            }
        })
        .transpose()?;
    let notes = options
        .get("--notes")
        .map(|note| print_field(note, "--notes", 45))
        .transpose()?;
    let size = match options.get("--size").copied() {
        None | Some("usps") => CardSize::Usps,
        Some("4x6") => CardSize::FourBySix,
        _ => return Err(invalid("--size must be usps or 4x6")),
    };
    let contact = Contact {
        utc,
        frequency_khz: frequency.into(),
        mode,
        rst: rst.into(),
        grid,
        notes,
    };
    Ok((contact, size, signing_only))
}

fn encode(offer: &CertificateOffer) -> io::Result<QrCode> {
    let plaintext = serde_json::to_vec(offer)?;
    if plaintext.len() as u64 > OFFER_LIMIT {
        return Err(invalid(
            "public certificate chains are too large for a postcard QR code",
        ));
    }
    let mut gzip = GzEncoder::new(Vec::new(), Compression::best());
    gzip.write_all(&plaintext)?;
    let text = format!("{MARKER}{}", URL_SAFE_NO_PAD.encode(gzip.finish()?));
    QrCode::with_error_correction_level(text, EcLevel::M)
        .map_err(|_| invalid("public certificate chains are too large for one postcard QR code"))
}

fn decode(text: &str) -> io::Result<CertificateOffer> {
    let text = text.trim();
    if text.len() > MAX_QR_TEXT {
        return Err(invalid("QSL code is too large"));
    }
    let payload = text
        .strip_prefix(MARKER)
        .ok_or_else(|| invalid("expected a Linkwise QSL QR code"))?;
    let compressed = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| invalid("invalid QSL QR encoding"))?;
    let mut gzip = GzDecoder::new(compressed.as_slice());
    let mut json = Vec::new();
    (&mut gzip).take(OFFER_LIMIT + 1).read_to_end(&mut json)?;
    if json.len() as u64 > OFFER_LIMIT || !gzip.get_ref().is_empty() {
        return Err(invalid("QSL QR contains an oversized or trailing payload"));
    }
    serde_json::from_slice(&json).map_err(|_| invalid("invalid QSL certificate offer"))
}

fn scan(path: &Path) -> io::Result<CertificateOffer> {
    let bytes = crate::pki::read_bounded(path, SCAN_LIMIT)?;
    if let Ok(text) = std::str::from_utf8(&bytes) {
        return decode(text);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if !matches!(reader.format(), Some(ImageFormat::Png | ImageFormat::Jpeg)) {
        return Err(invalid(
            "share a JPEG or PNG image, or a text file with the scanned QR code",
        ));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(8000);
    limits.max_image_height = Some(8000);
    limits.max_alloc = Some(160 * 1024 * 1024);
    reader.limits(limits);
    let image =
        image::DynamicImage::ImageLuma8(reader.decode().map_err(io::Error::other)?.into_luma8());
    let mut previous = 0;
    // Finder detection depends on how the camera samples each printed module.
    // Try a few pixel pitches before reporting that the photo is unreadable.
    for cap in [2300, 1500, 1100, 850] {
        let size = cap.min(image.width().max(image.height()));
        if previous == size {
            continue;
        }
        previous = size;
        let gray = image.thumbnail(size, size).into_luma8();
        let mut prepared = rqrr::PreparedImage::prepare(gray);
        let mut codes = prepared.detect_grids().into_iter().filter_map(|grid| {
            let (_, text) = grid.decode().ok()?;
            text.starts_with(MARKER).then_some(text)
        });
        if let Some(text) = codes.next() {
            if codes.next().is_some() {
                return Err(invalid("share a photo of one Linkwise QSL card at a time"));
            }
            return decode(&text);
        }
    }
    Err(invalid("no readable Linkwise QSL QR code in photo"))
}

fn generate(profile: &Path, to: &str, output: &Path, options: &[OsString]) -> io::Result<()> {
    let (contact, size, signing_only) = parse_contact(options)?;
    let pki = Pki::load(profile)?;
    let offer = pki.qsl_offer(to, !signing_only)?;
    let qr = encode(&offer)?;
    let pdf = card::render(
        size,
        pki.callsign(),
        to,
        &contact,
        offer.includes_encryption(),
        &qr,
    );
    let directory = output
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(&pdf)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)
        .map_err(|error| error.error)?;
    println!("Wrote QSL card for {to} to {}", output.display());
    Ok(())
}

fn import(
    profile: &Path,
    from: &str,
    photo: &Path,
    replace: bool,
    fingerprints: &[&str],
) -> io::Result<()> {
    let offer = scan(photo)?;
    Pki::load(profile)?.accept_qsl_offer(from, &offer, replace, fingerprints)?;
    let roles = if offer.includes_encryption() {
        "signing and mail-encryption"
    } else {
        "signing"
    };
    println!(
        "Installed verified {roles} certificates for {from} in {}",
        profile.display()
    );
    Ok(())
}

type FileState = (u64, SystemTime);

fn photos(dir: &Path) -> io::Result<BTreeMap<PathBuf, FileState>> {
    let mut files = BTreeMap::new();
    for (index, entry) in fs::read_dir(dir)?.enumerate() {
        if index >= 1000 {
            return Err(invalid("use a receive directory with at most 1000 entries"));
        }
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if kind.is_file()
            && ["jpg", "jpeg", "png", "txt"]
                .iter()
                .any(|format| ext.eq_ignore_ascii_case(format))
        {
            let meta = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            files.insert(path, (meta.len(), meta.modified()?));
        }
    }
    Ok(files)
}

fn receive(profile: &Path, from: &str, directory: &Path, fingerprints: &[&str]) -> io::Result<()> {
    let local = Pki::load(profile)?;
    if from == local.callsign() {
        return Err(invalid("the expected sender must be a different station"));
    }
    let starting = photos(directory)?;
    let mut seen = BTreeMap::new();
    let mut attempted = BTreeMap::new();
    println!(
        "Waiting for a new JPEG, PNG or QR text file in {}",
        directory.display()
    );
    loop {
        thread::sleep(Duration::from_millis(700));
        let current = photos(directory)?;
        attempted.retain(|path, _| current.contains_key(path));
        for (path, state) in &current {
            if starting.get(path) == Some(state)
                || seen.get(path) != Some(state)
                || attempted.get(path) == Some(state)
            {
                continue;
            }
            match import(profile, from, path, false, fingerprints) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    eprintln!("QSL input rejected: {error}");
                    attempted.insert(path.clone(), *state);
                }
            }
        }
        seen = current;
    }
}

pub(crate) fn run(args: &[OsString]) -> io::Result<()> {
    match args {
        [] => Err(invalid(USAGE)),
        [help] if help == "--help" || help == "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        [command, profile, to, output, options @ ..] if command == "card" => {
            let to = to
                .to_str()
                .ok_or_else(|| invalid("recipient must be a callsign"))?;
            generate(Path::new(profile), to, Path::new(output), options)
        }
        [command, photo] if command == "inspect" => {
            for line in scan(Path::new(photo))?.qsl_description()? {
                println!("{line}");
            }
            Ok(())
        }
        [command, profile, from, input, rest @ ..]
            if command == "import" || command == "receive" =>
        {
            let from = from
                .to_str()
                .filter(|call| crate::pki::valid_call(call))
                .ok_or_else(|| invalid("sender must be a callsign"))?;
            let replace = command == "import" && rest.last().is_some_and(|arg| arg == "--replace");
            let values = if replace {
                &rest[..rest.len() - 1]
            } else {
                rest
            };
            if !(1..=2).contains(&values.len()) {
                return Err(invalid(USAGE));
            }
            let fingerprints = values
                .iter()
                .map(|arg| {
                    let value = arg
                        .to_str()
                        .ok_or_else(|| invalid("enter complete SHA-256 fingerprints"))?;
                    crate::pki::parse_fingerprint(value)?;
                    Ok::<_, io::Error>(value)
                })
                .collect::<io::Result<Vec<_>>>()?;
            if command == "receive" {
                receive(Path::new(profile), from, Path::new(input), &fingerprints)
            } else {
                import(
                    Path::new(profile),
                    from,
                    Path::new(input),
                    replace,
                    &fingerprints,
                )
            }
        }
        _ => Err(invalid(USAGE)),
    }
}
