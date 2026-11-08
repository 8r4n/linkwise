#!/bin/sh
set -eu

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
WHITEPAPER_DIR="$REPO_ROOT/docs/whitepaper"
PLANTUML="${LINKWISE_PLANTUML:-plantuml}"
TECTONIC="${LINKWISE_TECTONIC:-tectonic}"

# Reuse explicitly provisioned workspace-local tools when they are not on PATH.
if ! command -v "$PLANTUML" >/dev/null 2>&1 &&
  [ -x "$WHITEPAPER_DIR/build/tools/bin/plantuml" ]; then
  PLANTUML="$WHITEPAPER_DIR/build/tools/bin/plantuml"
fi
if ! command -v "$TECTONIC" >/dev/null 2>&1 &&
  [ -x "$WHITEPAPER_DIR/build/tools/bin/tectonic" ]; then
  TECTONIC="$WHITEPAPER_DIR/build/tools/bin/tectonic"
fi

for tool in "$PLANTUML" "$TECTONIC"; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'Missing required tool: %s\n' "$tool" >&2
    printf 'See docs/whitepaper/README.md for installation instructions.\n' >&2
    exit 127
  fi
done

cd "$WHITEPAPER_DIR"
mkdir -p build/diagrams

# PlantUML is the source of truth. Its standalone LaTeX output is compiled to
# vector PDFs, which the main paper includes as figures.
"$PLANTUML" --check-before-run --stop-on-error --latex \
  -Playout=smetana \
  --output-dir "$WHITEPAPER_DIR/build/diagrams" diagrams/*.puml

for diagram in diagrams/*.puml; do
  diagram_name=$(sed -n 's/^@startuml[[:space:]]*\([^[:space:]]*\).*/\1/p' "$diagram" | sed -n '1p')
  if [ -z "$diagram_name" ]; then
    printf 'Could not determine PlantUML diagram name from %s.\n' "$diagram" >&2
    exit 1
  fi
  diagram_source="build/diagrams/$diagram_name.tex"
  if [ ! -f "$diagram_source" ]; then
    printf 'PlantUML did not produce %s.\n' "$diagram_source" >&2
    exit 1
  fi
  "$TECTONIC" --keep-logs --keep-intermediates --outdir build/diagrams \
    "$diagram_source"
done

"$TECTONIC" --keep-logs --keep-intermediates --outdir build \
  linkwise-architecture-whitepaper.tex
