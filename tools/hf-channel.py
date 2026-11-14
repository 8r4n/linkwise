#!/usr/bin/env python3
"""Apply a repeatable GNU Radio channel to a mono, float32 LE waveform."""
import argparse
import json
import math
import os
from pathlib import Path

# Select the generic VOLK kernels before GNU Radio loads them. SIMD tail
# rounding can otherwise depend on scheduler chunk boundaries.
os.environ["VOLK_GENERIC"] = "1"

import numpy as np
from gnuradio import analog, blocks, gr
from scipy.signal import hilbert


def apply_channel(samples, rate, snr_db, offset_hz, seed):
    flowgraph = gr.top_block()
    if offset_hz:
        source = blocks.vector_source_c(hilbert(samples).tolist())
        phase = (2 * np.pi * offset_hz / rate) * np.arange(len(samples))
        oscillator = blocks.vector_source_c(np.exp(1j * phase).tolist())
        mixer = blocks.multiply_cc()
        real = blocks.complex_to_real()
        flowgraph.connect(source, mixer)
        flowgraph.connect(oscillator, (mixer, 1))
        flowgraph.connect(mixer, real)
        signal = real
    else:
        signal = blocks.vector_source_f(samples.tolist())
    sink = blocks.vector_sink_f()
    if snr_db is None:
        flowgraph.connect(signal, sink)
    else:
        rms = math.sqrt(float(np.mean(samples.astype(np.float64) ** 2)))
        noise = analog.noise_source_f(analog.GR_GAUSSIAN, rms * 10 ** (-snr_db / 20), seed)
        summer = blocks.add_ff()
        flowgraph.connect(signal, summer)
        flowgraph.connect(noise, (summer, 1))
        flowgraph.connect(summer, sink)
    flowgraph.run()
    return np.asarray(sink.data(), dtype="<f4")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--rate", type=int, default=48000)
    parser.add_argument("--snr-db", type=float)
    parser.add_argument("--offset-hz", type=float, default=0)
    parser.add_argument("--seed", type=int, default=42)
    args = parser.parse_args()
    if (args.rate <= 0 or not math.isfinite(args.offset_hz)
            or abs(args.offset_hz) >= args.rate / 2
            or (args.snr_db is not None and not -100 <= args.snr_db <= 100)
            or not 1 <= args.seed < 2**31):
        parser.error("invalid rate, frequency offset, SNR, or seed")
    raw = args.input.read_bytes()
    if not raw or len(raw) % 4:
        parser.error("input must contain complete float32 samples")
    samples = np.frombuffer(raw, dtype="<f4")
    if not np.isfinite(samples).all():
        parser.error("input samples must be finite")
    output = apply_channel(samples, args.rate, args.snr_db, args.offset_hz, args.seed)
    output.tofile(args.output)
    print(json.dumps({
        "gnu_radio": gr.version(), "sample_rate_hz": args.rate, "samples": len(output),
        "snr_db": args.snr_db, "snr_reference": "input RMS over entire capture",
        "offset_hz": args.offset_hz, "seed": args.seed,
    }))


if __name__ == "__main__":
    main()
