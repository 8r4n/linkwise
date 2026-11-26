"""Gaussian scatter statistics, calibration and native decoder integration."""

import os
import struct
import unittest
from unittest.mock import patch

import numpy as np

from calibrate import measure_taps, reference_noise_check
from hf_channel import Channel
import watterson


PACKET = struct.pack(">IBBBH", 42, 0, 255, 33, 1024)


class WattersonTests(unittest.TestCase):
    def test_exact_complex_path_delays_and_tail(self):
        impulse = np.zeros(128, dtype=np.complex64)
        impulse[-1] = 1 + 2j
        length = len(impulse) + watterson.PATH_DELAY_SAMPLES
        direct = np.full(length, 0.5j, dtype=np.complex64)
        echo = np.full(length, 0.25 + 0.5j, dtype=np.complex64)
        received = watterson._sum_paths(impulse, (direct, echo))
        expected = np.zeros(length, dtype=np.float32)
        expected[len(impulse) - 1] = -1
        expected[-1] = -0.75
        np.testing.assert_array_equal(received, expected)

    def test_keyed_time_slices_and_independent_paths(self):
        key = (42, "SIM-003", "SIM-007", "40m")
        full = watterson.coefficients(498, 125, seed=1, path_key=key)
        cut = watterson.coefficients(500, 15, seed=925, path_key=key)
        for entire, overlap in zip(full, cut):
            np.testing.assert_array_equal(entire[2:17], overlap)
        alternative, _ = watterson.coefficients(500, 15, seed=1, path_key=(*key, "other"))
        self.assertFalse(np.array_equal(alternative, cut[0]))
        at_audio_rate = watterson._sampled_coefficients(48000, 3000, seed=1, path_key=key)
        offset_audio_rate = watterson._sampled_coefficients(49501, 700, seed=2, path_key=key)
        for entire, overlap in zip(at_audio_rate, offset_audio_rate):
            np.testing.assert_array_equal(entire[1501:2201], overlap)

    def test_gaussian_doppler_and_power(self):
        rate = watterson.COEFFICIENT_RATE
        for seed in (17, 42):
            taps = watterson.coefficients(0, rate * 1200, seed=seed)
            metrics = measure_taps(taps)
            for tap, observed in zip(taps, metrics["taps"]):
                self.assertAlmostEqual(observed["mean_power"], 0.5, delta=0.05)
                self.assertAlmostEqual(observed["mean_doppler_hz"], 0, delta=0.02)
                self.assertAlmostEqual(observed["doppler_spread_2sigma_hz"], 0.5, delta=0.05)
                self.assertLess(abs(np.corrcoef(tap.real, tap.imag)[0, 1]), 0.08)
            self.assertLess(metrics["power_imbalance_db"], 0.25)
            self.assertLess(metrics["peak_normalized_cross_correlation"], 0.15)

    def test_reference_noise_band_and_calibration(self):
        rate = watterson.SAMPLE_RATE
        t = np.arange(4 * rate) / rate
        tone, noise, measured = reference_noise_check()
        out_of_band = np.sin(2 * np.pi * 10000 * t).astype(np.float32)
        self.assertLess(watterson.reference_power(out_of_band), 1e-5)
        np.testing.assert_array_equal(
            watterson.background_noise(tone, len(tone), seed=42, snr_db=17), noise,
        )
        self.assertAlmostEqual(measured, 17, delta=0.25)
        self.assertEqual(noise.dtype, np.float32)

    @unittest.skipUnless(os.environ.get("LINKWISE_HF_TEST_MODEM"), "Linkwise CLI not configured")
    def test_bw500_exact_capture_and_interference_measurement(self):
        channel = Channel(os.environ["LINKWISE_HF_TEST_MODEM"], "watterson-moderate")
        burst = channel.prepare(PACKET)
        original = burst.samples.tobytes()
        signal, noise = channel.propagate(burst, seed=17, snr_db=26)
        noise_free, omitted = channel.propagate(burst, seed=17, snr_db=26, include_noise=False)
        self.assertEqual(signal.dtype, np.float32)
        self.assertEqual(len(signal), len(burst.samples) + 48)
        self.assertIsNone(omitted)
        np.testing.assert_array_equal(signal, noise_free)

        interference = signal * 0.01
        with patch.object(channel, "_modem_call", wraps=channel._modem_call) as modem:
            decoded, proof = channel.decode_received(
                burst, signal, noise, interference=interference, capture=True,
            )
            command, wav = modem.call_args.args
        self.assertEqual(command, ("decode-frame", "-"))
        self.assertEqual(decoded, PACKET)
        self.assertEqual(proof["samples"].tobytes(), wav[56:])
        self.assertEqual(proof["delay_samples"], (0, 48))
        self.assertEqual(proof["reference_band_hz"], (300, 3300))
        self.assertEqual(proof["doppler_spread_hz"], 0.5)
        self.assertLess(proof["measured_sinr_db"], proof["measured_snr_db"])
        self.assertFalse(proof["samples"].flags.writeable)
        self.assertEqual(burst.samples.tobytes(), original)
        self.assertAlmostEqual(burst.airtime_s, 5.376)
        self.assertIsNone(channel.receive(burst, seed=17, snr_db=-80))


if __name__ == "__main__":
    unittest.main()
