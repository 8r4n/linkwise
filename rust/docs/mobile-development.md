# Mobile modem development

The Rust library implements Winlink B2F envelopes, FC proposals and FS
replies, LZHUF compression, and binary transfer records. Incoming
transfers verify the suffix checksum, compression CRC, declared lengths,
and message ID before returning a complete envelope. The receive API
can join a saved compressed prefix to a newly received suffix.

The codec was cross-checked in both directions against the B2 reader and
writer in [wl2k-go v1.0.1](https://github.com/la5nta/wl2k-go/tree/v1.0.1/lzhuf).
The Rust tests keep an independent compressed fixture with a binary
attachment. The wire format is described in the
[Open B2F specification](https://winlink.org/B2F).

## Toolchains

Install full Xcode on a Mac, select its developer directory, complete its
first launch setup, and install an iOS Simulator runtime in Xcode settings.
For Android, install Android Studio, platform tools, the Android Emulator,
a matching ARM64 system image on Apple silicon, and the side-by-side NDK.
Create and boot an Android virtual device. Budget host storage for both
SDKs, simulator images, and build outputs.

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-linux-android
tools/mobile-preflight.sh
cargo test --locked --manifest-path rust/Cargo.toml --lib winlink::
for target in aarch64-apple-ios aarch64-apple-ios-sim aarch64-linux-android; do
    cargo check --locked --manifest-path rust/Cargo.toml --lib --target "$target" || break
done
```

The preflight checks a native GNU Radio installation by default. After
building the bench image below, run `tools/mobile-preflight.sh --container`
to check its GNU Radio installation instead. Cargo target checks establish Rust type checking.
App linking, simulator launch, and hardware tests are separate checks.

The ARM64 Android API 36 emulator passed all 78 Rust library tests,
including bidirectional mailbox exchange and retry after a lost
acknowledgment. The Android NDK builds of `linkwise` and
`linkwise-modemd` also ran on the emulator. The CLI imported an EML
message into its outbox, and the modem opened all three virtual BW500
ports. These checks use Android's shell and loopback sockets. Native
audio and mail app integration require separate tests.

## GNU Radio HF bench

Build the bench and check sample count, transparent pass-through, frequency
shift, seeded noise repeatability, and measured SNR.

```sh
docker build --tag linkwise-hf-bench containers/hf-bench
docker run --rm --volume "$PWD:/work:ro" --workdir /work linkwise-hf-bench \
    /usr/bin/python3 tools/test-hf-channel.py
```

On Linux with the Docker image above, add two loopback Linkwise BW500
stations. The transparent relay runs in both directions. The test asserts
delivery of queued bytes and a disconnect event at each station. Add
`--link-snr-db 40` to run an impairment experiment. The ARM64 Docker
bench timed out in the BW500 handshake at that setting, so the CI link
check uses the transparent setting.

```sh
cargo build --locked --manifest-path rust/Cargo.toml --bin linkwise-modemd
python3 tools/fetch-modem-profile.py
docker run --rm --volume "$PWD:/work:ro" --workdir /work linkwise-hf-bench \
    /usr/bin/python3 tools/test-hf-channel.py \
    --modem rust/target/debug/linkwise-modemd \
    --profile reference/vara/fixtures/level4/bw500-level4.json
```

Run `tools/hf-channel.py --help` for offline capture processing. Input
and output are mono little-endian float32 samples, at 48 kHz by default.
SNR is relative to RMS over the entire input capture. Seed and channel
settings are printed as JSON for the test record. The current model
covers additive Gaussian noise and a fixed frequency offset. Fading,
multipath, and time-varying Doppler require additional channel models.

## P2P forwarding and mailbox

The Rust P2P session runs over a caller-supplied buffered byte stream.
Set transport deadlines before calling it and close or disconnect the
transport on return. Both roles support SID negotiation, five-message
proposal batches, turn changes and FQ termination. Outbound messages
remain queued until an explicit rejection or a valid next turn. The
directory mailbox publishes complete files atomically and syncs files
and directories on Unix. Windows syncs files; directory durability
under sudden power loss has yet to be qualified. Use a private local
filesystem and serialize sessions per mailbox. Android requires support
for atomic rename without replacement. Other platforms use hard links
to publish complete files.
The directory outbox offers messages whose To and Cc entries all match
the connected peer. Multi-peer routing requires per-recipient state.

The local tests exercise bidirectional exchange, multiple batches, and
disconnect after durable storage but before acknowledgment. Reconnect
uses the stored MID to avoid duplicate delivery. The session can honor
outbound resume requests. Its directory mailbox starts interrupted
inbound transfers from the beginning.

A loopback TCP bench also exchanged attachments both ways with the
[wl2k-go v1.0.1 forwarding stack](https://github.com/la5nta/wl2k-go/tree/v1.0.1/fbb).
Both engines quit cleanly and both received SHA-256 hashes matched the
original 12-byte fixture. This bench uses a plain TCP stream. The Pat application modem test is described below.

## Exchange mail through the modem

Queue an existing B2F envelope and call the peer using the local modem.
On the peer, run `mail listen` first with its local and peer callsigns.
Each command runs one exchange and waits for a modem disconnect notice.

```sh
linkwise mail queue ./mailbox message.b2f
linkwise mail exchange ./mailbox N0CALL W1AW
```

The virtual BW500 test runs both commands against two local modems linked
by their virtual audio ports. It checks both received envelopes, attachment
SHA-256, delivery markers, and clean disconnection.

```sh
cargo build --locked --manifest-path rust/Cargo.toml --bins
python3 tools/fetch-modem-profile.py
python3 tools/test-winlink-modem.py rust/target/debug/linkwise \
    rust/target/debug/linkwise-modemd reference/vara/fixtures/level4/bw500-level4.json
```

### Pat interoperability

On Linux, add `--pat /path/to/pat` to the virtual BW500 test to use
Pat v1.0.1 as the VARA HF P2P caller. The test creates a disposable Pat
mailbox and configuration. Run with loopback only to isolate the session
from CMS. The CI workflow uses Docker with `--network none`.

In this test both 1,400-byte attachments matched their source SHA-256.
Pat filed its outbound message in Sent, Linkwise marked its own outbound
message confirmed, and both ends reported disconnection.

### Winlink Express interoperability

Winlink Express 1.8.6.0 received a 1,400-byte attachment in VARA HF P2P
mode through the installed Windows ARM64 Linkwise service. The Mac
Linkwise mail CLI initiated the BW500 session over a virtual waveform
relay. Express reported disconnection and returned to listening. The
Linkwise CLI exited successfully and marked the message confirmed.

The attachment extracted from Express message `LWMOB261113` matched the
original SHA-256
`2bb77b09b73f8d777d0af0e975ff2f1249511ebb7fff96aa7e81c421c51978df`.

In the reverse direction, Express sent message `8VJDYBE2NVUO` with a ZIP
attachment. Linkwise stored the envelope and the extracted 1,400-byte
fixture matched SHA-256
`4a54ac9f2e408dfef041ce0aa1c155d1db79d9fbd84b6bcad784695ecb025262`.
Both sides disconnected cleanly. The inbound parser accepts the single
NUL Express appends after the final CRLF.

## Import and export internet mail

Use `linkwise mail import ./mailbox message.eml` to queue an email for
P2P forwarding. Use `linkwise mail export ./mailbox MESSAGE_ID > received.eml`
to render an inbox message for an email client. Import expects one sender
and one recipient with `callsign@winlink.org` addresses, an ASCII
plain-text body, and ASCII subject and filenames. Binary attachments
retain their decoded bytes. Start local SMTP submission with `linkwise mail smtp ./mailbox N0CALL`.
A client on the same host can submit to `127.0.0.1:2525` using
`N0CALL@winlink.org` as its sender. This development endpoint uses
plain SMTP on loopback. A successful reply confirms that the message
was queued. Run a separate `mail exchange` to deliver it to the peer.
Start IMAP access with `linkwise mail imap ./mailbox`. The
development listener uses `127.0.0.1:1143` and automatically authenticates
local connections. Use it with a trusted local account. It supports
listing and selecting Inbox or Sent, setting or clearing the persistent
read flag with STORE or UID STORE,
checking mailbox status, searching messages, and fetching whole MIME
messages or individual sections by sequence number or UID. Selecting INBOX or polling it
with NOOP discovers new arrivals. Inbox UIDs and UIDVALIDITY persist
across restarts in the mailbox's `imap-inbox` metadata directory.
EXAMINE opens a mailbox read-only. Sent copies saved with APPEND retain their
UIDs across restarts and do not queue radio traffic. The in-memory Inbox
and the Sent archive each have a 64 MiB limit.

For a client that uses implicit TLS, build the development CLI with
`cargo build --locked --manifest-path rust/Cargo.toml --features mail-tls --bin linkwise`.
Supply the certificate chain and private key in PEM format:

```sh
linkwise mail imap ./mailbox 1143 --tls cert.pem key.pem
linkwise mail smtp ./mailbox N0CALL 2525 --tls cert.pem key.pem
```

With `--tls`, each connection starts with a TLS handshake. Configure
the client for SSL/TLS and use a server address that matches the
certificate's subject alternative name. The client must trust the
issuing CA. For an Android emulator using `adb reverse` with
`127.0.0.1`, the certificate needs an IP subject alternative name for
`127.0.0.1`. TLS protects the connection and authenticates the server.
Set both `LINKWISE_IMAP_USER` and `LINKWISE_IMAP_PASSWORD` in the
listener environment to require IMAP LOGIN over TLS. The listener
rejects credentials configured without TLS. Test credentials can
contain ASCII letters, digits, and `@._-+`. Without those variables
the IMAP listener uses its preauthenticated greeting. SMTP accepts
submissions from the configured callsign. Both listeners default to IPv4 loopback. Non-loopback binds require TLS
and authentication.
Run the full mail bridge test over two virtual BW500 modems:

```sh
python3 tools/test-winlink-modem.py rust/target/debug/linkwise \
    rust/target/debug/linkwise-modemd reference/vara/fixtures/level4/bw500-level4.json \
    --mail-bridge
```

The test opens an empty IMAP inbox, submits a MIME attachment through
SMTP, then transfers it in a Winlink P2P exchange. It checks that IMAP
detects the arrival on the open connection, retrieves the exact
attachment, and reports the same UIDVALIDITY. It also checks attachment
SHA-256 in both directions and clean modem disconnection. Mobile CI runs
this test with Python mail clients. Gmail for Android passed an attachment
round trip on the emulator with a trusted Linkwise CA and two virtual BW500
modems. See the [Rocky validation notes](../../docs/architecture/rocky-thunderbird-validation.md#gmail-attachment-round-trip-with-mime-section-retrieval).
Apple Mail on an iPhone and Gmail over physical radio remain unverified.

## Thunderbird for iOS simulator

The [simulator procedure](../../tools/ios-thunderbird/README.md) exercises
Thunderbird's SMTP STARTTLS and IMAP implicit TLS directly against Linkwise.
For SMTP, set both `LINKWISE_SMTP_USER` and `LINKWISE_SMTP_PASSWORD` and
start `linkwise mail smtp MAILBOX CALLSIGN PORT --starttls CERT.pem KEY.pem`.
The TLS listener maps submissions to the configured station and requires
successful authentication before accepting its sender address. The simulator
procedure uses a disposable CA and loopback sockets.

## Release qualification

- Connect the mailbox to the mail bridge and mobile applications.
- Run app tests on both simulators and verify the mail-client handoff on
  actual devices. Use a user-initiated app workflow on iOS because the OS
  can suspend apps in the background. An always-available SMTP/IMAP
  endpoint can live on an external Linkwise host.
- Qualify native audio, interruptions and route changes, physical radio
  interfaces, PTT release, and signed packages on real devices.
