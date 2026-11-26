"""Bounded, read-only NOAA SWPC inputs for the software HF simulation."""

from bisect import bisect_right
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
import hashlib
from http.client import HTTPException
import json
import math
from pathlib import Path
import re
import threading
import time
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from drap import DRAP_MAX_AGE, DRAP_MAX_BYTES, DRAP_URL, DrapGrid, parse_drap


GLOTEC_URL = "https://services.swpc.noaa.gov/products/glotec/geojson_2d_urt/"
KP_URL = "https://services.swpc.noaa.gov/json/planetary_k_index_1m.json"
GLOTEC_NAME = re.compile(rb'glotec_icao_(\d{8}T\d{6}Z)\.geojson')
GLOTEC_MAX_AGE = 60 * 60
KP_MAX_AGE = 15 * 60
GRID_WIDTH = 72
GRID_HEIGHT = 72
GRID_SIZE = GRID_WIDTH * GRID_HEIGHT
POLL_SECONDS = 120
DOWNLOAD_TIMEOUT = 6
GLOTEC_MAX_BYTES = 4_000_000
KP_MAX_BYTES = 128_000


def replay_timestamp(value):
    """Archived UTC times require an explicit offset."""
    if not isinstance(value, str) or "T" not in value:
        raise ValueError("missing archived UTC time")
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.utcoffset() != timedelta(0):
        raise ValueError("archived time must use UTC")
    return parsed.astimezone(timezone.utc)


def _now():
    return datetime.now(timezone.utc)


def _timestamp(value):
    if not isinstance(value, str) or "T" not in value:
        raise ValueError("missing NOAA time tag")
    parsed = datetime.fromisoformat(value)
    # The Kp feed reports UTC without a suffix. GloTEC includes a Z suffix.
    return parsed.replace(tzinfo=timezone.utc) if parsed.tzinfo is None else parsed.astimezone(timezone.utc)


def _iso(value):
    return value.isoformat(timespec="seconds").replace("+00:00", "Z") if value else None


def _number(value, minimum, maximum):
    return (
        type(value) in (int, float) and minimum <= value <= maximum
        and math.isfinite(value)
    )


def _download(url, limit):
    request = Request(url, headers={
        "User-Agent": "Linkwise-HF-Simulation/1.0",
        "Cache-Control": "no-cache",
    })
    deadline = time.monotonic() + DOWNLOAD_TIMEOUT
    with urlopen(request, timeout=DOWNLOAD_TIMEOUT) as response:
        size = response.headers.get("Content-Length")
        if size is not None and int(size) > limit:
            raise ValueError("NOAA response exceeds its size limit")
        data = bytearray()
        while len(data) <= limit and time.monotonic() < deadline:
            chunk = response.read1(min(64 * 1024, limit + 1 - len(data)))
            if not chunk:
                return bytes(data)
            data.extend(chunk)
    if len(data) > limit:
        raise ValueError("NOAA response exceeds its size limit")
    raise TimeoutError("NOAA response exceeded its download budget")


@dataclass(frozen=True, slots=True)
class IonosphereCell:
    nmf2_m3: float
    hmf2_km: float
    tec_tecu: float | None
    quality_flag: int  # Rounded observation coverage, capped at 5; 0 is model background.


@dataclass(frozen=True, slots=True)
class _Provenance:
    observed_at: datetime
    fetched_at: datetime
    source: str
    sha256: str

    @classmethod
    def from_download(cls, observed_at, source, data, fetched_at=None):
        return cls(observed_at, fetched_at or _now(), source, hashlib.sha256(data).hexdigest())


@dataclass(frozen=True, slots=True)
class _Grid:
    provenance: _Provenance
    cells: tuple[IonosphereCell | None, ...]
    usable_cells: int
    observed_cells: int


@dataclass(frozen=True, slots=True)
class _Kp:
    provenance: _Provenance
    value: float


@dataclass(frozen=True, slots=True)
class _Drap:
    provenance: _Provenance
    grid: DrapGrid


def _cell_index(lat, lon):
    row = min(int((lat + 90) / 2.5), GRID_HEIGHT - 1)
    column = int(((lon + 180) % 360) / 5)
    return column, row


def _parse_grid(data, source, observed_at=None, fetched_at=None):
    document = json.loads(data)
    if (not isinstance(document, dict) or document.get("type") != "FeatureCollection"
            or not isinstance(document.get("features"), list)):
        raise ValueError("unexpected GloTEC grid")
    tagged_at = _timestamp(document.get("time_tag"))
    if observed_at is not None and observed_at != tagged_at:
        raise ValueError("GloTEC validity time differs from the product filename")
    if source is None:
        source = f"{GLOTEC_URL}glotec_icao_{tagged_at:%Y%m%dT%H%M%SZ}.geojson"
    cells = [None] * GRID_SIZE
    for feature in document["features"]:
        if not isinstance(feature, dict):
            continue
        geometry, props = feature.get("geometry"), feature.get("properties")
        if not isinstance(geometry, dict) or not isinstance(props, dict):
            continue
        coords = geometry.get("coordinates")
        if geometry.get("type") != "Point" or not isinstance(coords, list) or len(coords) != 2:
            continue
        lon, lat = coords
        if not (_number(lat, -90, 90) and _number(lon, -180, 180)):
            continue
        column, row = _cell_index(lat, lon)
        if lon != -177.5 + 5 * column or lat != -88.75 + 2.5 * row:
            continue
        nmf2, hmf2, tec = (props.get(key) for key in ("NmF2", "hmF2", "tec"))
        flag = props.get("quality_flag")
        if not (
            _number(nmf2, 1, 1e15) and _number(hmf2, 1, 1_000)
            and type(flag) is int and 0 <= flag <= 5
        ):
            continue
        index = row * GRID_WIDTH + column
        if cells[index] is not None:
            raise ValueError("duplicate GloTEC grid cell")
        cells[index] = IonosphereCell(nmf2, hmf2, tec if _number(tec, 0, 300) else None, flag)
    usable = sum(cell is not None for cell in cells)
    if not usable:
        raise ValueError("GloTEC grid has no valid F2 values")
    observed = sum(cell is not None and cell.quality_flag > 0 for cell in cells)
    return _Grid(_Provenance.from_download(tagged_at, source, data, fetched_at), tuple(cells), usable, observed)


def _fetch_grid(previous):
    listing = _download(GLOTEC_URL, 2_000_000)
    stamps = sorted(set(GLOTEC_NAME.findall(listing)), reverse=True)
    now = _now()
    candidates = 0
    latest_error = None
    for raw_stamp in stamps:
        stamp = raw_stamp.decode("ascii")
        try:
            observed_at = datetime.strptime(stamp, "%Y%m%dT%H%M%SZ").replace(tzinfo=timezone.utc)
        except ValueError:
            continue
        if observed_at > now:
            continue
        if previous is not None and observed_at <= previous.provenance.observed_at:
            return previous
        url = f"{GLOTEC_URL}glotec_icao_{stamp}.geojson"
        try:
            return _parse_grid(_download(url, GLOTEC_MAX_BYTES), url, observed_at)
        except HTTPError as error:
            # The directory can list a newly issued grid before its URL is available.
            if error.code != 404:
                raise
            latest_error = error
        except ValueError as error:
            latest_error = error
        candidates += 1
        if candidates == 4:
            break
    if latest_error:
        raise ValueError(f"no recent valid GloTEC grid ({latest_error})") from latest_error
    raise ValueError("no recent GloTEC grid was listed")


def _fetch_kp():
    data = _download(KP_URL, KP_MAX_BYTES)
    return _parse_kp(data, KP_URL, _now())


def _parse_kp(data, source, fetched_at):
    rows = json.loads(data)
    if not isinstance(rows, list):
        raise ValueError("unexpected NOAA Kp feed")
    latest = None
    for row in rows:
        if not isinstance(row, dict) or not _number(row.get("estimated_kp"), 0, 9):
            continue
        try:
            observed_at = _timestamp(row.get("time_tag"))
        except ValueError:
            continue
        if observed_at <= fetched_at and (latest is None or observed_at > latest[0]):
            latest = observed_at, float(row["estimated_kp"])
    if latest is None:
        raise ValueError("NOAA Kp feed has no valid observations")
    observed_at, value = latest
    return _Kp(_Provenance.from_download(observed_at, source, data, fetched_at), value)


def _parse_drap(data, source, fetched_at):
    grid = parse_drap(data)
    if grid.observed_at > fetched_at:
        raise ValueError("D-RAP validity time is later than retrieval")
    return _Drap(_Provenance.from_download(grid.observed_at, source, data, fetched_at), grid)


def _fetch_drap():
    data = _download(DRAP_URL, DRAP_MAX_BYTES)
    return _parse_drap(data, DRAP_URL, _now())


@dataclass(frozen=True, slots=True)
class WeatherSnapshot:
    """The verified inputs available at a live or replay time."""

    now: datetime
    _grid: _Grid | None
    _kp: _Kp | None
    _errors: tuple[str | None, ...]
    _attempted: bool
    _drap: _Drap | None = None
    replay: bool = False
    _quiet_drap: _Drap | None = None

    @property
    def glotec_observed_at(self) -> datetime | None:
        return self._grid.provenance.observed_at if self._grid else None

    @property
    def kp_observed_at(self) -> datetime | None:
        return self._kp.provenance.observed_at if self._kp else None

    def _fresh(self, product, max_age):
        return product is not None and product.provenance.fetched_at <= self.now and 0 <= self._age(product) <= max_age

    def _age(self, product):
        return (self.now - product.provenance.observed_at).total_seconds()

    @property
    def kp(self) -> float | None:
        """NOAA's observation-derived estimate from the minute feed."""
        return self._kp.value if self._fresh(self._kp, KP_MAX_AGE) else None

    def cell(self, lat: float, lon: float) -> IonosphereCell | None:
        """Read the containing grid cell, retaining zero-coverage model values."""
        if not self._fresh(self._grid, GLOTEC_MAX_AGE):
            return None
        if not (_number(lat, -90, 90) and _number(lon, -180, 180)):
            return None
        column, row = _cell_index(lat, lon)
        return self._grid.cells[row * GRID_WIDTH + column]

    def drap_vertical_loss_db(self, lat: float, lon: float, frequency_mhz: float) -> float | None:
        if not self._fresh(self._drap, DRAP_MAX_AGE):
            return None
        if not (_number(lat, -90, 90) and _number(lon, -180, 180) and _number(frequency_mhz, 3, 30)):
            return None
        if self._drap.grid.quiet:
            return 0.0
        loss = self._drap.grid.vertical_loss_db(lat, lon, frequency_mhz)
        if self._quiet_reference() is not None:
            # Sample at the equivalent local solar time on the quiet grid.
            hours = (self._drap.grid.observed_at - self._quiet_drap.grid.observed_at).total_seconds() / 3600
            loss -= self._quiet_drap.grid.vertical_loss_db(lat, lon + 15 * hours, frequency_mhz)
        return max(0.0, loss)

    def _quiet_reference(self):
        if self._quiet_drap is not None and self._drap is not None:
            elapsed = self._drap.grid.observed_at - self._quiet_drap.grid.observed_at
            if timedelta(0) <= elapsed <= timedelta(days=1):
                return self._quiet_drap
        return None

    def _status(self, product, max_age, source, error):
        provenance = product.provenance if product else None
        if provenance is not None:
            status = "live" if self._fresh(product, max_age) else "stale"
        else:
            status = "unavailable" if self._attempted else "loading"
        return {
            "status": status,
            "observed_at": _iso(provenance.observed_at) if provenance else None,
            "fetched_at": _iso(provenance.fetched_at) if provenance else None,
            "age_s": round(self._age(product)) if product else None,
            "max_age_s": max_age,
            "source": provenance.source if provenance else source,
            "sha256": provenance.sha256 if provenance else None,
            "error": error,
        }

    def status(self) -> dict:
        glotec = self._status(self._grid, GLOTEC_MAX_AGE, GLOTEC_URL, self._errors[0])
        glotec.update(
            usable_cells=self._grid.usable_cells if self._grid else None,
            observed_cells=self._grid.observed_cells if self._grid else None,
            total_cells=GRID_SIZE,
        )
        kp = self._status(self._kp, KP_MAX_AGE, KP_URL, self._errors[1])
        kp.update(value=self.kp, kind="observation-derived estimate")
        drap = self._status(self._drap, DRAP_MAX_AGE, DRAP_URL, self._errors[2] if len(self._errors) > 2 else None)
        drap.update(kind="modeled solar X-ray and proton absorption")
        quiet = self._quiet_reference()
        if self._drap is not None:
            drap.update(quiet=self._drap.grid.quiet,
                        quiet_reference_at=_iso(quiet.provenance.observed_at) if quiet is not None else None)
        live = sum(source["status"] == "live" for source in (glotec, kp, drap))
        mode = ("fallback", "partial", "partial", "live")[live] if self._attempted else "loading"
        return {"mode": mode, "origin": "replay" if self.replay else "live",
                "glotec": glotec, "kp": kp, "drap": drap}


class WeatherFeed:
    """Fetch in the background, retaining the last inputs through failures."""

    def __init__(self):
        self._grid = self._kp = self._drap = self._quiet_drap = None
        self._errors = (None, None, None)
        self._attempted = False
        self._lock = threading.Lock()
        self._refresh_lock = threading.Lock()
        self._stop = threading.Event()
        self._worker = None

    def snapshot(self, elapsed_s=0) -> WeatherSnapshot:
        with self._lock:
            return WeatherSnapshot(_now(), self._grid, self._kp, self._errors, self._attempted,
                                   self._drap, _quiet_drap=self._quiet_drap)

    def refresh(self) -> WeatherSnapshot:
        """Fetch once on the calling thread for a deliberate headless run."""
        with self._refresh_lock:
            current = self.snapshot()
            grid, kp, drap = current._grid, current._kp, current._drap
            quiet = current._quiet_drap
            grid_error = kp_error = drap_error = None
            try:
                grid = _fetch_grid(grid)
            except (HTTPException, OSError, ValueError) as error:
                grid_error = str(error)
            try:
                observed = _fetch_kp()
                if kp is None or observed.provenance.observed_at >= kp.provenance.observed_at:
                    kp = observed
            except (HTTPException, OSError, ValueError) as error:
                kp_error = str(error)
            try:
                observed = _fetch_drap()
                if drap is None or observed.provenance.observed_at >= drap.provenance.observed_at:
                    drap = observed
                    if drap.grid.quiet:
                        quiet = drap
            except (HTTPException, OSError, ValueError) as error:
                drap_error = str(error)
            with self._lock:
                self._grid, self._kp, self._drap = grid, kp, drap
                self._quiet_drap = quiet
                self._errors = (grid_error, kp_error, drap_error)
                self._attempted = True
            return self.snapshot()

    def _poll(self):
        while not self._stop.is_set():
            self.refresh()
            self._stop.wait(POLL_SECONDS)

    def start(self):
        with self._lock:
            if self._worker is None and not self._stop.is_set():
                self._worker = threading.Thread(target=self._poll, name="noaa-space-weather", daemon=True)
                self._worker.start()

    def stop(self):
        self._stop.set()


class ReplayWeatherFeed:
    """Replay checksum-verified NOAA responses on the simulation clock."""

    def __init__(self, started_at, timeline):
        self.started_at = started_at
        self.timeline = timeline

    @classmethod
    def load(cls, path):
        path = Path(path)
        manifest = json.loads(path.read_bytes())
        if not isinstance(manifest, dict) or manifest.get("schema") != 1 or not isinstance(manifest.get("records"), list):
            raise ValueError("expected weather replay manifest schema 1")
        started_at = replay_timestamp(manifest.get("start_utc"))
        limits = {"glotec": GLOTEC_MAX_BYTES, "kp": KP_MAX_BYTES, "drap": DRAP_MAX_BYTES}
        products = {key: [] for key in limits}
        for item in manifest["records"]:
            if (
                not isinstance(item, dict) or not isinstance(item.get("product"), str)
                or item["product"] not in limits or not isinstance(item.get("file"), str)
            ):
                raise ValueError("invalid weather replay record")
            kind = item["product"]
            filename = path.parent / item["file"]
            with filename.open("rb") as archive:
                data = archive.read(limits[kind] + 1)
            if len(data) > limits[kind] or hashlib.sha256(data).hexdigest() != item.get("sha256"):
                raise ValueError(f"weather replay size or digest mismatch for {filename}")
            fetched_at = replay_timestamp(item.get("fetched_at"))
            if kind == "glotec":
                product = _parse_grid(data, None, fetched_at=fetched_at)
            elif kind == "kp":
                product = _parse_kp(data, KP_URL, fetched_at)
            else:
                product = _parse_drap(data, DRAP_URL, fetched_at)
            if product.provenance.observed_at > fetched_at:
                raise ValueError("weather replay product issued after its recorded retrieval")
            products[kind].append(product)

        timeline = {}
        for kind, records in products.items():
            accepted = []
            for product in sorted(records, key=lambda record: (record.provenance.fetched_at, record.provenance.observed_at)):
                if not accepted or product.provenance.observed_at >= accepted[-1].provenance.observed_at:
                    accepted.append(product)
            timeline[kind] = [product.provenance.fetched_at for product in accepted], tuple(accepted)
        quiet = tuple(record for record in timeline["drap"][1] if record.grid.quiet)
        timeline["quiet_drap"] = [record.provenance.fetched_at for record in quiet], quiet
        return cls(started_at, timeline)

    def snapshot(self, elapsed_s=0):
        now = self.started_at + timedelta(seconds=elapsed_s)
        available = {}
        for kind, (times, products) in self.timeline.items():
            index = bisect_right(times, now)
            available[kind] = products[index - 1] if index else None
        return WeatherSnapshot(
            now, available["glotec"], available["kp"], (None, None, None),
            True, available["drap"], replay=True, _quiet_drap=available["quiet_drap"],
        )

    def refresh(self):
        return self.snapshot()

    def start(self):
        pass

    def stop(self):
        pass
