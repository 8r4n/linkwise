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

The presentation opens with the 9 November native modem/demo evidence and
proposed agent-chat architecture. Older scaffold/demo slides are explicitly
historical. The crate contains tested codec and protocol prototypes, but is
not yet a qualified operational RF modem or agent messaging service.
