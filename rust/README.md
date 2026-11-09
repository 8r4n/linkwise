# Linkwise Rust scaffold

This crate contains the initial Linkwise MCP radio-control service and remains
the starting point for the independently implemented cross-platform modem.
The native modem currently implements BW500 level 1–4 DATA synthesis and
CRC-checked reception, setup/control primitives, an audio-driven handshake,
DATA containers, a bounded ARQ sender, and host-command/data stream transports. Level 4 requires
an external measurement profile. A bounded streaming receiver and optional CPAL input
backend expose the receive path. It is not a complete interoperable modem;
see [modem status and validation](docs/modem.md) for tested behavior and gaps.

The source modules mark the intended boundaries:

- `api`: application-facing command and status interface
- `audio`: platform audio-device integration
- `modem`: portable modem and link behavior
- `platform`: operating-system integration
- `radio`: typed control port, `rigctld` adapter, and simulated backend
- `mcp`: MCP stdio server mapped to the radio-control port

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

The MCP server uses the official Rust `rmcp` SDK. The simulated radio is the
default: frequency, mode, and power changes are held in memory for the life of
the MCP process, and it cannot access hardware or assert PTT. The Hamlib
adapter connects to the private `rigctld` service configured by
`LINKWISE_RIGCTLD_ADDR`. Physical CAT writes remain disabled unless explicitly
enabled with configured station limits. Additional VFO, passband, split,
antenna, memory-channel, level, and function writes also require explicit
`LINKWISE_ENABLED_HAMLIB_CONTROLS` entries. The Hamlib catalog is discovered
from the selected backend; unsupported controls are reported rather than
emulated. Native modem reception is separate from the MCP server; no modem
transmission or session control is exposed through MCP.

Launch the MCP server through the containerized development profile from the
repository root with `docker compose --profile dev run --build --rm -i linkwise`.
VS Code also has a `linkwise-simulated-radio` stdio MCP entry in
`.vscode/mcp.json`; reload the MCP servers after changing that file.

For AI/client usage, tool workflows, control value examples, result statuses,
and current policy limits, see [the MCP usage guide](docs/mcp-usage.md). The
runtime tool schemas and `radio_get_controls` response remain authoritative
for the backend and station profile currently selected.

Rust contributors should start with [the implementation guide](docs/developer-guide.md)
for module boundaries, backend contracts, concurrency and safety invariants,
and the checklist for adding a radio control. Public radio API contracts are
also documented in Rustdoc; generate them with `cargo doc --no-deps --locked`.

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
