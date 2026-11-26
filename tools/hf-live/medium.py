"""Receiver-side mixing for aligned, finite bursts on one modeled RF band."""

import math

import numpy as np


INTERFERENCE_PROFILES = ("none", "tone", "impulse", "mixed")


def add_overlap(target, incoming, offset_samples, gain):
    """Add the portion of a waveform that arrives in the receiver window."""
    begin = max(0, offset_samples)
    end = min(len(target), offset_samples + len(incoming))
    if end > begin:
        target[begin:end] += gain * incoming[begin - offset_samples:end - offset_samples]


def injected_interference(profile, *, rate, count, start_s, noise_power, seed):
    """Return repeatable bench disturbances, with levels referenced to noise.

    The tone has 10 dB interference-to-noise ratio. The impulsive case
    generates seeded, damped in-band pulses with a 26 dB peak-to-noise ratio.
    """
    audio = np.zeros(count, dtype=np.float32)
    noise_rms = math.sqrt(noise_power)
    if profile in ("tone", "mixed"):
        time_s = start_s + np.arange(count) / rate
        audio += math.sqrt(20) * noise_rms * np.sin(2 * np.pi * 1483 * time_s)
    if profile in ("impulse", "mixed"):
        rng = np.random.default_rng(seed)
        t = np.arange(round(0.02 * rate)) / rate
        envelope = np.exp(-t / 0.003)
        for offset in rng.integers(0, count, size=max(1, round(1.2 * count / rate))):
            frequency = rng.uniform(700, 2200)
            phase = rng.uniform(0, 2 * np.pi)
            pulse = 10 ** (26 / 20) * noise_rms * envelope * np.sin(2 * np.pi * frequency * t + phase)
            add_overlap(audio, pulse, offset, 1)
    return audio
