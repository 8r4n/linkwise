# Lab notes and presentation

- [`lab-notes.tex`](lab-notes.tex) is the chronological research log and
  evidence ledger.
- [`lab-notes.pdf`](lab-notes.pdf) is the published edition copied to the website.
- [`presentation.tex`](presentation.tex) is a Beamer progress briefing.
- Generated PDFs and LaTeX auxiliary files go in `build/`.

Build both PDFs from this directory with:

```sh
latexmk -pdf -interaction=nonstopmode -halt-on-error -outdir=build lab-notes.tex
latexmk -pdf -interaction=nonstopmode -halt-on-error -outdir=build presentation.tex
```

The October presentation records the initial setup and static analysis.
The November 25 entry records station integration, Rocky server and WSL
verification, and the remaining qualification.
