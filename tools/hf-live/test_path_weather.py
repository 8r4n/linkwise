import csv
from datetime import datetime, timedelta, timezone
import hashlib
import json
from pathlib import Path
import random
from tempfile import TemporaryDirectory
import unittest

from drap import parse_drap
from path_forecast import FREQUENCIES, NUMERIC_HEADINGS, REVISION, PathForecast, report_rows
from space_weather import ReplayWeatherFeed, WeatherSnapshot
from stations import make_stations, path_conditions


START = "2026-11-26T00:00:00Z"
STATIONS = make_stations(2)
DUMMY_SHA = "0" * 64


def drap_data(time, haf, *, quiet=False):
    xray = "Normal X-ray Background" if quiet else "Moderate X-ray Flux"
    columns = " ".join([str(haf)] * 4)
    return f"""# DRAP Tabular Values
# Product Valid At : 2026-11-26 {time} UTC
# X-RAY Message : {xray}
# Proton Message : Normal Proton Background
-135 -45 45 135
-----------
 45 | {columns}
-45 | {columns}
""".encode()


def forecast_document():
    return {
        "schema": 1,
        "start_utc": START,
        "engine": {
            "name": "ITURHFProp", "revision": REVISION,
            "artifact_sha256": dict.fromkeys(("ITURHFProp", "libp533.so", "libp372.so"), DUMMY_SHA),
            "table_sha256": dict.fromkeys(("COEFF11W.txt", "ionos11.bin", "P1239-3 Decile Factors.txt"), DUMMY_SHA),
        },
        "assumptions": {
            "noise_bandwidth_hz": 3000, "noise_environment": "RESIDENTIAL", "antenna_model": "isotropic",
            "path_direction": "short", "tx_power_w": 100, "smoothed_sunspot_number": 83,
        },
        "stations": {site["id"]: [site["lat"], site["lon"]] for site in STATIONS},
        "predictions": [["SIM-001", "SIM-002", "40m", 0, 12, -107, 60, 7, 8, 5]],
    }


class PathForecastTests(unittest.TestCase):
    def test_strict_lookup_and_predictor_snr(self):
        forecast = PathForecast(forecast_document())
        source, target = STATIONS
        median, _, detail = path_conditions(source, target, "40m", 0, random.Random(12), forecast=forecast)
        self.assertEqual(median, 7)
        self.assertEqual(detail["forecast"]["status"], "predicted")
        self.assertEqual(detail["forecast"]["snr_p10_db"], -1)
        self.assertEqual(detail["forecast"]["snr_p90_db"], 12)
        self.assertEqual(detail["forecast"]["noise_bandwidth_hz"], 3000)
        self.assertIsNone(forecast.at(target, source, "40m", forecast.started_at))
        self.assertIsNone(forecast.at(dict(source, lat=0), target, "40m", forecast.started_at))
        self.assertIsNone(forecast.at(source, target, "40m", forecast.started_at + timedelta(days=6)))

    def test_missing_row_preserves_seeded_fallback(self):
        forecast = PathForecast(forecast_document())
        same_clock = WeatherSnapshot(forecast.started_at, None, None, (None, None), False)
        fallback = path_conditions(*STATIONS, "80m", 0, random.Random(12), same_clock)
        model = path_conditions(*STATIONS, "80m", 0, random.Random(12), forecast=forecast)
        self.assertEqual(fallback[:2], model[:2])
        self.assertEqual(model[2]["forecast"]["status"], "fallback")
        model = path_conditions(*STATIONS, "80m", 0, random.Random(2), slow_offset_db=2)
        fixed = path_conditions(*STATIONS, "80m", 0, random.Random(3), slow_offset_db=2)
        self.assertEqual(model[0], fixed[0])

    def test_accepts_only_pinned_3000_hz_manifest(self):
        document = forecast_document()
        document["assumptions"]["noise_bandwidth_hz"] = 500
        with self.assertRaisesRegex(ValueError, "3000 Hz"):
            PathForecast(document)
        document = forecast_document()
        document["predictions"].append(document["predictions"][0])
        with self.assertRaisesRegex(ValueError, "duplicate"):
            PathForecast(document)

    def test_utc_bins_from_upstream_csv(self):
        with TemporaryDirectory() as directory:
            report = Path(directory) / "report.csv"
            headers = ("month", "hour", "frequency", *NUMERIC_HEADINGS)
            with report.open("w", newline="") as output:
                write = csv.writer(output)
                write.writerow(headers)
                for hour in range(1, 25):
                    for frequency in FREQUENCIES.values():
                        write.writerow([11, hour, frequency, 9, -107, 135, hour + 0.5, 4, 7])
            records = report_rows(report, "SIM-001", "SIM-002", 11)
            self.assertEqual(len(records), 72)
            self.assertEqual(records[0], ["SIM-001", "SIM-002", "80m", 0, 9, -107, 135, 1.5, 4, 7])
            self.assertEqual(records[-1][3], 23)


class DrapReplayTests(unittest.TestCase):
    def test_haf_absorption_and_grid_validation(self):
        grid = parse_drap(drap_data("00:00", 10))
        self.assertEqual(grid.observed_at, datetime(2026, 11, 26, tzinfo=timezone.utc))
        self.assertAlmostEqual(grid.vertical_loss_db(10, 180, 10), 1)
        self.assertAlmostEqual(grid.vertical_loss_db(-10, -180, 5), 2 ** 1.5)
        with self.assertRaisesRegex(ValueError, "regular global grid"):
            parse_drap(drap_data("00:00", 10).replace(b"-135 -45 45 135", b"-135 -45 45 136"))

    def test_archive_respects_retrieval_and_freshness(self):
        with TemporaryDirectory() as directory:
            parent = Path(directory)
            products = [
                ("quiet.txt", "00:00:00", drap_data("00:00", 3, quiet=True)),
                ("event.txt", "00:04:00", drap_data("00:01", 12)),
            ]
            records = []
            for name, fetched, content in products:
                (parent / name).write_bytes(content)
                records.append({"product": "drap", "file": name, "fetched_at": f"2026-11-26T{fetched}Z",
                                "sha256": hashlib.sha256(content).hexdigest()})
            manifest = parent / "weather.json"
            manifest.write_text(json.dumps({"schema": 1, "start_utc": START, "records": records}))
            replay = ReplayWeatherFeed.load(manifest)
            self.assertEqual(replay.snapshot(60).drap_vertical_loss_db(2, 5, 7.1), 0)
            state = replay.snapshot(240)
            self.assertEqual(state.status()["origin"], "replay")
            self.assertEqual(state.status()["drap"]["quiet_reference_at"], START)
            self.assertAlmostEqual(
                state.drap_vertical_loss_db(2, 5, 7.1), (12 / 7.1) ** 1.5 - (3 / 7.1) ** 1.5,
            )
            forecast = PathForecast(forecast_document())
            snr, _, detail = path_conditions(*STATIONS, "40m", 240, random.Random(12), state, forecast=forecast)
            self.assertLess(snr, 7)
            self.assertEqual(detail["drap_quiet_reference_at"], START)
            self.assertEqual(replay.snapshot(970).status()["drap"]["status"], "stale")
            self.assertIsNone(replay.snapshot(970).drap_vertical_loss_db(2, 5, 7.1))
            (parent / "event.txt").write_bytes(b"wrong file")
            with self.assertRaisesRegex(ValueError, "digest mismatch"):
                ReplayWeatherFeed.load(manifest)


if __name__ == "__main__":
    unittest.main()
