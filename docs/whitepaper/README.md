# Linkwise Station Architecture and Qualification

[White paper 1.0](linkwise-architecture-whitepaper.tex) follows a managed BW500
attempt from ranking and authorization through transfer, abort, and cleanup.
It identifies the installed station paths, the proposed commissioned radio
boundary, and the evidence needed before live operation. Rocky Linux 9 ARM64
is the production build target. The WSL development builds have Linux-side
verification.

## Build the white paper

From the repository root, run the following command. The PDF and three vector
figures are written to the ignored `docs/whitepaper/build/` directory.

```sh
bash tools/build-whitepaper.sh
```

The output is `docs/whitepaper/build/linkwise-architecture-whitepaper.pdf`.
Install PlantUML and Tectonic through your platform's package manager.
On macOS with Homebrew, run `brew install plantuml tectonic`. The build uses
PlantUML's built-in Smetana layout and compiles its LaTeX output with Tectonic.
For the JAR distribution, set `LINKWISE_JAVA_HOME` or `JAVA_HOME` as needed.
The build also detects a runtime at `docs/whitepaper/build/tools/jre`.
The first build can fetch TeX dependencies into the local cache.

## Technical references

- [Archived version 0.4](linkwise-architecture-reference.tex) preserves the
  larger design and experiment record for historical context. Build it with
  `bash tools/build-whitepaper.sh --reference` to produce
  `docs/whitepaper/build/linkwise-architecture-reference.pdf`.
- The archive includes the [modem architecture and DSP chapter](modem-software-architecture.tex),
  [worked wire example](modem-wire-example.tex), propagation literature
  survey, and original [PlantUML and TikZ sources](diagrams/).
- The [software design](../architecture/software-design.md) and
  [requirements](../architecture/requirements-and-use-cases.md) hold detailed
  contracts and traceability.
- The [station integration record](../architecture/station-integration-status.md)
  and [lab notes](../../lab-notes/lab-notes.tex) hold dated test scope,
  reproduction details, and open qualification items.
