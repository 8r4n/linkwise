# Linkwise test documents

The KiwiSDR modem test plan follows the engineering sections of the
[acq-latex-templates Test Plan](https://github.com/8r4n/acq-latex-templates/blob/fef160e37542fd7bc8e62de81404a83b8634a396/templates/engineering/verification/test-plan/main.tex),
tailored to Linkwise project metadata and an exploratory RF-observation campaign.
It uses a portable `article` class rather than importing the acquisition class,
its placeholder roles/signatures, or its distribution markings. The template
crosswalk and lineage are retained in the plan; no standards compliance is claimed.

- Maintained content: [Markdown draft](../../rust/docs/kiwisdr-test-plan.md).
- Generated, checked-in source: [LaTeX edition](kiwisdr-modem-test-plan.tex).
- Generated PDF: `docs/build/kiwisdr-test-plan/kiwisdr-modem-test-plan.pdf` (ignored).

From the repository root:

```sh
# Regenerate LaTeX after editing the Markdown plan.
python3 tools/build-kiwisdr-test-plan.py

# Regenerate and render the PDF; requires PlantUML, Tectonic and Python PyMuPDF.
python3 tools/build-kiwisdr-test-plan.py --build
```

The generator rearranges content into Scope, Referenced Documents, Test Items,
Test Approach, Resources/Facilities/Equipment, Safety, Schedule, and Data Management
and Reporting, followed by backlog, tailoring, and readiness appendices. Its
bounded Markdown renderer supports the constructs used by this document; a
changed section structure requires an explicit tailoring-map update. The Mermaid
measurement flow is rendered as a PlantUML vector figure in the LaTeX edition.
Five PlantUML sources live under `diagrams/`; two TikZ schematics live under
`figures/`. The build uses installed PlantUML/Tectonic tools or the whitepaper
workspace-local fallback tools, with the same `LINKWISE_PLANTUML`,
`LINKWISE_TECTONIC` and `LINKWISE_JAVA_HOME` overrides. The main PDF uses latexmk/pdflatex when available and Tectonic otherwise.

**Layout rule:** all figures, diagrams and tables (including revision history)
use dedicated landscape pages. Both width and height are bounded, with reserved
caption space. Keep text legible; restructure any crowded source. Do not add
portrait tables or inline diagrams to the publication edition. The title-page
metadata is prose, rather than a portrait table.

Revision 0.9 records the exploratory RF receive result and separate stock VARA
bench evidence. It defines a PRBS9-derived BW500 pilot and a station readiness
gate before the first controlled RF burst. The campaign remains unscheduled.

## Julian transmitter and candidate map

The landscape map uses an approximate Julian, California transmit reference,
not the operator's exact antenna coordinates. Eleven geographically distinct
candidate sites are cross-referenced to a dated KiwiSDR directory-mirror snapshot
and individually checked metadata endpoints. Permission, capacity at the test
time, and RF path usability remain separate readiness checks.

- [Candidate map PDF](figures/kiwisdr-candidate-map.pdf) and [PNG preview](figures/kiwisdr-candidate-map.png).
- [Registry, GIS layers and provenance](data/README.md), including a QGIS-compatible
  GeoPackage, WGS84 GeoJSON, CSV, and the selected original directory fields.
- Map renderer: `tools/build-kiwisdr-candidate-map.py`; GIS dependencies and rebuild
  instructions are in the provenance guide. The normal plan build embeds the
  checked-in map PDF without requiring the GIS stack.

## Executable procedure tables

Section 14 gives ordered commands, expected results, evidence and stop conditions
for environment checks, 30-burst source preparation, local replay, pinned
KiwiClient setup, bounded capture, conversion and scoring. Hardware transmit
steps remain explicitly manual until a station-specific procedure is qualified.

- `tools/kiwisdr-bench.py` prepares the PRBS9-derived source WAVs and an
  exact-frame oracle through the local Rust demo, then verifies the exported
  files with the Rust decoder.
- `procedures/capture-profile.env.example` supplies the reviewed capture profile;
  `procedures/capture-audio.sh` checks the pinned client and limits capture time.
- `data/kiwiclient-command-reference.json` records the client source revision and
  the command options checked against that source.

Use the sequence in the plan from the repository root. Local source verification
is separate from RF observation scoring. Receiver continuity instrumentation,
station transmit qualification and the FFmpeg conversion check remain readiness
items; generating this document does not execute those operations.

The map PDF has clickable receiver IDs and register names. The plan build uses
PyMuPDF (`pip install pymupdf`) to restore these links after LaTeX imports the
map, because PDF graphic inclusion drops source annotations. PNG previews cannot
contain clickable PDF annotations. Recheck candidate HTTP access with
`python3 tools/check-kiwisdr-candidates.py`; results overwrite the dated
`data/kiwisdr-access-check.json` report and do not open audio streams.
