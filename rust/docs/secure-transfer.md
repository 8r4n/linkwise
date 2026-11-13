# Encrypted Linkwise P2P transfer

`linkwise secure` runs a foreground file transfer against a dedicated local
Linkwise modem instance. Both ends need the secure CLI and their own
configuration. An omitted encryption section or `"enabled": false` rejects
the command before it opens any modem sockets. The regular modem package
build uses its existing feature set.

## Build

Use Rust, a native C toolchain, CMake, libclang and Go. On macOS AWS-LC
uses a shared FIPS module. Set its loader path for local tests.

```sh
cargo build --locked --manifest-path rust/Cargo.toml \
  --features audio-device,secure-transfer --bins
module=$(find "$PWD/rust/target/debug/build" -name "libaws_lc_fips_*_crypto.dylib" -print -quit)
cargo rustc --locked --manifest-path rust/Cargo.toml \
  --features audio-device,secure-transfer --bin linkwise -- \
  -C "link-arg=-Wl,-rpath,$(dirname "$module")"
```

## Configure both stations

Give each station a P-256 certificate with a DNS subject alternative name
and both client and server TLS authentication usages. Use a dedicated CA,
exchange the public leaf certificates securely, and restrict access to
the private keys and configuration. The current format takes one trust
anchor and one leaf per endpoint. Replace peer pins in coordination with
certificate rotation. Paths are relative to the invoking process unless
absolute.

```json
{
  "encryption": {
    "enabled": true,
    "settings": {
      "local_call": "KN6MQS",
      "peer_call": "W1AW",
      "peer_dns": "w1aw.example.test",
      "peer_certificate": "/etc/linkwise/w1aw.pem",
      "certificate": "/etc/linkwise/kn6mqs.pem",
      "private_key": "/etc/linkwise/kn6mqs.key",
      "ca_certificate": "/etc/linkwise/transfer-ca.pem",
      "command_port": 18300,
      "data_port": 18301,
      "max_bytes": 1048576
    }
  }
}
```

On the peer, swap the callsigns and certificate roles and set the sender's
DNS name. Use the port pair assigned to the dedicated modem on each host.
The CLI connects only to IPv4 loopback. Other local processes can reach
loopback ports, so run the current implementation in a trusted local
environment with exclusive use of those ports.

## Transfer

Start the receiver first. Both commands exit successfully after the TLS
transfer and modem disconnect finish.

```sh
linkwise secure receive receiver.json received.bin
linkwise secure send sender.json original.bin
```

Use `-` to read sender stdin or write receiver stdout. Stdout streams can
contain partial data on failure. For all-or-nothing publication, receive
to a file and check the exit status.

```sh
linkwise secure receive receiver.json - > received.bin
cat original.bin | linkwise secure send sender.json -
```

The receiver verifies the byte count and SHA-256 digest. File mode writes
to a temporary file in the destination directory and publishes it without
overwriting an existing file. The sender verifies the digest returned in
the authenticated receipt. If the link fails after publication but before
the receipt arrives, the receiver may already have the complete file.
The protocol has no automatic retry or deduplication.

## Protocol and qualification

TLS 1.3 requires `TLS_AES_256_GCM_SHA384`, which uses a 256-bit AES-GCM
traffic key. The handshake uses ephemeral P-256 key agreement. Both
peers authenticate against the configured CA and pinned leaf certificates
before application data flows. The caller also validates the receiver DNS
name. Both peers check the Linkwise ALPN identifier. The protocol has
bounded payload records, per-record flow control and an authenticated
receipt. A TLS or authentication error terminates the transfer.

This build uses rustls with the AWS-LC FIPS provider and checks the TLS
configuration at startup. A deployment's FIPS status depends on the
specific cryptographic module build, its CMVP certificate and security
policy, and the operating environment. The virtual-audio test qualifies
the application path. The bundled 4.x module is listed by [NIST as in process](https://csrc.nist.gov/projects/cryptographic-module-validation-program/modules-in-process/modules-in-process-list).
Deployment qualification, service-manager
integration, and OS-enforced local access controls are separate work.

## Run the radio-free test

```sh
python3 tools/fetch-modem-profile.py
python3 tools/test-secure-transfer.py rust/target/debug/linkwise \
  rust/target/debug/linkwise-modemd reference/vara/fixtures/level4/bw500-level4.json
```
