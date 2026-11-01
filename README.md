# OpenVARA

OpenVARA is an independent, cross-platform Rust modem research project aimed
at interoperating with VARA HF. This workspace contains the Rust scaffold,
research notes, propagation toolbox, and local reference materials.

## Layout

- `rust/` — OpenVARA Rust crate, tests, and development instructions.
- `lab-notes/` — LaTeX research log and Beamer progress presentation.
- `reference/propagation-toolbox.md` — documented propagation and monitoring
  tools for later analysis.
- `reference/vara/installers/` — supplied VARA HF and VARA Terminal installers.
- `reference/ghidra/installers/` — Ghidra distribution and GhidraMCP extension.
- `reference/ghidra/bridge/` — supplied Ghidra MCP bridge wheel.
- `reference/ghidra/docs/` — Ghidra manuals, API references, and student guide.
- `vara.gpr` and `vara.rep/` — active local Ghidra analysis project.
- `.vscode/mcp.json` — workspace registration for the local Ghidra MCP bridge.
- `tools/fedora-qemu-install.sh` — unattended Fedora Workstation x86_64 setup under QEMU TCG.
- `.venv/` — local Python environment used by that bridge; do not move without
  updating the MCP server command.

The Ghidra project and bridge environment are kept at their current paths so
the running Ghidra setup remains usable.

## Rust checks

From `rust/`, run:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The Rust crate is currently a scaffold. Modem protocol, framing, modulation,
and audio behavior are not implemented yet. VARA remains the interoperability
target; OpenVARA is the name of this independent project.

## Fedora x86_64 QEMU VM

The Fedora 43 Everything netinst ISO and its published checksum are in
`~/Downloads/utm-share/`. An unattended installation was attempted, but QEMU
exited without installing Fedora. The existing QCOW2 image is valid but has
only 196 KiB allocated; it is not a bootable Fedora system. The install script
refuses to overwrite it, and `boot` should not be used until installation is
successfully completed.

The separate 64 GiB Fedora Workstation VM can be retried after investigating
the failed Kickstart run:

```sh
tools/fedora-qemu-install.sh install
```

The install uses Kickstart, QEMU TCG, and a temporary HTTP service for the
Kickstart configuration. It does not modify the existing UTM `Linux` VM. Only
after a successful install and first-boot verification, start the VM with:

```sh
tools/fedora-qemu-install.sh boot
```

## Ghidra learning reference

The bundled beginner guide is
[`reference/ghidra/docs/GhidraClass/Beginner/Introduction_to_Ghidra_Student_Guide.html`](reference/ghidra/docs/GhidraClass/Beginner/Introduction_to_Ghidra_Student_Guide.html).
It covers importing programs, decompilation, function graphs, and references.
