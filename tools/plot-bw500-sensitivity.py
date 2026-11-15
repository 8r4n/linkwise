#!/usr/bin/env python3
"""Plot trial CSVs from the ignored BW500 sensitivity experiment."""

import argparse
from collections import defaultdict
import csv
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt


KINDS = {
    "data1": "DATA level 1", "data2": "DATA level 2", "data3": "DATA level 3",
    "data4": "DATA level 4", "setup": "Setup", "ack": "Setup ACK",
    "request": "Connection request", "response": "Connection response",
}
SERIES = {
    "baseline_start": ("Stock-capture replay baseline", "#242222", "o"),
    "baseline_noise_only": ("Baseline on noise only", "#242222", "x"),
    "band_start": ("Band-limited energy gate", "#14766c", "o"),
    "band_noise_only": ("Band-limited gate on noise only", "#14766c", "x"),
    "baseline_decode": ("Current gate + decoder", "#242222", "o"),
    "band_decode": ("Band-limited gate only", "#14766c", "s"),
    "filtered_decode": ("Offline band-limited gate + decoder", "#bc4b18", "o"),
    "streaming_decode": ("Causal BW500 receiver", "#166742", "P"),
    "clean_window_decode": ("Clean candidate boundaries supplied", "#665297", "^"),
    "aligned_decode": ("Clean alignment supplied", "#196eb2", "D"),
}


def read_trials(path):
    rows = {}
    with path.open(newline="") as source:
        for row in csv.DictReader(source):
            key = (row["kind"], row["fixture"], int(row["snr_db"]), int(row["seed"]))
            if key in rows or None in row or None in row.values():
                raise ValueError(f"Invalid or duplicate trial in {path}: {key}")
            rows[key] = row
    return rows


def plot(rows, keys, title, destination):
    data = defaultdict(list)
    for (kind, _, snr, _), row in rows.items():
        for key in keys:
            data[kind, snr, key].append(int(row[key]))

    present = {key[0] for key in rows}
    kinds = [(kind, label) for kind, label in KINDS.items() if kind in present]
    columns = min(3, len(kinds))
    row_count = (len(kinds) + columns - 1) // columns
    fig, axes = plt.subplots(row_count, columns, figsize=(12, 3 * row_count + 1),
                             sharex=True, sharey=True, squeeze=False)
    for unused in list(axes.flat)[len(kinds):]:
        unused.set_visible(False)
    for axis, (kind, label) in zip(axes.flat, kinds):
        axis.set_title(label, loc="left", fontweight="bold")
        axis.grid(axis="y", alpha=0.22)
        for key in keys:
            if kind in {"ack", "request", "response"} and key == "aligned_decode":
                continue
            points = sorted(
                (snr, sum(values) / len(values))
                for (group, snr, metric), values in data.items()
                if group == kind and metric == key
            )
            legend, color, marker = SERIES[key]
            x, y = zip(*points)
            axis.plot(x, y, label=legend, color=color, marker=marker, markersize=3,
                      linewidth=1.35, linestyle="--" if key.endswith("noise_only") else "-")
        axis.set_ylim(-0.05, 1.05)
        axis.set_yticks([0, 0.25, 0.5, 0.75, 1], ["0%", "25%", "50%", "75%", "100%"])
    handles, labels = axes.flat[0].get_legend_handles_labels()
    fig.suptitle(title, fontsize=16, fontweight="bold")
    fig.supxlabel("Input SNR (dB, 3 kHz noise reference)", y=0.106)
    fig.legend(handles, labels, loc="lower center", bbox_to_anchor=(0.5, 0.012), ncol=2)
    fig.tight_layout(rect=(0.02, 0.12, 0.99, 0.95))
    fig.savefig(destination, dpi=155)
    plt.close(fig)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trials", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--title", required=True)
    parser.add_argument("--filtered-trials", type=Path)
    args = parser.parse_args()
    rows = read_trials(args.trials)
    if args.filtered_trials:
        filtered = read_trials(args.filtered_trials)
        if filtered.keys() != rows.keys():
            raise ValueError("Trial keys differ between baseline and filtered CSVs")
        for key, row in rows.items():
            row.update({name: value for name, value in filtered[key].items() if value})
    args.output.mkdir(parents=True, exist_ok=True)
    with (args.output / "trials.csv").open("w", newline="") as output:
        writer = csv.DictWriter(output, fieldnames=list(next(iter(rows.values()))))
        writer.writeheader()
        writer.writerows(rows.values())
    for filename, keys, subtitle in (
        ("detection", ("baseline_start", "band_start", "baseline_noise_only", "band_noise_only"),
         "Burst starts and noise-only triggers"),
        ("decode", ("baseline_decode", "band_decode", "filtered_decode", "streaming_decode"),
         "Exact frame or keyed control recovery"),
        ("alignment", ("baseline_decode", "clean_window_decode", "aligned_decode"),
         "Effect of supplying clean timing"),
    ):
        available = tuple(key for key in keys if key in next(iter(rows.values())))
        plot(rows, available, f"{args.title} · {subtitle}", args.output / f"{filename}.png")


if __name__ == "__main__":
    main()
