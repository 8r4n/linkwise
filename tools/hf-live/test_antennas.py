"""Verify geometry and antenna gains through the native receive mix."""

import json
import math
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

import numpy as np

from antennas import Mount, link_budget, load_catalog, load_configuration, path_geometry, pattern_gain
from hf_channel import Channel
from simulator import Message, Simulation
from stations import make_stations


class AntennaModelTests(unittest.TestCase):
    def test_shared_geometry_and_gain_vector(self):
        stations = make_stations(48)
        geo = path_geometry(stations[21], stations[37])
        self.assertAlmostEqual(geo.distance_km, 1120.9979000318604)
        self.assertAlmostEqual(geo.tx_bearing_deg, 323.9471407711477)
        self.assertAlmostEqual(geo.rx_bearing_deg, 139.73227445319918)
        self.assertAlmostEqual(geo.elevation_deg, 25.07069398255475)
        catalog = load_catalog()
        rx = Mount("yagi-20m", geo.rx_bearing_deg)
        front = link_budget(catalog, geo, "20m", Mount("yagi-20m", geo.tx_bearing_deg), rx, 100)
        rear = link_budget(catalog, geo, "20m", Mount("yagi-20m", (geo.tx_bearing_deg + 180) % 360), rx, 100)
        self.assertAlmostEqual(front.net_gain_db, 14.796004463089849)
        self.assertAlmostEqual(front.net_gain_db - rear.net_gain_db, 20)
        self.assertEqual((front.tx.feedline_loss_db, front.rx.feedline_loss_db), (0.5, 0.5))
        self.assertAlmostEqual(front.eirp_w, 100 * 10 ** ((front.tx.gain_dbi - 0.5) / 10))
        self.assertEqual(front.rx, rear.rx)
        self.assertAlmostEqual(front.eirp_w / rear.eirp_w, 100)
        dipole = catalog["dipole-hf"]
        self.assertEqual(pattern_gain(dipole, "20m", 0, 30, 0), pattern_gain(dipole, "20m", 0, 30, 180))
        self.assertAlmostEqual(pattern_gain(dipole, "20m", 44, 30, 0), 1.5)

    def test_neutral_roles_coverage_polarization_and_wrapping(self):
        geo = path_geometry({"lat": 0, "lon": 170}, {"lat": 0, "lon": -170})
        self.assertAlmostEqual(geo.tx_bearing_deg, 90)
        self.assertAlmostEqual(geo.rx_bearing_deg, 270)
        circular = path_geometry({"lat": 0, "lon": 0}, {"lat": 0, "lon": 180})
        self.assertIsNone(circular.tx_bearing_deg)
        self.assertIsNone(circular.rx_bearing_deg)
        catalog = load_catalog()
        neutral = link_budget(catalog, geo, "80m", Mount(), Mount(), 37)
        self.assertEqual((neutral.net_gain_db, neutral.eirp_w), (0, 37))
        receive_only = link_budget(catalog, geo, "80m", Mount(), Mount("tci-530"), 37)
        self.assertEqual(receive_only.eirp_w, neutral.eirp_w)
        self.assertAlmostEqual(receive_only.net_gain_db, receive_only.rx.gain_dbi - receive_only.rx.feedline_loss_db)
        self.assertIsNone(link_budget(catalog, geo, "80m", Mount(), Mount("tci-527b"), 100))
        self.assertIsNone(link_budget(catalog, geo, "20m", Mount("active-loop"), Mount(), 100))
        cross = link_budget(catalog, geo, "40m", Mount("vertical-hf"), Mount("dipole-hf"), 100)
        self.assertEqual(cross.polarization_loss_db, 3)

    def test_config_rejects_receive_only_transmitters_and_unknown_ids(self):
        with TemporaryDirectory() as temporary:
            path = Path(temporary) / "config.json"
            path.write_text(json.dumps({
                "schema": 1, "stations": {"SIM-001": {"tx": {"profile_id": "active-loop"}}},
            }))
            with self.assertRaisesRegex(ValueError, "receive-only"):
                load_configuration(make_stations(2), path=path)
            path.write_text(json.dumps({"schema": 1, "stations": {"SIM-999": {}}}))
            with self.assertRaisesRegex(ValueError, "SIM-999"):
                load_configuration(make_stations(2), path=path)


class AntennaReceiveTests(unittest.TestCase):
    stations = make_stations(48)

    @staticmethod
    def mount(profile, heading):
        return {"profile_id": profile, "heading_deg": heading % 360}

    def configuration(self, stations):
        with TemporaryDirectory() as temporary:
            path = Path(temporary) / "antennas.json"
            path.write_text(json.dumps({"schema": 1, "stations": stations}))
            return load_configuration(self.stations, path=path)

    def start(self, channel, configuration, overlap=False):
        simulation = Simulation(channel, 48, 42, "20m", medium="shared", antennas=configuration)
        simulation.queues[21].append(Message(1, 21, 37, 10, 0))
        list(simulation._start())
        wanted = simulation.active[1]
        if overlap:
            simulation.next_access["20m"] = 0
            simulation.queues[1].append(Message(2, 1, 7, 20, 0))
            list(simulation._start())
            self.assertEqual([item.attempt_id for item in wanted.overlaps], [2])
        return simulation, wanted

    def receive(self, simulation, burst):
        with patch.object(simulation.channel, "decode_received", wraps=simulation.channel.decode_received) as decoder:
            _, capture = simulation._receive(burst)
        _, signal, noise = decoder.call_args.args
        return signal, noise, decoder.call_args.kwargs.get("interference"), capture

    def check_signal_gain(self, channel):
        geo = path_geometry(self.stations[21], self.stations[37])
        neutral = self.start(channel, None)
        isotropic = self.start(channel, self.configuration({}))
        boosted = self.start(channel, self.configuration({
            "SIM-022": {"tx": self.mount("yagi-20m", geo.tx_bearing_deg), "power_w": 200},
            "SIM-038": {"rx": self.mount("yagi-20m", geo.rx_bearing_deg)},
        }))
        signal, noise, _, unmodified = self.receive(*neutral)
        _, _, _, same = self.receive(*isotropic)
        np.testing.assert_array_equal(unmodified["samples"], same["samples"])
        changed_signal, changed_noise, _, changed = self.receive(*boosted)
        gain = boosted[1].antenna.net_gain_db + 10 * math.log10(2)
        self.assertEqual(boosted[1].event["reference_snr_db"], neutral[1].event["snr_db"])
        np.testing.assert_array_equal(noise, changed_noise)
        np.testing.assert_allclose(changed_signal, signal * 10 ** (gain / 20), rtol=1e-6)
        self.assertAlmostEqual(changed["measured_snr_db"] - unmodified["measured_snr_db"], gain, places=4)

    def test_signal_and_fixed_noise_with_gnu_radio(self):
        self.check_signal_gain(Channel())

    @unittest.skipUnless(os.environ.get("LINKWISE_HF_TEST_MODEM"), "Linkwise CLI not configured")
    def test_signal_and_fixed_noise_with_native_bw500(self):
        self.check_signal_gain(Channel(os.environ["LINKWISE_HF_TEST_MODEM"], channel_model="watterson-moderate"))

    def test_overlap_uses_frozen_interferer_tx_and_victim_rx(self):
        channel = Channel()
        victim = path_geometry(self.stations[21], self.stations[37])
        cochannel = path_geometry(self.stations[1], self.stations[37])
        rx = self.mount("yagi-20m", victim.rx_bearing_deg)
        front = self.configuration({
            "SIM-002": {"tx": self.mount("yagi-20m", cochannel.tx_bearing_deg)},
            "SIM-038": {"rx": rx},
        })
        rear = self.configuration({
            "SIM-002": {"tx": self.mount("yagi-20m", cochannel.tx_bearing_deg + 180)},
            "SIM-038": {"rx": rx},
        })
        first, wanted = self.start(channel, front, overlap=True)
        second, other = self.start(channel, rear, overlap=True)
        first.antennas = rear  # Only future mounts can change.
        desired1, noise1, interference1, captured1 = self.receive(first, wanted)
        desired2, noise2, interference2, captured2 = self.receive(second, other)
        np.testing.assert_array_equal(desired1, desired2)
        np.testing.assert_array_equal(noise1, noise2)
        np.testing.assert_allclose(interference1, interference2 * 10, rtol=1e-6)
        reported_front = captured1["antenna_interferers"][0]["antenna"]
        reported_rear = captured2["antenna_interferers"][0]["antenna"]
        self.assertEqual(reported_front["rx"]["heading_deg"], victim.rx_bearing_deg)
        self.assertEqual(reported_front["tx"]["heading_deg"], cochannel.tx_bearing_deg)
        self.assertAlmostEqual(reported_front["net_gain_db"] - reported_rear["net_gain_db"], 20)
        self.assertLess(captured1["measured_sinr_db"], captured2["measured_sinr_db"])

    def test_cli_blocks_unsupported_band(self):
        with TemporaryDirectory() as temporary:
            path = Path(temporary) / "antennas.json"
            config = {"tx": self.mount("yagi-20m", 0)}
            path.write_text(json.dumps({"schema": 1, "stations": {"SIM-001": config, "SIM-002": config}}))
            result = subprocess.check_output([
                sys.executable, str(Path(__file__).with_name("serve.py")), "--headless", "--duration", "10",
                "--stations", "2", "--band", "80m", "--antenna-config", str(path),
            ])
        snapshot = json.loads(result)
        self.assertEqual(snapshot["stats"]["attempted"], 0)
        self.assertGreater(snapshot["stats"]["unsupported_antenna_paths"], 0)
        self.assertEqual(snapshot["antenna_model"], "configured")
        self.assertTrue(snapshot["antennas"]["enabled"])
        self.assertIn("yagi-20m", [profile["id"] for profile in snapshot["antennas"]["catalog"]["profiles"]])
        self.assertEqual(snapshot["antennas"]["stations"][0], {
            "station": "SIM-001", "txPort": "yagi-20m", "rxPort": "isotropic", "txHeading": 0, "rxHeading": 0,
        })


if __name__ == "__main__":
    unittest.main()
