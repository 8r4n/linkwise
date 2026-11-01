# OpenVARA Rust scaffold

This crate is the starting point for an independently implemented,
cross-platform modem. The command protocol, on-air framing, modulation,
and adaptive link behavior are not implemented or specified yet.

The source modules mark the intended boundaries:

- `api`: application-facing command and status interface
- `audio`: platform audio-device integration
- `modem`: portable modem and link behavior
- `platform`: operating-system integration

Keep protocol and signal-processing code independent of operating-system
audio and device APIs. Record evidence and compatibility decisions before
turning the module boundaries into protocol assumptions.

## Development

Install the stable Rust toolchain, then run:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The crate is dependency-free at this stage. The binary only supports
`--help` and `--version`; modem operation is not implemented.

## Cross-platform targets

The initial build targets are macOS ARM64/x86_64, Windows ARM64/x86_64,
and Linux ARM64/x86_64. Rust standard libraries for these targets can be
installed with:

```sh
rustup target add \
  aarch64-apple-darwin x86_64-apple-darwin \
  aarch64-pc-windows-msvc x86_64-pc-windows-msvc \
  aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu
```

Cross-target compilation checks do not replace native builds, installer
validation, or audio-device testing on each operating system.
