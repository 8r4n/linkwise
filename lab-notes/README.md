# Lab notes and presentation

- [`lab-notes.tex`](lab-notes.tex) is the chronological research log and
  evidence ledger.
- [`presentation.tex`](presentation.tex) is a Beamer progress briefing.
- Generated PDFs and LaTeX auxiliary files go in `build/`.

Build both PDFs from this directory with:

```sh
latexmk -pdf -interaction=nonstopmode -halt-on-error -outdir=build lab-notes.tex
latexmk -pdf -interaction=nonstopmode -halt-on-error -outdir=build presentation.tex
```

The October presentation records the initial setup and static analysis.
See the dated entries in the lab notes for current audio interoperability
results and the limits of the radio-free runtime.
