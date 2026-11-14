#!/usr/bin/env python3
"""Measure the GNU Radio channel and optionally relay a BW500 session."""
import argparse
import itertools
import runpy
from pathlib import Path

import numpy as np


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--modem", help="path to a linkwise-modemd binary")
    parser.add_argument("--profile", help="path to the measured BW500 level-4 profile")
    parser.add_argument("--link-snr-db", type=float, help="apply seeded noise to each modem burst")
    args = parser.parse_args()
    if args.link_snr_db is not None and (not args.modem or not -100 <= args.link_snr_db <= 100):
        parser.error("--link-snr-db requires --modem and a value between -100 and 100")
    if bool(args.modem) != bool(args.profile):
        parser.error("--modem and --profile must be supplied together")

    directory = Path(__file__).parent
    apply_channel = runpy.run_path(str(directory / "hf-channel.py"))["apply_channel"]
    rate = 48000
    samples = np.sin(2 * np.pi * 1500 * np.arange(rate) / rate).astype("<f4")
    assert np.array_equal(samples, apply_channel(samples, rate, None, 0, 42))
    shifted = apply_channel(samples, rate, None, 25, 42)
    assert len(shifted) == len(samples)
    assert np.argmax(abs(np.fft.rfft(shifted))) == 1525
    noisy = apply_channel(samples, rate, 24, 25, 42)
    assert np.array_equal(noisy, apply_channel(samples, rate, 24, 25, 42))
    assert not np.array_equal(noisy, apply_channel(samples, rate, 24, 25, 43))
    snr = 10 * np.log10(np.mean(shifted**2) / np.mean((noisy - shifted)**2))
    assert abs(snr - 24) < 0.3, snr
    print(f"GNU Radio waveform checks passed. Measured SNR {snr:.2f} dB.")

    if args.modem:
        bench = runpy.run_path(str(directory / "test-virtual-bw500.py"))
        seeds = itertools.count(42)

        def relay(stream):
            raw = bench["waveform"](stream)
            samples = np.frombuffer(raw, offset=4, dtype="<f4")
            return raw[:4] + apply_channel(samples, rate, args.link_snr_db, 0, next(seeds)).tobytes()

        bench["disconnect_after_drain"](args.modem, args.profile, relay=relay)


if __name__ == "__main__":
    main()
