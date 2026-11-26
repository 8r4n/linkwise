#!/usr/bin/env python3
"""Archive calibration traces for the Linkwise F.1487 reference profile."""

import argparse
import json
import math
from pathlib import Path

import numpy as np
from scipy.signal import correlate, welch

import watterson


def measure_taps(taps):
    rate = watterson.COEFFICIENT_RATE
    statistics = []
    for tap in taps:
        frequency, density = welch(tap, fs=rate, nperseg=4096, detrend=False, return_onesided=False)
        center = float(np.sum(frequency * density) / np.sum(density))
        variance = float(np.sum((frequency - center) ** 2 * density) / np.sum(density))
        statistics.append({
            "mean_power": float(np.mean(abs(tap) ** 2)),
            "mean_doppler_hz": center,
            "doppler_spread_2sigma_hz": 2 * math.sqrt(variance),
        })
    first, second = (tap - tap.mean() for tap in taps)
    lags = correlate(first, second, method="fft") / (np.linalg.norm(first) * np.linalg.norm(second))
    middle = len(first) - 1
    width = 10 * rate
    return {
        "taps": statistics,
        "power_imbalance_db": abs(10 * math.log10(statistics[0]["mean_power"] / statistics[1]["mean_power"])),
        "peak_normalized_cross_correlation": float(np.max(abs(lags[middle - width:middle + width + 1]))),
    }


def reference_noise_check():
    t = np.arange(4 * watterson.SAMPLE_RATE) / watterson.SAMPLE_RATE
    signal = (0.5 * np.sin(math.tau * 1500 * t)).astype(np.float32)
    noise = watterson.background_noise(signal, len(signal), seed=42, snr_db=17)
    achieved = 10 * math.log10(watterson.reference_power(signal) / watterson.reference_power(noise))
    return signal, noise, achieved


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new directory for raw traces and JSON measurements")
    args = parser.parse_args()
    output = args.output
    output.mkdir()
    duration = 1200
    traces = []
    count = duration * watterson.COEFFICIENT_RATE
    for seed in (17, 42):
        taps = watterson.coefficients(0, count, seed=seed)
        traces.append({"seed": seed, **measure_taps(taps)})
        np.savez_compressed(output / f"taps-{seed}.npz", direct=taps[0], delayed=taps[1])
    signal, noise, measured_snr = reference_noise_check()
    np.savez_compressed(output / "receiver-reference.npz", signal=signal, noise=noise)
    summary = {
        "profile": watterson.PROFILE,
        "tap_trace_duration_s": duration,
        "coefficient_rate_hz": watterson.COEFFICIENT_RATE,
        "sample_rate_hz": watterson.SAMPLE_RATE,
        "delay_samples": [0, watterson.PATH_DELAY_SAMPLES],
        "doppler_fit": "First and second moments of two-sided Welch PSD, 4096 samples per segment",
        "cross_correlation_max_lag_s": 10,
        "measurement_band_hz": watterson.REFERENCE_BAND_HZ,
        "effective_noise_bandwidth_hz": watterson.effective_noise_bandwidth_hz(),
        "requested_snr_db": 17,
        "measured_snr_db": measured_snr,
        "traces": traces,
    }
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
