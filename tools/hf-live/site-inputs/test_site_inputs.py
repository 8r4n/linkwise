import json
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

import build
from path_forecast import REVISION


class SiteInputsTests(unittest.TestCase):
    def test_model_staging_and_browser_fallback(self):
        with TemporaryDirectory() as temporary:
            directory = Path(temporary)
            build.stage_modules(directory)
            forecast = directory / "native-cache.json"
            digest = "a" * 64
            forecast.write_text(json.dumps({
                "schema": 1, "start_utc": "2026-11-26T13:00:00Z",
                "engine": {"name": "ITURHFProp", "revision": REVISION,
                           "artifact_sha256": dict.fromkeys(("ITURHFProp", "libp533.so", "libp372.so"), digest),
                           "table_sha256": dict.fromkeys(("COEFF11W.txt", "ionos11.bin", "P1239-3 Decile Factors.txt"), digest)},
                "assumptions": {"noise_bandwidth_hz": 3000, "noise_environment": "RESIDENTIAL",
                                "antenna_model": "isotropic", "path_direction": "short", "tx_power_w": 100,
                                "smoothed_sunspot_number": 83},
                "stations": {site["id"]: [site["lat"], site["lon"]] for site in build.SITES},
                "predictions": [
                    ["SIM-001", "SIM-002", "40m", 4, 10.05, -103, 52, 2.25, 14.75, 9.5],
                    ["SIM-001", "SIM-002", "40m", 13, 8.6, -400, 57, -387.24, 15.47, 9.4],
                ],
            }))
            build.stage_forecast(forecast, directory)
            static = json.loads((directory / "forecast.json").read_text())
            self.assertEqual(static["rows"], 2)
            self.assertEqual(static["byte_length"], 1624320)
            self.assertEqual((directory / static["file"]).stat().st_size, static["byte_length"])
            node = shutil.which("node")
            if not node:
                self.skipTest("Node is required to test the browser module")
            subprocess.run(
                [node, str(Path(__file__).with_name("test-site-browser.mjs")), str(directory)],
                check=True, timeout=20,
            )

    def test_noaa_outage_preserves_the_archive(self):
        with TemporaryDirectory() as temporary:
            directory = Path(temporary)
            path = directory / "weather.json"
            archived = {"schema": 1, "drap": {"observed_at": "2026-11-26T04:00:00Z"}}
            build.save_json(path, archived)
            with (
                patch.object(build, "_fetch_grid", side_effect=TimeoutError()),
                patch.object(build, "_fetch_kp", side_effect=TimeoutError()),
                patch.object(build, "_fetch_drap", side_effect=TimeoutError()),
            ):
                build.refresh_weather(directory)
            self.assertEqual(json.loads(path.read_text()), archived)


if __name__ == "__main__":
    unittest.main()
