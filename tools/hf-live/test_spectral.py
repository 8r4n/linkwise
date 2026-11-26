"""Check spectral axes, peak-amplitude calibration, and response bounds."""

import json
import unittest

import numpy as np

from spectral import FLOOR_DBFS, MAX_ROWS, analyze


class SpectralTests(unittest.TestCase):
    def test_bpsk_dimensions_and_bw500_tone_calibration(self):
        bpsk = analyze(np.ones(3840), 8000)
        self.assertEqual((bpsk["fft_size"], bpsk["rows"], bpsk["bins"]), (512, 27, 257))
        self.assertEqual((bpsk["frequency_start_hz"], bpsk["bin_hz"]), (0, 15.625))
        self.assertEqual((bpsk["time_start_s"], bpsk["time_step_s"]), (0.032, 0.016))
        self.assertEqual(bpsk["spectrum_dbfs"][0], 0)

        t = np.arange(258048) / 48000
        bw500 = analyze(0.5 * np.sin(2 * np.pi * 1500 * t), 48000)
        self.assertEqual(bw500["fft_size"], 2048)
        self.assertLessEqual(bw500["rows"], MAX_ROWS)
        self.assertEqual((bw500["spectrum_rows"], bw500["spectrum_hop_samples"]), (501, 512))
        self.assertEqual((bw500["rows"], bw500["hop_samples"]), (84, 3072))
        self.assertEqual(bw500["bins"], 1025)
        peak = int(np.argmax(bw500["spectrum_dbfs"]))
        self.assertEqual(peak * bw500["bin_hz"], 1500)
        self.assertEqual(bw500["spectrum_dbfs"][peak], -6.0)
        waterfall = np.asarray(bw500["waterfall_dbfs"]).reshape(bw500["rows"], bw500["bins"])
        np.testing.assert_array_equal(np.argmax(waterfall, axis=1), peak)
        np.testing.assert_array_equal(waterfall[:, peak], -6.0)
        json.dumps(bw500, allow_nan=False)

    def test_average_spectrum_includes_missing_waterfall_times(self):
        samples = np.zeros(258048)
        # Between the first two displayed windows (0–2047 and 3072–5119).
        samples[2304:2816] = 0.5 * np.sin(2 * np.pi * 1500 * np.arange(512) / 48000)
        analysis = analyze(samples, 48000)
        self.assertGreater(analysis["spectrum_dbfs"][64], FLOOR_DBFS)
        self.assertTrue(all(value == FLOOR_DBFS for value in analysis["waterfall_dbfs"]))

    def test_dc_nyquist_silence_and_short_input(self):
        samples = 0.5 + 0.25 * (-1.0) ** np.arange(512)
        values = analyze(samples, 8000)["spectrum_dbfs"]
        self.assertEqual(values[0], -6.0)
        self.assertEqual(values[-1], -12.0)
        self.assertEqual(analyze([1.0], 8000)["waterfall_dbfs"], [0.0])
        self.assertEqual(analyze([-1.0, 1.0], 8000)["waterfall_dbfs"], [FLOOR_DBFS, 0.0])
        self.assertEqual(analyze([0.0, 0.0, 0.0], 8000)["waterfall_dbfs"], [FLOOR_DBFS] * 2)

    def test_spectrum_averages_power_and_rejects_bad_inputs(self):
        samples = np.sin(2 * np.pi * np.arange(3840) / 16)
        samples[:1920] *= 0.1
        report = analyze(samples, 8000)
        waterfall = np.asarray(report["waterfall_dbfs"]).reshape(report["rows"], report["bins"])
        tone_rows = waterfall[:, 32]
        power_mean = 10 * np.log10(np.mean(10 ** (tone_rows / 10)))
        self.assertAlmostEqual(report["spectrum_dbfs"][32], power_mean, delta=0.1)
        self.assertGreater(report["spectrum_dbfs"][32], np.mean(tone_rows) + 5)
        for samples, rate in (([], 8000), ([np.nan], 8000), ([1 + 1j], 8000), ([1], 0)):
            with self.subTest(samples=samples, rate=rate), self.assertRaises(ValueError):
                analyze(samples, rate)


if __name__ == "__main__":
    unittest.main()
