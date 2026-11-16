//! Validate and install public certificates exchanged during chat enrollment.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
};

use aws_lc_rs::digest::{SHA256, digest};

use super::*;

const OFFER_CONTEXT: &str = "linkwise/chat/certificate-offer/1";
const QSL_CONTEXT: &str = "linkwise/qsl/certificate-offer/1";
const MAX_CHAIN: usize = 8;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CertificateOffer {
    certificates: PublicCertificates,
    proof: Signed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PublicCertificates {
    signing: Vec<String>,
    encryption: Option<Vec<String>>,
}

pub(crate) struct InspectedOffer {
    previous: Option<PublicCertificates>,
    replacement: bool,
    pub details: Vec<String>,
}

fn decode_chain(encoded: &[String]) -> io::Result<Vec<CertificateDer<'static>>> {
    if !(1..=MAX_CHAIN).contains(&encoded.len()) {
        return Err(other(
            "certificate chain must contain one to eight certificates",
        ));
    }
    encoded
        .iter()
        .map(|der| {
            STANDARD
                .decode(der)
                .map(CertificateDer::from)
                .map_err(other)
        })
        .collect()
}

fn encode_chain(chain: Vec<CertificateDer<'static>>) -> Vec<String> {
    chain
        .iter()
        .map(|cert| STANDARD.encode(cert.as_ref()))
        .collect()
}

fn proof_payload(
    challenge: Option<&[u8; 32]>,
    certificates: &PublicCertificates,
) -> io::Result<Vec<u8>> {
    let hash = digest(&SHA256, &serde_json::to_vec(certificates)?);
    Ok([
        challenge.map_or(&[][..], |value| value.as_slice()),
        hash.as_ref(),
    ]
    .concat())
}

fn describe(label: &str, chain: &[String]) -> io::Result<String> {
    let chain = decode_chain(chain)?;
    let leaf = &chain[0];
    let parsed = Certificate::from_der(leaf.as_ref()).map_err(other)?;
    let issuer: String = parsed
        .tbs_certificate
        .issuer
        .to_string()
        .chars()
        .filter(|ch| !ch.is_control())
        .collect();
    let fingerprint = display_fingerprint(&sha256(leaf.as_ref()));
    Ok(format!(
        "{label} SHA-256 {fingerprint} (issuer {issuer}, expires {})",
        parsed.tbs_certificate.validity.not_after.to_date_time()
    ))
}

impl CertificateOffer {
    pub(crate) fn qsl_description(&self) -> io::Result<Vec<String>> {
        aws_lc_rs::try_fips_mode().map_err(other)?;
        if !valid_call(&self.proof.from) || !valid_call(&self.proof.to) {
            return Err(other("invalid station or recipient in QSL card"));
        }
        let mut details = vec![
            format!(
                "QSL card from {} to {} (unverified)",
                self.proof.from, self.proof.to
            ),
            describe("Signing certificate", &self.certificates.signing)?,
        ];
        if let Some(encryption) = &self.certificates.encryption {
            details.push(describe("Encryption certificate", encryption)?);
        }
        Ok(details)
    }

    pub(crate) fn includes_encryption(&self) -> bool {
        self.certificates.encryption.is_some()
    }
}

fn includes_replacement(
    previous: &Option<PublicCertificates>,
    offered: &PublicCertificates,
) -> bool {
    previous.as_ref().is_some_and(|old| {
        old.signing.first() != offered.signing.first()
            || matches!((&old.encryption, &offered.encryption), (Some(previous), Some(current)) if previous.first() != current.first())
    })
}

impl Pki {
    pub(super) fn read_chain(
        &self,
        source: &CertificateSource,
    ) -> io::Result<Vec<CertificateDer<'static>>> {
        match source {
            CertificateSource::File(path) => pem_objects(&self.directory.join(path)),
            CertificateSource::Inline(certificate) => decode_chain(&certificate.der),
        }
    }

    pub fn certificate_offer(
        &self,
        to: &str,
        challenge: &[u8; 32],
        include_encryption: bool,
    ) -> io::Result<CertificateOffer> {
        self.make_offer(to, Some(challenge), include_encryption)
    }

    pub(crate) fn qsl_offer(
        &self,
        to: &str,
        include_encryption: bool,
    ) -> io::Result<CertificateOffer> {
        if !valid_call(to) || to == self.callsign() {
            return Err(other(
                "enter the recipient's callsign, distinct from the local callsign",
            ));
        }
        self.make_offer(to, None, include_encryption)
    }

    fn make_offer(
        &self,
        to: &str,
        challenge: Option<&[u8; 32]>,
        include_encryption: bool,
    ) -> io::Result<CertificateOffer> {
        let signing = pem_objects(&self.directory.join(&self.profile.signing.certificate))?;
        self.validate_chain(&signing, self.callsign(), true)?;
        let encryption = if include_encryption {
            let settings = self
                .profile
                .encryption
                .as_ref()
                .ok_or_else(|| other("no public encryption certificate is configured"))?;
            let chain = pem_objects(&self.directory.join(&settings.credential.certificate))?;
            self.validate_chain(&chain, self.callsign(), false)?;
            Some(encode_chain(chain))
        } else {
            None
        };
        let certificates = PublicCertificates {
            signing: encode_chain(signing),
            encryption,
        };
        let context = challenge.map_or(QSL_CONTEXT, |_| OFFER_CONTEXT);
        let proof = self.sign_to(context, to, &proof_payload(challenge, &certificates)?)?;
        Ok(CertificateOffer {
            certificates,
            proof,
        })
    }

    pub fn verify_certificate_offer(
        &self,
        peer: &str,
        challenge: &[u8; 32],
        offer: &CertificateOffer,
    ) -> io::Result<()> {
        self.verify_offer(peer, Some(challenge), offer)
    }

    fn verify_offer(
        &self,
        peer: &str,
        challenge: Option<&[u8; 32]>,
        offer: &CertificateOffer,
    ) -> io::Result<()> {
        if !valid_call(peer) || peer == self.callsign() || offer.proof.from != peer {
            return Err(other("certificate offer is from the wrong station"));
        }
        let signing =
            self.validate_chain(&decode_chain(&offer.certificates.signing)?, peer, true)?;
        if let Some(chain) = &offer.certificates.encryption {
            self.validate_chain(&decode_chain(chain)?, peer, false)?;
        }
        let context = challenge.map_or(QSL_CONTEXT, |_| OFFER_CONTEXT);
        let actual = self.verify_with_certificate(context, &offer.proof, 64, &signing)?;
        if actual != proof_payload(challenge, &offer.certificates)? {
            return Err(other(if challenge.is_some() {
                "certificate offer does not bind this enrollment call"
            } else {
                "certificate offer does not bind the QSL card certificates"
            }));
        }
        Ok(())
    }

    pub fn inspect_certificate_offer(
        &self,
        peer: &str,
        challenge: &[u8; 32],
        offer: &CertificateOffer,
    ) -> io::Result<InspectedOffer> {
        self.verify_certificate_offer(peer, challenge, offer)?;
        let previous = self.installed_certificates(peer)?;
        let offered = &offer.certificates;
        let replacement = includes_replacement(&previous, offered);
        let mut details = vec![format!(
            "Certificate offer for {peer}. CA and signing-key proof verified."
        )];
        if let Some(old) = &previous {
            details.push(describe("Installed signing certificate", &old.signing)?);
            if let Some(encryption) = &old.encryption {
                details.push(describe("Installed encryption certificate", encryption)?);
            }
        }
        details.push(describe("Offered signing certificate", &offered.signing)?);
        if let Some(encryption) = &offered.encryption {
            details.push(describe("Offered encryption certificate", encryption)?);
        }
        details.push(if replacement {
            "Compare fingerprints out of band, then use /cert accept replace SIGNING [ENCRYPTION]."
                .into()
        } else {
            "Compare fingerprints out of band, then use /cert accept SIGNING [ENCRYPTION].".into()
        });
        Ok(InspectedOffer {
            previous,
            replacement,
            details,
        })
    }

    fn installed_certificates(&self, call: &str) -> io::Result<Option<PublicCertificates>> {
        self.profile
            .peers
            .get(call)
            .map(|peer| {
                Ok(PublicCertificates {
                    signing: encode_chain(self.read_chain(&peer.signing_certificate)?),
                    encryption: peer
                        .encryption_certificate
                        .as_ref()
                        .map(|source| self.read_chain(source).map(encode_chain))
                        .transpose()?,
                })
            })
            .transpose()
    }

    pub fn accept_certificate_offer(
        &mut self,
        peer: &str,
        challenge: &[u8; 32],
        offer: &CertificateOffer,
        inspected: &InspectedOffer,
        replace: bool,
        fingerprints: &[&str],
    ) -> io::Result<()> {
        if inspected.replacement != replace {
            return Err(other(if inspected.replacement {
                "this offer changes an installed certificate; use /cert accept replace SIGNING [ENCRYPTION]"
            } else {
                "this offer can be accepted with /cert accept SIGNING [ENCRYPTION]"
            }));
        }
        self.install_offer(
            peer,
            Some(challenge),
            offer,
            &inspected.previous,
            fingerprints,
        )
    }

    pub(crate) fn accept_qsl_offer(
        &mut self,
        peer: &str,
        offer: &CertificateOffer,
        replace: bool,
        fingerprints: &[&str],
    ) -> io::Result<()> {
        self.verify_offer(peer, None, offer)?;
        let previous = self.installed_certificates(peer)?;
        if includes_replacement(&previous, &offer.certificates) && !replace {
            return Err(other(
                "the card changes an installed certificate; verify the new fingerprints and use qsl import --replace",
            ));
        }
        self.install_offer(peer, None, offer, &previous, fingerprints)
    }

    fn install_offer(
        &mut self,
        peer: &str,
        challenge: Option<&[u8; 32]>,
        offer: &CertificateOffer,
        previous: &Option<PublicCertificates>,
        fingerprints: &[&str],
    ) -> io::Result<()> {
        let offered = &offer.certificates;
        let offered_chains = std::iter::once(&offered.signing).chain(offered.encryption.as_ref());
        if fingerprints.len() != 1 + usize::from(offered.encryption.is_some()) {
            return Err(other(
                "enter fingerprints from the trusted channel for every offered certificate",
            ));
        }
        for (entered, chain) in fingerprints.iter().zip(offered_chains) {
            let leaf = &decode_chain(chain)?[0];
            if parse_fingerprint(entered)? != sha256(leaf.as_ref()) {
                return Err(other(
                    "out-of-band fingerprint does not match the certificate offer",
                ));
            }
        }
        let mut lock_path = self.profile_path.clone().into_os_string();
        lock_path.push(".lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .map_err(|error| other(format!("could not lock PKI profile: {error}")))?;
        if fs::symlink_metadata(&self.profile_path)?
            .file_type()
            .is_symlink()
        {
            return Err(other("run enrollment with the PKI profile's real path"));
        }
        let mut current = Self::load(&self.profile_path)?;
        if current.callsign() != self.callsign() {
            return Err(other("PKI station changed; restart enrollment"));
        }
        current.verify_offer(peer, challenge, offer)?;
        if &current.installed_certificates(peer)? != previous {
            return Err(other(if challenge.is_some() {
                "installed certificates changed; reconnect and inspect the offer again"
            } else {
                "installed certificates changed during import; verify the current profile and retry"
            }));
        }

        if challenge.is_none()
            && previous.as_ref().is_some_and(|old| {
                old.signing == offered.signing
                    && (offered.encryption.is_none() || old.encryption == offered.encryption)
            })
        {
            *self = current;
            return Ok(());
        }
        let old = current.profile.peers.remove(peer);
        let require_encryption = old.as_ref().is_some_and(|entry| entry.require_encryption);
        let encryption_certificate = offered
            .encryption
            .as_ref()
            .map(|der| CertificateSource::Inline(InlineCertificate { der: der.clone() }))
            .or_else(|| old.and_then(|entry| entry.encryption_certificate));
        let updated = Peer {
            signing_certificate: CertificateSource::Inline(InlineCertificate {
                der: offered.signing.clone(),
            }),
            encryption_certificate,
            require_encryption,
        };
        if current.encryption_credentials_incomplete(&updated) {
            return Err(other(
                "the existing peer policy requires complete mail-encryption credentials",
            ));
        }
        current.profile.peers.insert(peer.to_owned(), updated);
        let content = serde_json::to_vec_pretty(&current.profile)?;
        if content.len() as u64 + 1 > CONFIG_LIMIT {
            return Err(other("updated PKI profile exceeds the size limit"));
        }
        let mut temporary = tempfile::NamedTempFile::new_in(
            current
                .profile_path
                .parent()
                .expect("absolute profile path"),
        )?;
        temporary.write_all(&content)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&current.profile_path)
            .map_err(|error| error.error)?;
        *self = current;
        Ok(())
    }
}
