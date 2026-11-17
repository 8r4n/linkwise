# Station identity for chat and radio mail

A Linkwise station can use one X.509 profile to authenticate connected BW500
chat and sign radio email. Signed mode is the default when you pass
`--pki`. Encrypted mode adds confidentiality and keeps the same signature
checks. Protected chat requires Linkwise at both ends. Protected mail can
travel through the existing B2F exchange as an opaque attachment.

## Build and provision

Build with `--features secure-transfer` (also enable `audio-device` for
hardware audio). The signing and verification provider must report AWS-LC
FIPS mode. Encrypted chat uses the same provider through rustls. Encrypted
radio mail requires OpenSSL 3 with its FIPS provider and AES-GCM CMS
support. See the
[secure transfer guide](secure-transfer.md) for build prerequisites.

Provision stations through a CA that verifies each operator's claim to the
callsign. Distribute trust anchors and peer certificates over an authenticated
channel. For each station, issue an EC P-256 signing certificate with named
curve parameters, a `digitalSignature` key usage, and the exact DNS subject
alternative name `<lowercase-callsign>.radio.linkwise.invalid`. If you add
extended key usage, include `emailProtection`. Use a separate P-256 identity
certificate with `keyAgreement` for store-and-forward mail encryption.
Live chat authenticates its ephemeral TLS key exchange with the signing
certificate. CA signatures in each chain
must use ECDSA with SHA-256. Include any intermediates after the leaf in the
station certificate PEM, in issuer order. Supply current PEM CRLs covering
the certificate chains. Linkwise rejects unknown, expired, or revoked
certificate status. Chat reloads CRL files at connection and every 30
seconds during a call. Replace a CRL file atomically. Restart chat after
changing trust anchors or the list of CRL paths.

Store the signing key as PKCS#8 PEM. Use an owner-only directory and restrict
private key files to the station account. This example opts in to encryption
for mail and chat. Omit the entire `encryption` block and peer encryption
certificate for signing-only use. Relative paths are resolved next to the
profile. The OpenSSL executable path must be absolute.

~~~json
{
  "version": 1,
  "callsign": "AA1AA",
  "signing": {
    "certificate": "AA1AA-sign.pem",
    "private_key": "AA1AA-sign-key.pem"
  },
  "authorities": ["station-ca.pem"],
  "allow_chat_encryption": true,
  "revocation_lists": ["station-ca.crl.pem"],
  "peers": {
    "BB2BB": {
      "signing_certificate": "BB2BB-sign.pem",
      "encryption_certificate": "BB2BB-encrypt.pem",
      "require_encryption": false
    }
  },
  "encryption": {
    "openssl": "/usr/bin/openssl",
    "openssl_config": "openssl-fips.cnf",
    "credential": {
      "certificate": "AA1AA-encrypt.pem",
      "private_key": "AA1AA-encrypt-key.pem"
    },
    "allow_mail": true
  }
}
~~~

Run `linkwise pki check station-pki.json` on each station. The check
validates certificates and tests the local keys and configured providers. The station callsign
must match the identity configured in the modem.

When a profile trusts more than one CA key, add a `peer_authorities` object
mapping each peer callsign to its authorized CA key fingerprint. Add this
entry before enrollment. For a single configured CA key, Linkwise uses that
key when a peer has no explicit pin. Hash the CA's DER SubjectPublicKeyInfo.

~~~sh
linkwise pki fingerprint ca peer-ca.pem
linkwise pki fingerprint cert AA1AA-sign.pem
~~~

The first command prints the CA key fingerprint for `peer_authorities`.
The second prints the fingerprint of an entire station certificate for
out-of-band confirmation.

## Exchange certificates in chat

For first contact or certificate renewal, launch both stations with
`linkwise chat --pki station-pki.json --enroll AA1AA` using their own
callsigns. The receiver enters `/listen on` and the caller enters
`/connect PEERCALL`. Wait for `Enrollment ready`.

1. Enter `/cert offer` to send your signing certificate, or
   `/cert offer --encrypt` to include your encryption certificate.
2. When the peer's offer arrives, enter `/cert inspect`. Linkwise checks
   the certificate chains, roles, callsign, revocation, and a fresh proof
   of signing-key possession. Compare the displayed SHA-256 fingerprints
   over a channel you already trust.
3. Enter `/cert accept SIGNING [ENCRYPTION]` with the complete fingerprints
   you obtained over the trusted channel. Use `/cert accept replace SIGNING
   [ENCRYPTION]` if the offer changes an installed certificate. Include the
   encryption fingerprint when the offer includes that certificate.
   Accepted certificates are saved inline in the shared profile, so mail
   can use them as well. A signing-only offer preserves an existing
   encryption certificate.
4. After both operators finish, disconnect and relaunch with `--pki FILE`
   for regular verified chat.

Each operator can run a separate CA and issue their own station certificates.
Each station configures both trusted public CA certificates and their
current signed revocation lists. Verify each CA's identity out of band.
An enrollment call carries public certificates and signing proofs, with
up to four incoming offers per call and a 4,096-byte total limit per offer.
Keep the CA private keys offline and the station keys local.

## Mail a QSL card with public certificates

Use the same secure-transfer build and station profile as chat and radio
mail. The card confirms a contact and carries the sender's public signing
and mail-encryption certificate chains. The signing certificate also
authenticates the peer in encrypted chat. The QR holds the chains and a
signature binding them to the sender and intended recipient. Private keys
stay on the sender's computer.

~~~sh
linkwise qsl card station-pki.json BB2BB qsl-BB2BB.pdf \
  --utc 2026-11-15T19:30:00Z --freq-khz 7105 --mode BW500 --rst 59 \
  --grid CM87 --notes 'Thanks for the QSO.'
~~~

Match the printer's paper size to the PDF and print at **Actual Size
(100%)**, landscape, on the blank side of the postcard. The default is
**5½ × 3½ inches**, the size of the current
[USPS Schooner stamped card](https://store.usps.com/store/product/cards-envelopes/schooner-stamped-cards-S_250910).
Add `--size 4x6` for landscape 6 × 4-inch stock. Use the other side for
the address and postage. Add `--signing-only` to send only the signing
chain, including for a station that uses encrypted chat without a
separate mail-encryption certificate. For a chain too large for one
postcard QR symbol, use chat enrollment or exchange the chains through a
trusted channel.

Before receiving a card, add the sender's CA and current CRLs to the local
profile. Set its per-callsign CA pin if the profile trusts multiple CA
keys. Compare the full SHA-256 fingerprints with the sender over an
independent authenticated channel. The sender gets them with
`linkwise pki fingerprint cert CERT.pem`, once for signing and once for
encryption when included. Linkwise checks those fingerprints, the
intended recipient, the sender, the signature, the certificate roles,
the chain, and revocation before installing the peer.

Start the receiver with a directory where the phone will share the
photo. `SIGNING_SHA256` and `ENCRYPTION_SHA256` below stand for those
full fingerprints in that order.

~~~sh
linkwise qsl receive station-pki.json AA1AA ~/Downloads \
  SIGNING_SHA256 ENCRYPTION_SHA256
~~~

With the phone camera, photograph the QR in good light so it fills most
of the image and its white border is visible. Share the photo to the
watched directory, for example through AirDrop on a Mac or a synced Files
folder. Use JPEG or PNG. For an iPhone photo, select **Most Compatible**
when sharing if the original is HEIC. A file containing the QR text
copied from a scanner also works. Linkwise waits for a new or changed
file, validates it locally, adds the certificates to the shared profile,
and exits after one successful import. Omit the encryption fingerprint
for a signing-only card. Stop waiting with Ctrl-C.

For a saved photo, inspect or import it directly.

~~~sh
linkwise qsl inspect qsl-photo.jpg
linkwise qsl import station-pki.json AA1AA qsl-photo.jpg \
  SIGNING_SHA256 ENCRYPTION_SHA256
~~~

Inspection displays the offered fingerprints before trust checks.
`qsl receive` preserves an existing peer's leaf certificates when the
card offers changed ones. For an authorized rotation, independently
confirm the new fingerprints and use `qsl import ... --replace`.
Restart open chat clients to load changes. The new peer is available to
subsequent protected mail commands through the same profile.

The QR text begins with `LINKWISE-QSL1:` followed by unpadded URL-safe
Base64 of a gzip-compressed public certificate offer. The offer contains
the DER chains and a signature over their SHA-256 digest in the
`linkwise/qsl/certificate-offer/1` context, addressed to the receiving
callsign. The decoder bounds the image and compressed payload. The
postcard can be scanned offline. It does not install trust anchors, CRLs,
or private keys.

## Chat and message evidence

~~~sh
linkwise chat --pki station-pki.json AA1AA
linkwise chat --pki station-pki.json --encrypt AA1AA
~~~

The receiver enters `/listen on`, and the caller enters `/connect BB2BB`.
Start both clients in the same protection mode. A peer setting of
`require_encryption: true` selects encryption for that peer and enforces it
for protected mail. Each client displays `Verified peer` after checking a
fresh signed challenge. The verified status identifies the certificate
holder's station key. Chat verifies every message against the current
session and the next expected sequence number before display. The callsign
prefix reserves nine columns for the BW500 callsign, including an optional
SSID such as `-15`. A single status field shows `|V` for signed plaintext
or `|E` for signed and encrypted chat. On incoming messages, either
code indicates a verified signature. A blank slot indicates ordinary chat.
`|X` marks failed verification and `|F` marks an encryption or decryption
failure, including a protection-mode mismatch. Rejected incoming messages
are hidden and the call ends. If encrypting outgoing text fails, the draft
stays in the input field. The signed plaintext payload uses public Base64
encoding and can be read by anyone who receives it.

To upgrade an active signed call, enter `/encrypt` in the peer window. The
other operator sees the request and enters `/encrypt` to accept. Both clients
must support the command and have chat encryption enabled in their profiles.
Each client holds outgoing text as a draft while the upgrade is pending.
The clients exchange signed readiness records, then open mutually
authenticated TLS 1.3 over the existing BW500 DATA stream. TLS exporters
bind the encrypted channel to the current signed call.
Once the client displays `Encryption enabled.`, press Enter to send a held
draft. The call remains encrypted until disconnect. A failed or timed-out
upgrade ends the call and leaves unsent text in the draft.

In the peer window, `/proof message-proof.json` saves the latest verified
incoming message as a private file. An existing file is preserved. This action
also exports the plaintext of an encrypted message. The client retains one
latest proof in memory across peer windows. Verify an export using the
recipient's current station profile.

~~~sh
linkwise pki verify-chat station-pki.json message-proof.json
~~~

The JSON output identifies the signing callsign and recipient and includes the
signed session identifiers, sequence number, and text. This is evidence of
what the station key signed. It does not establish who was at the keyboard,
the signing time, or whether the recipient read the message. Linkwise does
not generate signed delivery receipts or trusted timestamps.

## Radio email

~~~sh
linkwise mail import --pki station-pki.json mailbox message.eml
linkwise mail import --pki station-pki.json --encrypt mailbox message.eml
linkwise mail export --pki station-pki.json mailbox MESSAGE_ID > verified.eml
~~~

Use `mail queue` with the same switches for an existing B2F envelope.
Protected mail addresses exactly one callsign with no Cc. It signs the
inner envelope, including the MIME content and attachments after B2F
conversion. The final B2F envelope must fit the 1 MiB mailbox limit.
`mail exchange` and `mail listen` transfer it through the normal queue.
The outer callsigns and message ID remain visible for routing. Export
checks the protection before writing any MIME output. Add `--encrypt`
to export when you require encryption for that message. The local SMTP
and IMAP listeners do not use this profile.

On the hardened Rocky Live USB, start Mutt with
`linkwise-mutt --pki station-pki.json YOURCALL`. Press `m` to compose
and `y` to send to one `PEERCALL@winlink.org` recipient. Mutt sends the
message directly to `linkwise mail submit --pki FILE MAILBOX RECIPIENT`
on standard input. This command always signs and encrypts before queuing.
Mutt displays a send error if the profile or recipient certificate cannot
be used. Run the normal B2F exchange from another console.

## Cryptographic profile

Linkwise uses ECDSA P-256 with SHA-256 via AWS-LC FIPS for signatures.
The signed input starts with the ASCII label `LINKWISE-PKI` followed by
a NUL. It then contains the context label, sender, recipient and payload,
each preceded by an unsigned 64-bit big-endian byte length. Chat uses
separate context labels for the handshake and messages. It frames records
with the regular VarAC length prefix and a reserved `~LWP1~` application
marker. Protected chat text shares the modem's 4,096-byte queued-record
limit. Radio mail uses the context `linkwise/mail/1` and places the signed
envelope in the B2F attachment `linkwise-pki-v1.bin`.

Encrypted chat uses TLS 1.3 with ephemeral P-256 ECDHE and
`TLS_AES_256_GCM_SHA384`. The channel authenticates both pinned signing
certificates. Resumption and 0-RTT are disabled. A later compromise of
the signing key does not decrypt a recorded chat if the session keys
were erased. Callsigns, timing and record sizes remain visible.

Mail encryption uses CMS AuthEnvelopedData with AES-256-GCM, 96-bit nonces,
128-bit tags, P-256 ECDH with SHA-256 for key derivation, and AES-256 key
wrap. The configured OpenSSL process uses `fips=yes`. Linkwise enforces
the CMS algorithm profile and does not fall back to unsigned mode.
Mail data is passed through bounded pipes with a 30-second OpenSSL deadline.
Stored mail remains decryptable by the recipient's long-term encryption
key. The dedicated gzip/SHA-256 encrypted
[file transfer](chat.md#transfer-a-file) uses its own TLS configuration.

### Require hybrid post-quantum chat

Build with `--features pqc-chat` against OpenSSL 3.5.9 and install its
FIPS provider according to the [CMVP #5586 security policy][openssl-fips].
Point `OPENSSL_CONF` to the approved provider configuration with
`fips=yes` and security checks active. Set these fields on both peers:

~~~json
{
  "allow_chat_encryption": true,
  "chat_key_exchange": "p256_mlkem768"
}
~~~

Run `linkwise pki check FILE` before connecting. Linkwise checks the
OpenSSL library and FIPS provider versions, verifies approved-mode
configuration and runs the provider self-tests. The TLS connection
requires `SecP256r1MLKEM768` and `TLS_AES_256_GCM_SHA384` and verifies
the negotiated parameters before sending chat text. A peer configured
for classical-only TLS cannot complete this handshake. The terminal
reports `TLS 1.3 with P-256 + ML-KEM-768` when the channel is ready.

The hybrid group protects session key establishment. Station identity
uses ECDSA certificates. This setting applies to live chat.
Use the security policy to qualify the installed provider and operating
environment for deployments with CMVP requirements.

[openssl-fips]: https://csrc.nist.gov/projects/cryptographic-module-validation-program/certificate/5586

Allow encryption only on transports where you are authorized to use it.
Both application switches default to false. For U.S. amateur operation,
see [47 CFR § 97.113(a)(4)](https://www.ecfr.gov/current/title-47/chapter-I/subchapter-D/part-97/subpart-B/section-97.113).
For deployments that require FIPS 140 validation, match the exact module,
build and operating environment against the applicable
[NIST CMVP certificate and security policy](https://csrc.nist.gov/projects/cryptographic-module-validation-program/validated-modules).

## Qualification

`tools/test-pki.py` provisions disposable identities and runs mail with an
attachment, tampering and revocation checks, and a two-station virtual
BW500 chat. Pass `--openssl-fips` and `--openssl-config` to exercise the
encrypted profile. Run it without those options for signed plaintext.

`tools/test-qsl.py` generates and renders both postcard sizes, verifies
automatic receipt from a JPEG, checks rejection for a wrong recipient,
fingerprint, or missing CA pin, and exchanges encrypted mail using the
installed certificates. It needs the secure-transfer build, OpenSSL,
Poppler's `pdftoppm` and `pdfinfo`, and the FIPS provider configuration
required for the protected mail test.

~~~sh
python3 tools/test-qsl.py rust/target/debug/linkwise \
  --openssl /path/to/openssl --openssl-config /path/to/openssl-fips.cnf
~~~

~~~sh
python3 tools/test-pki.py rust/target/debug/linkwise \
  rust/target/debug/linkwise-modemd \
  reference/vara/fixtures/level4/bw500-level4.json
~~~

The chat bench uses virtual audio and exercises proof export and verification,
including rejection of a modified proof. It requires no RF transmission.
