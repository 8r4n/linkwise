# Linkwise architecture whitepaper

Build the paper and all PlantUML figures from the repository root:

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

Architecture and software-design figures are authored only in
[`diagrams/`](diagrams/). The paper distinguishes the proposed system views
from as-built MCP views: the latter cover runtime components, Compose
deployment, control read/write sequences, write validation, and current PTT
behavior. Each diagram is placed on its own landscape page with a bounded
image size and caption. Edit PlantUML sources rather than generated LaTeX or
PDFs. PlantUML's LaTeX output is an intermediate rendering format, not a
diagram source of truth.

The source uses a portable `article` class and standard TeX packages. It is
original project material. The supplied
`acq-latex-templates` collection was surveyed for SAD/SDD/SRS document
structures, but its DoW acquisition class and markings were not imported or
copied.

The whitepaper is a proposed-system design, not a claim that modem, MCP, or
automated RF session-control behavior is implemented. The contract-level
software design is in
[`../architecture/software-design.md`](../architecture/software-design.md);
supporting architecture, requirements, and safety documents are in
[`../architecture/`](../architecture/).
