"""Finite GNU Radio burst bench. Runs entirely in software with aligned samples."""

from dataclasses import dataclass
import math
import os
from pathlib import Path
import struct
import subprocess
import zlib

# Use generic kernels for reproducible rounding across scheduler chunk sizes.
os.environ.setdefault("VOLK_GENERIC", "1")

from gnuradio import blocks, gr
import numpy as np


BPSK_RATE = 8_000
BPSK_SAMPLES_PER_BIT = 32
BPSK_MAX_PAYLOAD = 4_096
BW500_RATE = 48_000
BW500_PAYLOAD_SIZES = (9, 22, 34)
LENGTH = struct.Struct(">H")
CRC = struct.Struct(">I")
GNU_RADIO_VERSION = gr.version()


@dataclass(frozen=True, slots=True, eq=False)
class EncodedBurst:
    """Transmit samples and the metadata needed to decode their received copy."""

    samples: np.ndarray
    sample_rate: int
    _payload_size: int
    _frame_size: int | None

    @property
    def airtime_s(self) -> float:
        return len(self.samples) / self.sample_rate


def _bpsk_samples(payload):
    frame = LENGTH.pack(len(payload)) + payload
    frame += CRC.pack(zlib.crc32(frame))
    bits = np.unpackbits(np.frombuffer(frame, dtype=np.uint8)).astype(np.float32)
    return np.repeat(2 * bits - 1, BPSK_SAMPLES_PER_BIT)


def _bpsk_receive(symbols):
    if len(symbols) % 8:
        return None
    wire = np.packbits(np.asarray(symbols) > 0).tobytes()
    if len(wire) < LENGTH.size + CRC.size:
        return None
    size, = LENGTH.unpack_from(wire)
    if len(wire) != LENGTH.size + size + CRC.size:
        return None
    frame, checksum = wire[:-CRC.size], wire[-CRC.size:]
    if CRC.pack(zlib.crc32(frame)) != checksum:
        return None
    return frame[LENGTH.size:]


def _flat_components(samples, *, seed, snr_db, fading_db, include_noise=True):
    rng = np.random.Generator(np.random.PCG64(seed))
    phase = rng.uniform(0, 2 * np.pi)
    cycles = rng.uniform(0.5, 1.5)
    time = np.arange(len(samples), dtype=np.float64) / len(samples)
    loss = fading_db * (1 + np.cos(2 * np.pi * cycles * time + phase)) / 2
    gain = np.power(10, -loss / 20)
    if not include_noise:
        return gain, None
    rms = math.sqrt(float(np.mean(samples.astype(np.float64) ** 2)))
    # Fix the sample sequence outside the scheduler. GNU Radio applies gain,
    # sums the noise, and integrates BPSK symbols within the flowgraph.
    noise = rng.normal(0, rms * 10 ** (-snr_db / 20), size=len(samples))
    return gain, noise


def _impair(samples, *, seed, snr_db, fading_db, integrate=None):
    gain, noise = _flat_components(samples, seed=seed, snr_db=snr_db, fading_db=fading_db)
    flow = gr.top_block()
    transmit = blocks.vector_source_f(samples.tolist())
    fading = blocks.vector_source_f(gain.tolist())
    gaussian = blocks.vector_source_f(noise.tolist())
    attenuate = blocks.multiply_ff()
    summer = blocks.add_ff()
    receive = blocks.vector_sink_f()
    flow.connect(transmit, attenuate, summer)
    flow.connect(fading, (attenuate, 1))
    flow.connect(gaussian, (summer, 1))
    if integrate is None:
        flow.connect(summer, receive)
    else:
        integrator = blocks.integrate_ff(integrate)
        flow.connect(summer, integrator, receive)
    flow.run()
    output = receive.data()
    expected = len(samples) if integrate is None else len(samples) // integrate
    if len(output) != expected:
        raise RuntimeError("GNU Radio returned an incomplete burst")
    return output


def _mix_audio(signal, noise, interference):
    components = [part for part in (signal, noise, interference) if part is not None]
    if len(components) == 1:
        return signal.copy()
    if any(part.shape != signal.shape for part in components[1:]):
        raise ValueError("receive signal, noise and interference must be aligned")
    flow = gr.top_block()
    summer = blocks.add_ff()
    sink = blocks.vector_sink_f()
    for i, part in enumerate(components):
        source = blocks.vector_source_f(part.tolist())
        flow.connect(source, (summer, i))
    flow.connect(summer, sink)
    flow.run()
    samples = np.asarray(sink.data(), dtype=np.float32)
    if samples.shape != signal.shape:
        raise RuntimeError("GNU Radio returned an incomplete mixed burst")
    return samples


def _integrate_bpsk(samples):
    flow = gr.top_block()
    sink = blocks.vector_sink_f()
    source = blocks.vector_source_f(samples.tolist())
    integrator = blocks.integrate_ff(BPSK_SAMPLES_PER_BIT)
    flow.connect(source, integrator, sink)
    flow.run()
    return sink.data()


def _float_wav(samples):
    pcm = np.asarray(samples, dtype="<f4").tobytes()
    # Match the mono IEEE-float WAV container emitted by the Linkwise CLI.
    return (
        b"RIFF" + struct.pack("<I", 48 + len(pcm)) + b"WAVEfmt "
        + struct.pack("<IHHIIHH", 16, 3, 1, BW500_RATE, BW500_RATE * 4, 4, 32)
        + b"fact" + struct.pack("<II", 4, len(samples))
        + b"data" + struct.pack("<I", len(pcm)) + pcm
    )


class Channel:
    """Return receiver bytes after GNU Radio processing, or None on frame rejection.

    The harness supplies isolated bursts and symbol alignment for BPSK.
    BW500 uses the Linkwise decoder on the finite received waveform. SNR is
    referenced to transmit power before fading. The flat model measures
    broadband power. The Watterson profile uses a defined audio reference
    band and a stable path key for continuous fading.
    """

    def __init__(self, modem=None, channel_model="flat"):
        if channel_model not in ("flat", "watterson-moderate"):
            raise ValueError(f"unknown channel model {channel_model!r}")
        if channel_model == "watterson-moderate" and modem is None:
            raise ValueError("the Watterson channel requires a native BW500 modem")
        self.modem = Path(modem).resolve() if modem is not None else None
        self.channel_model = channel_model
        self.sample_rate = BPSK_RATE if self.modem is None else BW500_RATE
        self.name = (
            "GNU Radio BPSK + CRC-32" if self.modem is None
            else "Linkwise BW500 + GNU Radio"
        )
        self._bw500_sample_counts = {}
        self._watterson = None
        if channel_model == "watterson-moderate":
            import watterson
            self._watterson = watterson

    def _check_size(self, size):
        limit = BPSK_MAX_PAYLOAD if self.modem is None else BW500_PAYLOAD_SIZES[-1]
        if not 1 <= size <= limit:
            raise ValueError(f"payload must contain 1 to {limit} bytes")

    def _modem_call(self, args, data):
        env = dict(os.environ)
        # Levels 1 through 3 require no external level-4 measurement profile.
        env.pop("LINKWISE_BW500_PROFILE", None)
        try:
            return subprocess.run(
                [self.modem, *args], input=data, capture_output=True,
                env=env, timeout=60, check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise RuntimeError(f"could not run Linkwise {args[0]}: {error}") from error

    def _bw500_samples(self, payload):
        level, size = next(
            (level, size)
            for level, size in enumerate(BW500_PAYLOAD_SIZES, 1)
            if len(payload) <= size
        )
        result = self._modem_call(
            ("encode-frame", str(level), "0", "-", "-", "pcm-f32le"),
            payload.ljust(size, b"\0"),
        )
        if result.returncode or not result.stdout or len(result.stdout) % 4:
            raise RuntimeError(f"Linkwise encode-frame failed: {result.stderr.decode(errors='replace')}")
        samples = np.frombuffer(result.stdout, dtype="<f4")
        self._bw500_sample_counts[size] = len(samples)
        return samples, size

    def airtime_s(self, payload_bytes: int) -> float:
        """Model burst duration from transmit samples at the mode's sample rate."""
        self._check_size(payload_bytes)
        if self.modem is None:
            wire_bytes = LENGTH.size + payload_bytes + CRC.size
            count = wire_bytes * 8 * BPSK_SAMPLES_PER_BIT
        else:
            size = next(size for size in BW500_PAYLOAD_SIZES if payload_bytes <= size)
            count = self._bw500_sample_counts.get(size)
            if count is None:
                samples, _ = self._bw500_samples(bytes(payload_bytes))
                count = len(samples)
        return count / self.sample_rate

    def prepare(self, payload: bytes) -> EncodedBurst:
        """Encode once so the same transmit samples can be viewed and received."""
        self._check_size(len(payload))
        if self.modem is None:
            samples, frame_size = _bpsk_samples(payload), None
        else:
            samples, frame_size = self._bw500_samples(payload)
        samples = np.asarray(samples, dtype=np.float32)
        samples.setflags(write=False)
        return EncodedBurst(samples, self.sample_rate, len(payload), frame_size)

    def _validate_channel(self, encoded, seed, snr_db, fading_db, started_at_s):
        if encoded.sample_rate != self.sample_rate:
            raise ValueError("encoded burst sample rate does not match the channel")
        if not (
            isinstance(seed, int) and seed >= 0 and -100 <= snr_db <= 100
            and 0 <= fading_db <= 120 and math.isfinite(started_at_s) and started_at_s >= 0
        ):
            raise ValueError("invalid seed, SNR, fading, or burst start")

    def reference_power(self, samples):
        """Clean input power in this model's SNR measurement bandwidth."""
        if self._watterson:
            return self._watterson.reference_power(samples)
        return float(np.mean(np.asarray(samples, dtype=np.float64) ** 2))

    def propagate(
        self, encoded: EncodedBurst, *, seed: int, snr_db: float, fading_db: float = 6.0,
        path_key=None, started_at_s: float = 0.0, include_noise: bool = True,
    ) -> tuple[np.ndarray, np.ndarray | None]:
        """Return start-aligned desired signal and independent background noise."""
        self._validate_channel(encoded, seed, snr_db, fading_db, started_at_s)
        if self._watterson:
            signal = self._watterson.propagate(
                encoded.samples, seed=seed, path_key=path_key, started_at_s=started_at_s,
            )
            noise = (
                self._watterson.background_noise(encoded.samples, len(signal), seed=seed, snr_db=snr_db)
                if include_noise else None
            )
            return signal, noise

        gain, noise = _flat_components(
            encoded.samples, seed=seed, snr_db=snr_db, fading_db=fading_db,
            include_noise=include_noise,
        )
        signal = encoded.samples * gain.astype(np.float32)
        return signal, noise.astype(np.float32) if noise is not None else None

    def _decode_bw500(self, encoded, output):
        result = self._modem_call(("decode-frame", "-"), _float_wav(output))
        if result.returncode:
            if result.returncode == 1 and b"expected exactly one CRC-valid frame" in result.stderr:
                return None
            raise RuntimeError(f"Linkwise decode-frame failed: {result.stderr.decode(errors='replace')}")
        if len(result.stdout) != encoded._frame_size or any(result.stdout[encoded._payload_size:]):
            return None
        return result.stdout[:encoded._payload_size]

    def decode_received(self, encoded, signal, noise, *, interference=None, capture=False):
        """Decode the combined receive waveform and optionally retain its exact samples."""
        if encoded.sample_rate != self.sample_rate:
            raise ValueError("encoded burst sample rate does not match the channel")
        signal = np.asarray(signal, dtype=np.float32)
        noise = None if noise is None else np.asarray(noise, dtype=np.float32)
        interference = None if interference is None else np.asarray(interference, dtype=np.float32)
        if signal.ndim != 1 or signal.size == 0:
            raise ValueError("expected a nonempty mono receive signal")
        samples = _mix_audio(signal, noise, interference)
        if self.modem is None:
            decoded = _bpsk_receive(_integrate_bpsk(samples))
        else:
            decoded = self._decode_bw500(encoded, samples)
        if not capture:
            return decoded

        desired = self.reference_power(signal)
        background = self.reference_power(noise) if noise is not None else 0.0
        overlap = self.reference_power(interference) if interference is not None else 0.0
        undesired = background + overlap
        samples.setflags(write=False)
        profile = self._watterson
        return decoded, {
            "samples": samples,
            "sample_rate_hz": self.sample_rate,
            "channel_model": self.channel_model,
            "profile": profile.PROFILE if profile else "Flat amplitude fading + AWGN",
            "measured_snr_db": 10 * math.log10(desired / background) if desired and background else None,
            "measured_sinr_db": 10 * math.log10(desired / undesired) if desired and undesired else None,
            "reference_band_hz": profile.REFERENCE_BAND_HZ if profile else (0, self.sample_rate / 2),
            "delay_samples": (0, profile.PATH_DELAY_SAMPLES) if profile else (0,),
            "doppler_spread_hz": profile.DOPPLER_SPREAD_HZ if profile else None,
        }

    def receive(
        self, encoded: EncodedBurst, *, seed: int, snr_db: float, fading_db: float = 6.0,
        path_key=None, started_at_s: float = 0.0, capture=False,
    ):
        if self._watterson or capture:
            signal, noise = self.propagate(
                encoded, seed=seed, snr_db=snr_db, fading_db=fading_db,
                path_key=path_key, started_at_s=started_at_s,
            )
            return self.decode_received(encoded, signal, noise, capture=capture)

        self._validate_channel(encoded, seed, snr_db, fading_db, started_at_s)
        bpsk = self.modem is None
        output = _impair(
            encoded.samples, seed=seed, snr_db=snr_db, fading_db=fading_db,
            integrate=BPSK_SAMPLES_PER_BIT if bpsk else None,
        )
        return _bpsk_receive(output) if bpsk else self._decode_bw500(encoded, output)

    def transfer(
        self, payload: bytes, *, seed: int, snr_db: float, fading_db: float = 6.0
    ) -> bytes | None:
        return self.receive(
            self.prepare(payload), seed=seed, snr_db=snr_db, fading_db=fading_db
        )
