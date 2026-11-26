"""One-sided amplitude spectra for the finite real-valued TX captures."""

import math

import numpy as np


FLOOR_DBFS = -120.0
MAX_ROWS = 96
MIN_AMPLITUDE = 10 ** (FLOOR_DBFS / 20)


def _dbfs(amplitudes):
    return np.round(20 * np.log10(np.maximum(amplitudes, MIN_AMPLITUDE)), 1)


def analyze(samples, sample_rate):
    """Return a Hann STFT and a linear-power average of its rows.

    A full-scale, bin-centred sinusoid has a 0 dBFS peak. DC and Nyquist
    use single-sided gain; other bins sum their two mirrored contributions.
    The spectrum includes all overlapping windows. The waterfall retains
    regularly spaced windows with times at their centres.
    """
    if not math.isfinite(sample_rate) or sample_rate <= 0:
        raise ValueError("sample rate must be finite and positive")
    audio = np.asarray(samples)
    if audio.ndim != 1 or audio.size == 0 or np.iscomplexobj(audio):
        raise ValueError("expected a nonempty mono real-valued capture")
    audio = np.asarray(audio, dtype=np.float64)
    if not np.isfinite(audio).all():
        raise ValueError("capture samples must be finite")

    # The 48 kHz mode retains the Rust modem demo's FFT resolution. A shorter
    # window gives the brief 8 kHz BPSK capture more time resolution.
    window_limit = 512 if sample_rate <= 16_000 else 2048
    fft_size = min(audio.size, window_limit)
    spectrum_hop = max(1, fft_size // 4)
    if fft_size <= 2:
        window = np.ones(fft_size)
        window_name = "rectangular"
    else:
        window = np.hanning(fft_size)
        window_name = "Hann"
    frames = np.lib.stride_tricks.sliding_window_view(audio, fft_size)[::spectrum_hop]
    amplitudes = np.abs(np.fft.rfft(frames * window, axis=-1)) / window.sum()
    if fft_size % 2:
        amplitudes[:, 1:] *= 2
    else:
        amplitudes[:, 1:-1] *= 2
    spectrum = _dbfs(np.sqrt(np.mean(amplitudes ** 2, axis=0)))
    waterfall_stride = math.ceil(len(frames) / MAX_ROWS)
    waterfall_hop = spectrum_hop * waterfall_stride
    waterfall = _dbfs(amplitudes[::waterfall_stride])
    peak_dbfs = float(_dbfs(amplitudes.max()))
    rows, bins = waterfall.shape

    return {
        "fft_size": fft_size,
        "hop_samples": waterfall_hop,
        "window": window_name,
        "spectrum_kind": "mean-power",
        "spectrum_rows": len(frames),
        "spectrum_hop_samples": spectrum_hop,
        "bins": bins,
        "rows": rows,
        "frequency_start_hz": 0.0,
        "bin_hz": sample_rate / fft_size,
        "time_start_s": fft_size / (2 * sample_rate),
        "time_step_s": waterfall_hop / sample_rate,
        "floor_dbfs": FLOOR_DBFS,
        "ceiling_dbfs": max(0.0, peak_dbfs),
        "spectrum_dbfs": spectrum.tolist(),
        "waterfall_dbfs": waterfall.ravel().tolist(),
    }
