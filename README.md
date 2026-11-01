# OpenVARA

OpenVARA is an independent, cross-platform Rust modem research project. Its
long-term goal is to implement a native modem that interoperates with an
unmodified VARA HF installation on macOS, Windows, and Linux, while providing
the tooling needed to measure, test, and document that compatibility.

VARA HF is the interoperability target. OpenVARA is a separate implementation:
it is not a redistribution of VARA software, decompiled source, or proprietary
assets.

## Project vision

The project brings together two related efforts:

1. **Modem interoperability.** Establish the externally observable behavior of
   VARA HF through authorized, controlled reference observations, then build a
   clean-room Rust implementation with deterministic tests and documented
   evidence.
2. **HF propagation operations and research.** Build a practical toolbox for
   selecting VARA HF Winlink gateways and studying 10 m propagation, including
   sporadic-E observations and a carefully separated investigation of whether
   satellite reentry timing correlates with unusual openings.

The propagation work supports the modem work by defining realistic operating
conditions and repeatable experiments. It does not assume that sporadic-E or
reentry events are predictable, global, or suitable as automatic transmission
triggers.

## Current status

OpenVARA is in the research and foundation stage.

- The Rust crate is a scaffold with module boundaries for the application API,
  audio, modem, and platform integration.
- The stable Rust toolchain, formatter, Clippy, rust-analyzer, and six declared
  cross-compilation targets are configured.
- Research notes, a proposed implementation roadmap, and a propagation-toolbox
  guide are available.
- Static reference analysis and planning are underway.
- The modem protocol, waveform, framing, modulation, FEC, ARQ, and audio
  behavior are not yet implemented or established.
- Compilation does not constitute an interoperability claim.

The authoritative research record is
[lab-notes/lab-notes.tex](lab-notes/lab-notes.tex).

## Research principles

OpenVARA treats compatibility as an evidence problem, not an assumption. Each
technical claim should be labeled as one of:

- **DOCUMENTED** — stated by a source or specification;
- **MEASURED** — observed in a reproducible recording or experiment;
- **VALIDATED** — confirmed by repeatable decoding or interoperability tests;
- **HYPOTHESIS** — plausible but not yet confirmed;
- **UNKNOWN** — insufficient evidence.

UI strings, binary cross-references, candidate parameters, generic OFDM
knowledge, and simulations are useful research leads. They are not proof of
VARA protocol semantics.

## Modem engineering goal

The eventual implementation is intended to provide modular, portable Rust
components for:

- host command and data interfaces;
- physical-layer synchronization and waveform analysis;
- modulation, demodulation, coding, framing, and integrity checks;
- link/session behavior and ARQ;
- audio input/output and PTT integration;
- diagnostics, capture analysis, and reproducible regression fixtures.

The proposed architecture keeps protocol and DSP code separate from
operating-system and device adapters. Candidate waveform parameters and
features such as bandwidth, sample rate, OFDM structure, adaptive levels, ACK
signaling, and coding remain research questions until supported by captures and
interoperability tests.

## Propagation and Winlink research

The station scenario described by the propagation work is a Julian, California
station using a 10 m horizontal dipole, approximately 5–10 m above ground,
oriented East–West, with up to 100 W and LMR-400 feedline.

The operational objective is to reach an available VARA HF Winlink RMS gateway;
Winlink can then route the message onward. The preferred decision process is:

1. Refresh the current Winlink RMS gateway/channel list.
2. Rank 10 m VARA gateways by availability, distance, bearing, and service
   code.
3. Run a separately documented VOACAP prediction for each candidate gateway.
4. Compare the prediction with live PSK Reporter and DX Maps activity.
5. Inspect relevant GIRO ionograms or derived ionospheric data.
6. Record KeepTrack reentry information only as a separate experimental event
   marker.
7. Attempt a short, legal, clear-channel Winlink connection and log the result.

The [propagation toolbox](reference/propagation-toolbox.md) documents the
referenced tools, their roles, access methods, automation constraints, and
provenance requirements. It distinguishes model predictions, live radio
observations, ionosonde measurements, gateway state, and orbital event data.

## Evidence and safety boundaries

Dynamic modem tests should use an audio loopback, shielded setup, or conducted
test arrangement that prevents unintended radiation. Reference software and
captured material must be used only within the applicable authorization,
license, and legal boundaries. Do not transmit VARA on an FT8 frequency merely
to generate propagation reports; use a published, permitted Winlink channel.

The reentry/sporadic-E relationship is an unverified research hypothesis.
KeepTrack is an event-timing source, not a propagation forecast. A useful test
requires repeated events, non-event control periods, independent 10 m
observations, and timestamped records of both successful and failed attempts.

## Repository layout

```text
.
├── rust/                         OpenVARA Rust crate
├── lab-notes/                    Research log and progress presentation
├── reference/
│   ├── propagation-toolbox.md    Propagation-tool documentation
│   ├── vara/                     Local VARA reference materials
│   └── ghidra/                   Local Ghidra analysis resources
├── tools/                        Environment and VM support scripts
├── .vscode/                      Workspace MCP, editor, and task settings
└── rust-toolchain.toml           Pinned Rust channel/components/targets
```

Large vendor-supplied binaries, generated build output, local VM state, and
proprietary reference bundles are intentionally excluded by
[.gitignore](.gitignore). They may remain available in a local research
environment without becoming part of the source distribution.

## Development

Install the stable Rust toolchain with rustup, then work from the Rust crate:

```sh
cd rust
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The complete configured verification sequence is available in VS Code as
`OpenVARA: verify`. The repository currently checks these targets:

```text
aarch64-apple-darwin       x86_64-apple-darwin
aarch64-pc-windows-msvc    x86_64-pc-windows-msvc
aarch64-unknown-linux-gnu  x86_64-unknown-linux-gnu
```

Cross-target compilation is necessary but does not replace native execution,
audio-device validation, PTT testing, or reference interoperability tests.

## Roadmap

The planned sequence is intentionally gated by evidence:

1. Establish an authorized reference runtime and controlled audio capture.
2. Build a traceable corpus of authentic VARA HF observations.
3. Implement deterministic WAV handling and waveform/spectrum analysis.
4. Characterize and test synchronization and receiver behavior.
5. Characterize framing, coding, modulation, and data decoding.
6. Implement a transmitter accepted by an unmodified reference receiver.
7. Add ARQ, session management, host interfaces, audio, and platform support.
8. Harden, document, and release only the modes and platforms actually tested.

The full proposed plan, exit criteria, blockers, and corpus policy are in the
[lab notes](lab-notes/lab-notes.tex).

## Project status and licensing

OpenVARA is research software and is not yet a functioning replacement modem.
No project license has been selected in this repository yet. Before a public
release, the project must review the licenses and trademarks covering OpenVARA,
reference software, captured data, dependencies, and distributed tooling.
