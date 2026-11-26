"""Exercise the scheduler, same-band mixing, and matched capture API."""

import base64
from http.client import HTTPConnection
from http.server import ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import unittest

import numpy as np

from hf_channel import Channel
from medium import add_overlap, injected_interference
from serve import EventStream, Handler
from simulator import Simulation, WaveformPair


class MediumTests(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("LINKWISE_HF_TEST_MODEM"), "Linkwise CLI not configured")
    def test_cli_defaults_native_bw500_to_watterson(self):
        result = subprocess.check_output([
            sys.executable, str(Path(__file__).with_name("serve.py")), "--headless",
            "--duration", "0.1", "--stations", "2", "--modem", os.environ["LINKWISE_HF_TEST_MODEM"],
        ])
        self.assertEqual(json.loads(result)["channel_model"], "watterson-moderate")

    def test_overlap_places_both_edges_and_never_wraps(self):
        signal = np.zeros(8, dtype=np.float32)
        add_overlap(signal, np.array([1, 2, 3, 4]), -2, 2)
        add_overlap(signal, np.array([1, 2, 3, 4]), 6, 3)
        add_overlap(signal, np.array([100]), 8, 1)
        np.testing.assert_array_equal(signal, [6, 8, 0, 0, 0, 0, 3, 6])

    def test_receiver_interference_is_seeded_and_tone_is_time_aligned(self):
        options = dict(rate=8000, count=8000, start_s=0, noise_power=0.1, seed=13)
        mixed = injected_interference("mixed", **options)
        np.testing.assert_array_equal(mixed, injected_interference("mixed", **options))
        tone = injected_interference("tone", **options)
        offset = injected_interference("tone", **{**options, "start_s": 0.25})
        np.testing.assert_allclose(tone[2000:], offset[:6000], atol=1e-6)
        self.assertFalse(np.array_equal(mixed, tone))

    def test_shared_band_records_both_observations_and_reserves_stations(self):
        simulator = Simulation(Channel(), 24, 42, "40m", medium="shared")
        attempts = {}
        for tick in range(505):
            for kind, event in simulator.advance(tick, 50):
                attempt = event["attempt"]
                if kind == "attempt" and attempt["state"] != "active":
                    attempts[attempt["id"]] = attempt
            endpoints = [station for burst in simulator.active.values()
                         for station in (burst.message.source, burst.message.target)]
            self.assertEqual(len(endpoints), len(set(endpoints)))
            self.assertLessEqual(len(simulator.active), 2)
        self.assertGreater(simulator.stats()["overlapped"], 0)
        for attempt in attempts.values():
            for other in attempt["interferers"]:
                if other in attempts:
                    self.assertIn(attempt["id"], attempts[other]["interferers"])
                self.assertLessEqual(attempt["measured_sinr_db"], attempt["measured_snr_db"])

    @unittest.skipUnless(os.environ.get("LINKWISE_HF_TEST_MODEM"), "Linkwise CLI not configured")
    def test_native_decoder_capture_under_cochannel_interference(self):
        channel = Channel(os.environ["LINKWISE_HF_TEST_MODEM"], channel_model="watterson-moderate")
        expected = b"wanted--1"
        wanted = channel.prepare(expected)
        other = channel.prepare(b"another-2")
        wanted_audio, noise = channel.propagate(wanted, seed=42, snr_db=30, path_key="wanted")
        other_audio, _ = channel.propagate(other, seed=7, snr_db=30, path_key="other", include_noise=False)
        weak = np.zeros_like(wanted_audio)
        add_overlap(weak, other_audio, -4_800, 0.01)
        received, capture = channel.decode_received(wanted, wanted_audio, noise, interference=weak, capture=True)
        self.assertEqual(received, expected)
        self.assertLess(capture["measured_sinr_db"], capture["measured_snr_db"])
        strong = np.zeros_like(wanted_audio)
        add_overlap(strong, other_audio, -4_800, 20)
        self.assertNotEqual(channel.decode_received(wanted, wanted_audio, noise, interference=strong), expected)


class CaptureTests(unittest.TestCase):
    def test_worker_fault_marks_unfinished_tx_as_aborted(self):
        simulator = Simulation(Channel(), 6, 42)
        for tick in range(40):
            simulator.advance(tick)
            if simulator.active:
                break
        attempt = next(iter(simulator.active.values())).event
        simulator.stop_with_error(RuntimeError("decoder failed"))
        pair = simulator.waveforms[attempt["source"]]
        self.assertEqual(pair.attempt["decode_status"], "aborted")
        self.assertIsNone(pair.receiver)
        self.assertEqual(simulator.stats()["active"], 0)
        self.assertGreater(simulator.stats()["aborted"], 0)

    def test_http_retains_attempt_id_and_delivers_exact_rx_pcm(self):
        channel = Channel()
        encoded = channel.prepare(b"wanted--1")
        _, received = channel.receive(encoded, seed=42, snr_db=20, capture=True)
        active = {"id": 2, "source": "SIM-001", "decode_status": "pending",
                  "channel_model": "flat", "snr_db": 20}
        completed = {**active, "id": 1, "decode_status": "delivered"}
        stream = EventStream({})
        stream.publish({}, [], {"SIM-001": WaveformPair(active, encoded)}, [WaveformPair(completed, encoded, received)])
        with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
            server.stream = stream
            worker = threading.Thread(target=server.serve_forever, daemon=True)
            worker.start()
            conn = HTTPConnection("127.0.0.1", server.server_port, timeout=5)
            self.addCleanup(conn.close)
            self.addCleanup(worker.join)
            self.addCleanup(server.shutdown)

            def fetch(query):
                conn.request("GET", "/waveforms/SIM-001" + query)
                response = conn.getresponse()
                return response.status, response.read()

            self.assertEqual(fetch("?stage=rx&attempt=2")[0], 204)
            self.assertEqual(fetch("?stage=tx&after=2")[0], 204)
            self.assertEqual(fetch("?stage=rx&attempt=42")[0], 404)
            self.assertEqual(fetch("?stage=invalid")[0], 400)
            status, body = fetch("?stage=rx&attempt=1")
            self.assertEqual(status, 200)
            result = json.loads(body)
            self.assertEqual((result["attempt"]["id"], result["stage"]), (1, "rx"))
            self.assertEqual(base64.b64decode(result["waveform_f32le"]), received["samples"].astype("<f4").tobytes())
            status, body = fetch("")
            self.assertEqual(status, 200)
            self.assertEqual((json.loads(body)["attempt"]["id"], json.loads(body)["stage"]), (2, "tx"))


if __name__ == "__main__":
    unittest.main()
