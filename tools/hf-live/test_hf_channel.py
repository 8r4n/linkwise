"""Exercise actual finite GNU Radio bursts and optionally Linkwise BW500."""

import os
import struct
import unittest
from unittest.mock import patch

import numpy as np

from hf_channel import BPSK_SAMPLES_PER_BIT, Channel, _bpsk_receive, _bpsk_samples, _impair


PACKET = struct.pack(">IBBBH", 42, 0, 255, 33, 1024)


class BurstTests(unittest.TestCase):
    def test_bpsk_success_loss_and_airtime(self):
        channel = Channel()
        self.assertAlmostEqual(channel.airtime_s(len(PACKET)), 0.48)
        self.assertEqual(channel.transfer(PACKET, seed=17, snr_db=26), PACKET)
        self.assertIsNone(channel.transfer(PACKET, seed=17, snr_db=-80))

    def test_seed_and_crc(self):
        samples = _bpsk_samples(PACKET)
        options = dict(snr_db=10, fading_db=6, integrate=BPSK_SAMPLES_PER_BIT)
        first = _impair(samples, seed=17, **options)
        self.assertEqual(first, _impair(samples, seed=17, **options))
        self.assertNotEqual(first, _impair(samples, seed=18, **options))
        damaged = np.array(first)
        damaged[16] *= -1  # Flip the first payload bit, leaving the length and CRC intact.
        self.assertIsNone(_bpsk_receive(damaged))

    def test_bpsk_prepared_samples_are_used_for_reception(self):
        channel = Channel()
        burst = channel.prepare(PACKET)
        self.assertEqual(burst.sample_rate, 8_000)
        self.assertEqual(burst.airtime_s, channel.airtime_s(len(PACKET)))
        self.assertEqual(burst.samples.dtype, np.float32)
        self.assertFalse(burst.samples.flags.writeable)
        with patch("hf_channel._impair", wraps=_impair) as impair:
            self.assertEqual(channel.receive(burst, seed=17, snr_db=26), PACKET)
            self.assertIs(impair.call_args.args[0], burst.samples)

    @unittest.skipUnless(os.environ.get("LINKWISE_HF_TEST_MODEM"), "Linkwise CLI not configured")
    def test_bw500_success_loss_and_airtime(self):
        channel = Channel(os.environ["LINKWISE_HF_TEST_MODEM"])
        self.assertAlmostEqual(channel.airtime_s(len(PACKET)), 5.376)
        self.assertEqual(channel.transfer(PACKET, seed=17, snr_db=26), PACKET)
        burst = channel.prepare(PACKET)
        self.assertEqual(burst.sample_rate, 48_000)
        self.assertEqual(burst.airtime_s, channel.airtime_s(len(PACKET)))
        self.assertFalse(burst.samples.flags.writeable)
        with patch("hf_channel._impair", wraps=_impair) as impair:
            self.assertIsNone(channel.receive(burst, seed=17, snr_db=-80))
            self.assertIs(impair.call_args.args[0], burst.samples)


if __name__ == "__main__":
    unittest.main()
