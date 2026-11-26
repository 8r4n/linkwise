"""Seeded two-path, mid-latitude moderate Gaussian-scatter HF channel."""

import hashlib
import json
import math

import numpy as np
from scipy.signal import fftconvolve, firwin, hilbert


SAMPLE_RATE = 48_000
PATH_DELAY_SAMPLES = 48
DOPPLER_SPREAD_HZ = 0.5  # ITU-R F.1487 Annex 3 convention, two standard deviations.
REFERENCE_BAND_HZ = (300, 3300)
COEFFICIENT_RATE = 32
SAMPLES_PER_COEFFICIENT = SAMPLE_RATE // COEFFICIENT_RATE
WHITE_BLOCK = 512
FILTER_RADIUS = 4 * COEFFICIENT_RATE
ANALYTIC_GUARD = 256
PROFILE = "ITU-R F.1487 mid-latitude moderate"

# Square root of the Gaussian Doppler power spectrum in the frequency
# domain corresponds to this Gaussian impulse response. Its unit L2 norm
# preserves the ensemble noise power without normalizing individual bursts.
_TIME = np.arange(-FILTER_RADIUS, FILTER_RADIUS + 1) / COEFFICIENT_RATE
_GAUSSIAN_KERNEL = np.exp(-(np.pi * DOPPLER_SPREAD_HZ * _TIME) ** 2)
_GAUSSIAN_KERNEL /= np.linalg.norm(_GAUSSIAN_KERNEL)
_AUDIO_FILTER = firwin(513, REFERENCE_BAND_HZ, pass_zero=False, fs=SAMPLE_RATE).astype(np.float32)
# Noise passes through the shaping filter and then the measurement filter.
_NOISE_GAIN = float(np.sum(np.convolve(_AUDIO_FILTER, _AUDIO_FILTER).astype(np.float64) ** 2))


def _key(seed, path_key):
    # A stable link key decouples propagation from the attempt-specific noise
    # seed. Signed blocks let the FIR warm up before virtual time zero.
    key = ("burst", seed) if path_key is None else ("path", path_key)
    return json.dumps(key, separators=(",", ":"), sort_keys=True).encode()


def _white_block(key, block):
    suffix = block.to_bytes(8, "little", signed=True)
    block_seed = int.from_bytes(hashlib.sha256(key + suffix).digest()[:16], "little")
    return np.random.default_rng(block_seed).standard_normal((WHITE_BLOCK, 4))


def coefficients(first, count, *, seed, path_key=None):
    """Return both complex path gains on the absolute 32 Hz coefficient grid."""
    key = _key(seed, path_key)
    begin = first - FILTER_RADIUS
    stop = first + count + FILTER_RADIUS
    first_block, offset = divmod(begin, WHITE_BLOCK)
    last_block = (stop - 1) // WHITE_BLOCK
    white = np.concatenate([_white_block(key, block) for block in range(first_block, last_block + 1)])
    white = white[offset:offset + stop - begin]
    shaped = [
        np.convolve(white[:, component], _GAUSSIAN_KERNEL, mode="valid")
        for component in range(4)
    ]
    # Real and imaginary variances are each 1/4, hence E[|tap|²] = 1/2.
    return (shaped[0] + 1j * shaped[1]) / 2, (shaped[2] + 1j * shaped[3]) / 2


def _sampled_coefficients(start, count, *, seed, path_key):
    first = start // SAMPLES_PER_COEFFICIENT
    last = (start + count - 1) // SAMPLES_PER_COEFFICIENT + 1
    taps = coefficients(first, last - first + 1, seed=seed, path_key=path_key)
    # Interpolate the slow fading at exact sample positions. Independently
    # requested, overlapping bursts then share the same channel phase.
    indices = np.arange(start, start + count, dtype=np.int64)
    left, remainder = np.divmod(indices, SAMPLES_PER_COEFFICIENT)
    left -= first
    fraction = remainder / SAMPLES_PER_COEFFICIENT
    return tuple(
        (tap[left] + fraction * (tap[left + 1] - tap[left])).astype(np.complex64)
        for tap in taps
    )


def _sum_paths(analytic, taps):
    count = len(analytic)
    direct, echo = taps
    received = np.zeros(count + PATH_DELAY_SAMPLES, dtype=np.float32)
    received[:count] = (analytic * direct[:count]).real
    received[PATH_DELAY_SAMPLES:] += (analytic * echo[PATH_DELAY_SAMPLES:]).real
    return received


def propagate(samples, *, seed, path_key, started_at_s):
    """Return real receive audio at the TX origin, including the delayed tail."""
    guarded = np.pad(samples, ANALYTIC_GUARD)
    analytic = hilbert(guarded)[ANALYTIC_GUARD:ANALYTIC_GUARD + len(samples)]
    start = round(started_at_s * SAMPLE_RATE)
    taps = _sampled_coefficients(
        start, len(samples) + PATH_DELAY_SAMPLES, seed=seed, path_key=path_key
    )
    return _sum_paths(analytic, taps)


def reference_power(samples):
    """Mean square measured through the declared receiver reference band."""
    in_band = fftconvolve(samples, _AUDIO_FILTER, mode="same")
    return float(np.mean(in_band.astype(np.float64) ** 2))


def effective_noise_bandwidth_hz():
    center = sum(REFERENCE_BAND_HZ) / 2
    response = np.dot(_AUDIO_FILTER, np.exp(-2j * np.pi * center * np.arange(len(_AUDIO_FILTER)) / SAMPLE_RATE))
    return float(SAMPLE_RATE / 2 * np.sum(_AUDIO_FILTER.astype(np.float64) ** 2) / abs(response) ** 2)


def background_noise(samples, output_count, *, seed, snr_db):
    desired = reference_power(samples) * 10 ** (-snr_db / 10)
    rng = np.random.default_rng(seed)
    guard = len(_AUDIO_FILTER) // 2
    white = rng.standard_normal(output_count + 2 * guard, dtype=np.float32)
    noise = fftconvolve(white, _AUDIO_FILTER, mode="valid")
    noise *= math.sqrt(desired / _NOISE_GAIN)
    return noise
