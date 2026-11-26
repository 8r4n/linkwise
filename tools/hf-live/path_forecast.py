"""Generate and load immutable path-hour predictions from upstream ITURHFProp."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import csv
from dataclasses import dataclass
import hashlib
from itertools import permutations
import json
import math
import os
from pathlib import Path
import re
import subprocess
from tempfile import TemporaryDirectory

from space_weather import replay_timestamp
from stations import BANDS, make_stations


UPSTREAM = "https://github.com/ITU-R-Study-Group-3/ITU-R-HF"
REVISION = "cd172be56dc04b154e5d2fa91cbaa6ecf5284305"
RELEASE = "v14.3"
NOISE_BANDWIDTH_HZ = 3000
NOAA_SOLAR_CYCLE_URL = "https://services.swpc.noaa.gov/json/solar-cycle/predicted-solar-cycle.json"
ENVIRONMENTS = ("CITY", "RESIDENTIAL", "RURAL", "QUIETRURAL", "QUIET", "NOISY")
FREQUENCIES = {band["id"]: band["frequency_mhz"] for band in BANDS}
NUMERIC_HEADINGS = ("BMUF", "PR", "FamT", "SNR", "DlSN", "DuSN")
REPORT = "RPT_BMUF | RPT_PR | RPT_NOISETOTAL | RPT_SNR | RPT_SNRD"
_SHA256 = re.compile(r"[0-9a-f]{64}")


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as data:
        for chunk in iter(lambda: data.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


@dataclass(frozen=True, slots=True)
class Prediction:
    basic_muf_mhz: float
    wanted_power_dbw: float
    noise_factor_db: float
    snr_median_db: float
    lower_deviation_db: float
    upper_deviation_db: float

    def __post_init__(self):
        values = (self.basic_muf_mhz, self.wanted_power_dbw, self.noise_factor_db,
                  self.snr_median_db, self.lower_deviation_db, self.upper_deviation_db)
        if (
            any(type(value) not in (float, int) or not math.isfinite(value) for value in values)
            or not 0 <= self.basic_muf_mhz <= 100
            or not (0 <= self.lower_deviation_db <= 100 and 0 <= self.upper_deviation_db <= 100)
        ):
            raise ValueError("invalid path prediction")


class PathForecast:
    def __init__(self, document):
        if not isinstance(document, dict) or document.get("schema") != 1:
            raise ValueError("expected path forecast manifest schema 1")
        engine = document.get("engine", {})
        if not isinstance(engine, dict) or engine.get("revision") != REVISION or engine.get("name") != "ITURHFProp":
            raise ValueError("path forecast requires the pinned ITURHFProp revision")
        artifact = engine.get("artifact_sha256", {})
        if not isinstance(artifact, dict) or set(artifact) != {"ITURHFProp", "libp533.so", "libp372.so"} or any(
            not isinstance(value, str) or _SHA256.fullmatch(value) is None for value in artifact.values()
        ):
            raise ValueError("path forecast is missing engine artifact digests")
        self.started_at = replay_timestamp(document.get("start_utc"))
        expected_tables = {f"COEFF{self.started_at.month:02d}W.txt", f"ionos{self.started_at.month:02d}.bin", "P1239-3 Decile Factors.txt"}
        tables = engine.get("table_sha256", {})
        if not isinstance(tables, dict) or set(tables) != expected_tables or any(
            not isinstance(value, str) or _SHA256.fullmatch(value) is None for value in tables.values()
        ):
            raise ValueError("path forecast is missing monthly table digests")
        self.assumptions = document.get("assumptions", {})
        if (
            not isinstance(self.assumptions, dict)
            or self.assumptions.get("noise_bandwidth_hz") != NOISE_BANDWIDTH_HZ
            or self.assumptions.get("noise_environment") not in ENVIRONMENTS
            or self.assumptions.get("antenna_model") != "isotropic"
            or self.assumptions.get("path_direction") != "short"
            or type(self.assumptions.get("tx_power_w")) not in (int, float)
            or not 1 <= self.assumptions["tx_power_w"] <= 1e9
            or type(self.assumptions.get("smoothed_sunspot_number")) is not int
            or not 1 <= self.assumptions["smoothed_sunspot_number"] <= 311
        ):
            raise ValueError("path forecast requires declared power, solar activity, antenna, path and 3000 Hz noise assumptions")
        self.engine = engine
        sites = document.get("stations")
        rows = document.get("predictions")
        if not isinstance(sites, dict) or not isinstance(rows, list) or not rows:
            raise ValueError("path forecast needs station coordinates and predictions")
        self.stations = {}
        for site, coordinates in sites.items():
            if not isinstance(coordinates, list) or len(coordinates) != 2 or any(
                type(value) not in (float, int) or not -limit <= value <= limit
                for value, limit in zip(coordinates, (90, 180))
            ):
                raise ValueError("invalid path forecast station coordinates")
            self.stations[site] = tuple(coordinates)

        self.rows = {}
        for row in rows:
            if not isinstance(row, list) or len(row) != 10:
                raise ValueError("invalid path forecast prediction record")
            source, target, band, hour, *result = row
            if (
                not isinstance(source, str) or not isinstance(target, str) or not isinstance(band, str)
                or source == target or source not in self.stations or target not in self.stations
                or band not in FREQUENCIES or type(hour) is not int or not 0 <= hour < 24
            ):
                raise ValueError("invalid path forecast prediction key")
            key = source, target, band, hour
            if key in self.rows:
                raise ValueError("duplicate path forecast prediction")
            self.rows[key] = Prediction(*result)

    @classmethod
    def load(cls, path):
        return cls(json.loads(Path(path).read_bytes()))

    def at(self, source, target, band, now):
        if (now.year, now.month) != (self.started_at.year, self.started_at.month):
            return None
        for station in (source, target):
            if self.stations.get(station["id"]) != (station["lat"], station["lon"]):
                return None
        return self.rows.get((source["id"], target["id"], band, now.hour))

    def describe(self, prediction):
        result = {
            "status": "predicted" if prediction is not None else "fallback",
            "engine": "ITU-R P.533 / ITURHFProp",
            "revision": REVISION,
            "noise_bandwidth_hz": NOISE_BANDWIDTH_HZ,
        }
        if prediction is not None:
            result.update(
                snr_median_db=prediction.snr_median_db,
                snr_p10_db=round(prediction.snr_median_db - prediction.lower_deviation_db, 2),
                snr_p90_db=round(prediction.snr_median_db + prediction.upper_deviation_db, 2),
                basic_muf_mhz=prediction.basic_muf_mhz,
                wanted_power_dbw=prediction.wanted_power_dbw,
                noise_factor_db=prediction.noise_factor_db,
            )
        return result

    def status(self):
        return {
            "engine": "ITU-R P.533 / ITURHFProp",
            "release": RELEASE,
            "revision": REVISION,
            "month_utc": self.started_at.strftime("%Y-%m"),
            "rows": len(self.rows),
            "noise_model": "upstream bundled P.372 implementation",
            "assumptions": self.assumptions,
            "artifact_sha256": self.engine["artifact_sha256"],
            "table_sha256": self.engine["table_sha256"],
        }


def configuration(source, target, start, sunspots, tx_w, environment):
    # The upstream input and output number internal UTC bins 1 through 24.
    hours = ",".join(str(hour) for hour in range(1, 25))
    frequencies = ",".join(str(frequency) for frequency in FREQUENCIES.values())
    point = "".join(f"{corner}.lat {target['lat']}\n{corner}.lng {target['lon']}\n" for corner in ("LL", "LR", "UL", "UR"))
    return f"""PathName "{source['id']} to {target['id']}"
PathTXName "{source['id']}"
Path.L_tx.lat {source['lat']}
Path.L_tx.lng {source['lon']}
TXAntFilePath "ISOTROPIC"
TXGOS 0
PathRXName "{target['id']}"
Path.L_rx.lat {target['lat']}
Path.L_rx.lng {target['lon']}
RXAntFilePath "ISOTROPIC"
RXGOS 0
AntennaOrientation "TX2RX"
Path.year {start.year}
Path.month {start.month}
Path.hour {hours}
Path.SSN {sunspots}
Path.frequency {frequencies}
Path.txpower {10 * math.log10(tx_w / 1000):.9f}
Path.BW {NOISE_BANDWIDTH_HZ}
Path.SNRr 0
Path.SNRXXp 90
Path.ManMadeNoise "{environment}"
Path.Modulation "ANALOG"
Path.SorL "SHORTPATH"
RptFilePath "./"
RptFileFormat "{REPORT}"
{point}latinc 1.0
lnginc 1.0
DataFilePath "./model-data/"
"""


def report_rows(path, source, target, month):
    bands = {frequency: band for band, frequency in FREQUENCIES.items()}
    expected = {(hour, band) for hour in range(24) for band in FREQUENCIES}
    predictions = []
    with path.open(newline="", encoding="ascii") as data:
        reader = csv.DictReader(data)
        if set(reader.fieldnames or ()) != {"month", "hour", "frequency", *NUMERIC_HEADINGS}:
            raise ValueError("unexpected upstream ITURHFProp CSV headings")
        for row in reader:
            band = bands[float(row["frequency"])]
            hour = int(row["hour"]) - 1
            if int(row["month"]) != month or (hour, band) not in expected:
                raise ValueError("unexpected or duplicate ITURHFProp path-hour result")
            expected.remove((hour, band))
            values = [float(row[key]) for key in NUMERIC_HEADINGS]
            Prediction(*values)
            predictions.append([source, target, band, hour, *values])
    if expected:
        raise ValueError("ITURHFProp report omits path-hour results")
    return predictions


def generate(args):
    root = args.itu_root.resolve()
    rev = subprocess.check_output(["git", "-C", root, "rev-parse", "HEAD"], text=True).strip()
    if rev != REVISION:
        raise ValueError(f"expected upstream {RELEASE} at {REVISION}, found {rev}")
    subprocess.run(
        ["git", "-C", root, "diff", "--quiet", "HEAD", "--", ":(glob)**/*.c", ":(glob)**/*.h", ":(glob)**/Makefile", "P372/Data"],
        check=True,
    )
    binary = root / "ITURHFProp/Linux/ITURHFProp"
    lib533 = root / "P533/Linux/libp533.so"
    lib372 = root / "P372/Linux/libp372.so"
    libraries = os.pathsep.join(str(path.parent) for path in (lib533, lib372))
    env = dict(os.environ, LC_ALL="C", TZ="UTC", LD_LIBRARY_PATH=libraries, DYLD_LIBRARY_PATH=libraries)
    start = replay_timestamp(args.start_utc)
    solar_activity = {"kind": "explicit scenario assumption"}
    sunspots = args.sunspots
    if args.noaa_solar_cycle is not None:
        raw = args.noaa_solar_cycle.read_bytes()
        month = start.strftime("%Y-%m")
        matching = [row for row in json.loads(raw) if isinstance(row, dict) and row.get("time-tag") == month]
        if len(matching) != 1:
            raise ValueError(f"expected one NOAA solar-cycle prediction for {month}")
        predicted = matching[0].get("predicted_ssn")
        if type(predicted) not in (int, float) or not 0 <= predicted < 311.5:
            raise ValueError("invalid NOAA predicted smoothed sunspot number")
        sunspots = max(1, math.floor(predicted + 0.5))
        solar_activity = {"kind": "NOAA SWPC predicted monthly smoothed sunspot number",
                          "source": NOAA_SOLAR_CYCLE_URL, "value": predicted,
                          "sha256": hashlib.sha256(raw).hexdigest()}
    data = root / "P372/Data"
    tables = (f"COEFF{start.month:02d}W.txt", f"ionos{start.month:02d}.bin", "P1239-3 Decile Factors.txt")
    sites = make_stations(args.stations)
    paths = list(permutations(sites, 2))
    if args.only_path:
        requested = set(args.only_path)
        paths = [(tx, rx) for tx, rx in paths if f"{tx['id']}:{rx['id']}" in requested]
        if len(paths) != len(requested):
            raise ValueError("--only-path must name two different stations in this run")

    def predict(pair):
        source, target = pair
        with TemporaryDirectory(prefix="linkwise-p533-") as temporary:
            work = Path(temporary)
            (work / "model-data").symlink_to(data, target_is_directory=True)
            (work / "input.in").write_text(
                configuration(source, target, start, sunspots, args.tx_w, args.noise_environment),
                encoding="ascii",
            )
            run = subprocess.run([binary, "-c", "-s", "input.in", "output.csv"],
                                 cwd=work, env=env, capture_output=True, text=True, timeout=60)
            if run.returncode:
                raise ValueError(f"ITURHFProp failed for {source['id']} to {target['id']}: {run.stdout}{run.stderr}")
            return report_rows(work / "output.csv", source["id"], target["id"], start.month)

    with ThreadPoolExecutor(max_workers=args.jobs) as workers:
        rows = [row for result in workers.map(predict, paths) for row in result]
    document = {
        "schema": 1,
        "start_utc": start.isoformat(),
        "engine": {
            "name": "ITURHFProp", "source": UPSTREAM, "release": RELEASE, "revision": REVISION,
            "artifact_sha256": {file.name: sha256(file) for file in (binary, lib533, lib372)},
            "table_sha256": {name: sha256(data / name) for name in tables},
        },
        "assumptions": {
            "tx_power_w": args.tx_w, "antenna_model": "isotropic", "path_direction": "short",
            "smoothed_sunspot_number": sunspots, "solar_activity": solar_activity,
            "noise_environment": args.noise_environment,
            "noise_bandwidth_hz": NOISE_BANDWIDTH_HZ,
        },
        "stations": {site["id"]: [site["lat"], site["lon"]] for site in sites},
        "predictions": rows,
    }
    PathForecast(document)
    args.output.write_text(json.dumps(document, separators=(",", ":"), allow_nan=False) + "\n", encoding="utf-8")
    print(f"Wrote {len(rows)} path-hour-band predictions to {args.output}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--itu-root", type=Path, required=True,
                        help=f"local Git checkout of {UPSTREAM} at {RELEASE}, with its Linux targets built")
    parser.add_argument("--start-utc", required=True, help="scenario start and the model's UTC month, e.g. 2026-11-26T00:00:00Z")
    solar = parser.add_mutually_exclusive_group(required=True)
    solar.add_argument("--sunspots", type=int, choices=range(1, 312), metavar="1–311",
                       help="explicit 12-month smoothed sunspot number")
    solar.add_argument("--noaa-solar-cycle", type=Path,
                       help="frozen NOAA SWPC predicted-solar-cycle.json input")
    parser.add_argument("--stations", type=int, default=48, choices=range(2, 241), metavar="2–240")
    parser.add_argument("--tx-w", type=float, default=100.0)
    parser.add_argument("--noise-environment", choices=ENVIRONMENTS, default="RESIDENTIAL")
    parser.add_argument("--only-path", action="append", help="limit to a directed pair such as SIM-001:SIM-002; repeat as needed")
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not 1 <= args.tx_w <= 1e9 or args.jobs < 1:
        parser.error("require transmit power within model limits and positive jobs")
    try:
        generate(args)
    except (KeyError, OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
