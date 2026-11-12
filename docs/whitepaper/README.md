# Linkwise architecture whitepaper

Build the paper and its PlantUML and TikZ figures from the repository root:

```sh
bash tools/build-whitepaper.sh
```

Install PlantUML and Tectonic using your platform's package manager. For
example, macOS with Homebrew: `brew install plantuml tectonic`. Java is needed
only when using PlantUML's JAR distribution. The build selects PlantUML's
built-in Smetana layout, so a separate Graphviz installation is not required.
When using the JAR distribution, the build uses `LINKWISE_JAVA_HOME` or
`JAVA_HOME` if set, and also detects a workspace-local runtime at
`build/tools/jre`.
PlantUML is used locally; no remote diagram-rendering service is involved. The build
validates each `.puml` source, generates standalone LaTeX/TikZ through
PlantUML, compiles each figure to a vector PDF with Tectonic, then compiles the
whitepaper. The first build needs network access so the tools can fetch and
cache PlantUML/TeX resources; subsequent builds reuse those caches. Generated
PDFs and auxiliary files are written to the ignored `build/` folder.

Figure sources are in [`diagrams/`](diagrams/). PlantUML sources use
the `.puml` extension. Native `.tikz` figures compile with the main paper.
The paper distinguishes the proposed system views
from as-built MCP views: the latter cover runtime components, Compose
deployment, control read/write sequences, write validation, and current PTT
behavior. Each diagram is placed on its own landscape page with a bounded
image size and caption. Edit the `.puml` or `.tikz` source for each figure.

The source uses a portable `article` class and standard TeX packages. It is
original project material. The supplied
`acq-latex-templates` collection was surveyed for SAD/SDD/SRS document
structures, but its DoW acquisition class and markings were not imported or
copied.

The whitepaper describes the proposed station system and the implemented
radio-free modem and MCP components. The contract-level
software design is in
[`../architecture/software-design.md`](../architecture/software-design.md);
supporting architecture, requirements, and safety documents are in
[`../architecture/`](../architecture/).

The detailed modem chapter is maintained in
[`modem-software-architecture.tex`](modem-software-architecture.tex). It covers
ChatGPT Agent/Codex orchestration, station execution and modem ports, native
module ownership, index and level-4 receive/transmit pipelines, FEC, framing,
ARQ, keying, and qualification gates. The six `modem-*` / `modem-dsp-*` diagram
sources distinguish implemented components from proposed runtime integration;
all are rebuilt by the same build script. The additional
`agent-session-start.puml` view separates asynchronous execution from
preparation and authorization.

The proposed standalone modem runtime and traditional-client integration are
specified in the same modem chapter. It defines standalone/managed admission,
exclusive TCP channel ownership, audio/PTT completion, byte accounting, and
six shared-core implementation gates for station integration. The chapter
also records the foreground radio-free service, preview installers, and
the scope of the stock VARA and Winlink Express tests.
