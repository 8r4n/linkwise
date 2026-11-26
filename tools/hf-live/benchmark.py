#!/usr/bin/env python3
"""Measure exact Linkwise BW500 frame recovery on paired seeded bursts."""

import argparse
from collections import Counter
import json
import math
from pathlib import Path
import random
import statistics
import time

from hf_channel import GNU_RADIO_VERSION, Channel
from watterson import PATH_DELAY_SAMPLES, REFERENCE_BAND_HZ, background_noise, reference_power


MODELS = ("flat", "watterson-moderate")


def sweep(modem, snrs, trials, seed):
    channels = {model: Channel(modem, channel_model=model) for model in MODELS}
    rng = random.Random(seed)
    cases = [(rng.randbytes(9), rng.randrange(2**32)) for _ in range(trials)]
    bursts = [channels["flat"].prepare(payload) for payload, _ in cases]
    rows = []
    started = time.monotonic()
    for snr in snrs:
        matches = Counter()
        observed = {model: [] for model in MODELS}
        for index, ((payload, case_seed), burst) in enumerate(zip(cases, bursts)):
            results = {}
            # Use the same receiver noise in both arms. The flat arm retains
            # its legacy fade envelope, while the noise bandwidth stays fixed.
            noise = background_noise(
                burst.samples, len(burst.samples) + PATH_DELAY_SAMPLES, seed=case_seed, snr_db=snr,
            )
            for model, channel in channels.items():
                signal, _ = channel.propagate(
                    burst, seed=case_seed, snr_db=snr, include_noise=False,
                    path_key=(seed, index, "paired-bench"), started_at_s=0,
                )
                background = noise[:len(signal)]
                received = channel.decode_received(burst, signal, background)
                results[model] = received == payload
                observed[model].append(10 * math.log10(reference_power(signal) / reference_power(background)))
            matches[tuple(results.values())] += 1
        rows.append({
            "requested_snr_db": snr,
            "exact_frames": {
                model: sum(count for result, count in matches.items() if result[i])
                for i, model in enumerate(MODELS)
            },
            "both_exact": matches[True, True],
            "flat_only_exact": matches[True, False],
            "watterson_only_exact": matches[False, True],
            "neither_exact": matches[False, False],
            "median_measured_snr_db": {
                model: statistics.median(values) if values else None
                for model, values in observed.items()
            },
        })
    return {
        "seed": seed, "trials_per_snr": trials, "modem": str(modem),
        "gnu_radio": GNU_RADIO_VERSION, "elapsed_s": round(time.monotonic() - started, 3),
        "shared_noise_band_hz": REFERENCE_BAND_HZ,
        "rows": rows,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--modem", type=Path, required=True)
    parser.add_argument("--trials", type=int, default=24)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--snr", type=float, nargs="+", default=[-8, -4, 0, 4, 8, 12])
    args = parser.parse_args()
    if args.trials < 1 or not 0 <= args.seed < 2**32:
        parser.error("supply at least one trial and an unsigned 32-bit seed")
    if not all(math.isfinite(snr) and -100 <= snr <= 100 for snr in args.snr):
        parser.error("SNR must be finite and between -100 and 100 dB")
    print(json.dumps(sweep(args.modem, args.snr, args.trials, args.seed), indent=2))


if __name__ == "__main__":
    main()
