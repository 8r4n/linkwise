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

The presentation deliberately distinguishes completed setup and static
analysis from pending dynamic testing. The Rust crate is not yet a functioning
modem.
