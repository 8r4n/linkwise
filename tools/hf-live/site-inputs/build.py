"""Stage optional HF model data and browser modules for a static site."""

import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import shutil
import struct
import sys
from tempfile import NamedTemporaryFile
from http.client import HTTPException

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from path_forecast import FREQUENCIES, PathForecast
from space_weather import _fetch_drap, _fetch_grid, _fetch_kp, _iso, _timestamp
from stations import BANDS, make_stations


SITES = make_stations(48)
BAND_IDS = tuple(FREQUENCIES)
FORECAST_ROW = struct.Struct("<ihhh")
MISSING = -(2 ** 31)


def atomic_write(path, data):
    with NamedTemporaryFile(dir=path.parent, delete=False) as file:
        temporary = Path(file.name)
        file.write(data)
    temporary.chmod(0o644)
    temporary.replace(path)


def save_json(path, data):
    atomic_write(path, (json.dumps(data, separators=(",", ":"), ensure_ascii=False, allow_nan=False) + "\n").encode())


def stage_modules(output):
    for name in ("path-inputs.mjs", "weather-inputs.mjs"):
        shutil.copyfile(Path(__file__).with_name(name), output / name)
    stations = json.dumps(SITES, separators=(",", ":"), ensure_ascii=False)
    bands = json.dumps(BANDS, separators=(",", ":"), ensure_ascii=False)
    atomic_write(output / "stations.mjs", f"export const stations = {stations};\nexport const bands = {bands};\n".encode())


def stage_forecast(path, output):
    forecast = PathForecast.load(path)
    sites = {site["id"]: (index, site["lat"], site["lon"]) for index, site in enumerate(SITES)}
    if forecast.stations != {key: value[1:] for key, value in sites.items()}:
        raise ValueError("forecast stations must match the published 48 station coordinates")
    count = len(sites)
    slots = count * (count - 1) * len(BAND_IDS) * 24
    binary = bytearray(FORECAST_ROW.pack(MISSING, 0, 0, 0) * slots)
    for (source, target, band, hour), row in forecast.rows.items():
        tx, rx = sites[source][0], sites[target][0]
        pair = tx * (count - 1) + rx - (rx > tx)
        index = ((pair * len(BAND_IDS) + BAND_IDS.index(band)) * 24 + hour) * FORECAST_ROW.size
        fields = (row.snr_median_db, row.basic_muf_mhz, row.lower_deviation_db, row.upper_deviation_db)
        values = [round(100 * value) for value in fields]
        if not MISSING < values[0] < 2 ** 31 or any(not 0 <= value < 32768 for value in values[1:]):
            raise ValueError("forecast value exceeds the static table format")
        FORECAST_ROW.pack_into(binary, index, *values)
    digest = hashlib.sha256(binary).hexdigest()
    file = f"forecast-{digest[:16]}.bin"
    atomic_write(output / file, binary)
    save_json(output / "forecast.json", {
        "schema": 1, "file": file, "byte_length": len(binary), "sha256": digest,
        "start_utc": _iso(forecast.started_at), **forecast.status(),
    })
    for stale in output.glob("forecast-*.bin"):
        if stale.name != file:
            stale.unlink()


def encode_bytes(data):
    return base64.b64encode(data).decode("ascii")


def provenance(product):
    info = product.provenance
    return {
        "observed_at": _iso(info.observed_at), "fetched_at": _iso(info.fetched_at),
        "source": info.source, "sha256": info.sha256,
    }


def glotec_asset(product):
    row = struct.Struct("<ffB")
    data = bytearray()
    for cell in product.cells:
        data.extend(row.pack(cell.nmf2_m3, cell.hmf2_km, cell.quality_flag) if cell else row.pack(0, 0, 255))
    return {
        **provenance(product), "cells": encode_bytes(data),
        "usable_cells": product.usable_cells, "observed_cells": product.observed_cells,
    }


def drap_asset(product):
    grid = product.grid
    rows = len(grid.rows)
    cols = len(grid.rows[0])
    values = [round(10 * value) for row in grid.rows for value in row]
    data = struct.pack(f"<{len(values)}H", *values)
    return {**provenance(product), "rows": rows, "cols": cols, "quiet": grid.quiet, "cells": encode_bytes(data)}


def _safe_fetch(key, fetch):
    try:
        return fetch()
    except (HTTPException, OSError, ValueError) as exc:
        print(f"NOAA {key} refresh failed, keeping any archived product ({type(exc).__name__})", file=sys.stderr)
        return None


def refresh_weather(output):
    path = output / "weather.json"
    try:
        weather = json.loads(path.read_bytes())
        if not isinstance(weather, dict) or weather.get("schema") != 1:
            weather = {"schema": 1}
    except (OSError, ValueError):
        weather = {"schema": 1}

    fetchers = {
        "glotec": (lambda: _fetch_grid(None), glotec_asset),
        "kp": (_fetch_kp, lambda product: {**provenance(product), "value": product.value}),
        "drap": (_fetch_drap, drap_asset),
    }
    with ThreadPoolExecutor(max_workers=len(fetchers)) as pool:
        fetched = pool.map(_safe_fetch, fetchers, (entry[0] for entry in fetchers.values()))
        for (key, (_, encode)), product in zip(fetchers.items(), fetched):
            if product is None:
                continue
            previous = weather.get(key)
            try:
                previous_time = _timestamp(previous["observed_at"])
            except (TypeError, KeyError, ValueError):
                previous_time = None
            if previous_time is None or product.provenance.observed_at >= previous_time:
                product_asset = encode(product)
                weather[key] = product_asset
                if key == "drap" and product.grid.quiet:
                    weather["quiet_drap"] = product_asset
    if len(weather) > 1:
        save_json(path, weather)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path, help="staged public /hf/data directory")
    parser.add_argument("--forecast", type=Path, help="complete or partial native ITURHFProp monthly forecast JSON")
    parser.add_argument("--refresh-weather", action="store_true", help="refresh public NOAA inputs without discarding archived data")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    stage_modules(args.output)
    if args.forecast is not None:
        try:
            stage_forecast(args.forecast, args.output)
        except (OSError, ValueError) as exc:
            print(f"Forecast staging skipped, keeping any archived baseline ({exc})", file=sys.stderr)
    if args.refresh_weather:
        refresh_weather(args.output)


if __name__ == "__main__":
    main()
