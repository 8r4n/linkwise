# Linkwise modem implementation reference

This reference covers the Rust waveform, framing, and session implementation.
It shares the DSP core and the worked BW500 appendix with the architecture
white paper. It adds literature traceability, the wide-mode geometry, and a
captured BW2300 recovery trace.

Build from the repository root:

~~~sh
bash tools/build-modem-implementation.sh
~~~

The build uses the [white paper's diagram pipeline](../whitepaper/README.md)
and XeLaTeX through latexmk. The generated PDF is
`docs/build/modem-implementation/linkwise-modem-implementation.pdf`.
See the [LaTeX source](linkwise-modem-implementation.tex) for the test
commands, source baseline, and bibliography.
