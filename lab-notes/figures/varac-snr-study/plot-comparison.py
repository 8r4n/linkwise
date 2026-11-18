from pathlib import Path
import json

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

root = Path(__file__).resolve().parent
data = json.loads((root / "report.json").read_text())
table = np.array(data["table"])
fig, ax = plt.subplots(figsize=(297 / 25.4, 210 / 25.4))
fig.patch.set_facecolor("#FAFBFC")
ax.set_facecolor("#FAFBFC")
fig.subplots_adjust(left=.10, right=.96, bottom=.43, top=.79)
fig.text(.10, .925, "Fresh VarAC beacon decoding", fontsize=23, weight="bold", color="#173744")
fig.text(.10, .875, "17 November 2026  •  Two beacon payloads  •  20 trials per SNR",
         fontsize=12, color="#465B66")
positions = np.arange(len(table))
width = .23
series = [
    ("Baseline Linkwise", "#9CAAB5", -width, 1),
    ("Updated Linkwise", "#087F8C", 0, 2),
    ("Stock VARA 4.9.1", "#BA5B2B", width, 3),
]
for name, color, shift, column in series:
    counts = table[:, column]
    bars = ax.bar(positions + shift, counts * 5, width=.20, color=color, label=name, zorder=3)
    for bar, count in zip(bars, counts):
        ax.text(bar.get_x() + bar.get_width()/2, bar.get_height()+2,
                f"{count}/20", ha="center", va="bottom", fontsize=11, weight="bold",
                color="#173744")
ax.axhline(95, color="#6C5768", lw=1.2, ls=(0,(5,4)), zorder=2)
ax.text(-.43, 96.3, "95% target", color="#6C5768", fontsize=10, va="bottom")
ax.set_xticks(positions, [f"{snr:+d}".replace("+0", "0") for snr in table[:,0]])
ax.set_xlabel("Added-noise SNR (dB)", fontsize=12, labelpad=12, color="#173744")
ax.set_ylabel("Exact payload recovery (%)", fontsize=12, labelpad=12, color="#173744")
ax.set_ylim(0, 113)
ax.set_yticks([0,25,50,75,100])
ax.tick_params(axis="both", length=0, labelsize=11, pad=8)
ax.grid(axis="y", color="#DCE3E8", lw=.8, zorder=0)
for side in ("top","right","left"):
    ax.spines[side].set_visible(False)
ax.spines["bottom"].set_color("#BAC6CE")
ax.legend(loc="lower left", bbox_to_anchor=(-.015,1.035), ncol=3,
          frameon=False, fontsize=11, handlelength=1.4, columnspacing=2)
fig.text(.10, .315, "How to read this chart", fontsize=13, weight="bold", color="#173744")
fig.text(.10, .275,
         "Each bar counts complete, correct messages out of 20 attempts. Taller bars mean more successful decodes.",
         fontsize=10.5, color="#465B66")
fig.text(.10, .24,
         "Moving right means less added noise. At 0 dB, recorded burst and added noise have equal average power.",
         fontsize=10.5, color="#465B66")
fig.text(.10, .19, "What changed in Linkwise", fontsize=13, weight="bold", color="#173744")
fig.text(.10, .15,
         "Pilot tones help locate a frame. Linkwise now tries decoding when their match score reaches 0.20,",
         fontsize=10.5, color="#465B66")
fig.text(.10, .118,
         "down from 0.35. This lets weaker candidates reach timing refinement and error correction. A valid CRC",
         fontsize=10.5, color="#465B66")
fig.text(.10, .086,
         "is required to accept a frame. The 0.20 score is a detection setting; it is not an SNR value in dB.",
         fontsize=10.5, color="#465B66")
fig.text(.10, .042,
         "Stock VARA missed 1 of 6 clean controls. Results cover two source frames; original off-air SNR is unknown.",
         fontsize=9.5, color="#8B431F")
fig.savefig(root / "modem-comparison.pdf")
fig.savefig(root / "modem-comparison.png", dpi=160)
