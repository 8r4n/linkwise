#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
sh "$root/tools/build-whitepaper.sh"
cd "$root/docs/whitepaper"
output="$root/docs/build/modem-implementation"
mkdir -p "$output"
latexmk -xelatex -interaction=nonstopmode -halt-on-error -outdir="$output" \
    ../modem-implementation/linkwise-modem-implementation.tex
