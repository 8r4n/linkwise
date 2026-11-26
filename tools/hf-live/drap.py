"""NOAA's combined solar X-ray and solar proton D-region absorption grid."""

from dataclasses import dataclass
from datetime import datetime, timezone
import math
import re


DRAP_URL = "https://services.swpc.noaa.gov/text/drap_global_frequencies.txt"
DRAP_MAX_AGE = 15 * 60
DRAP_MAX_BYTES = 512_000
_VALID_AT = re.compile(r"^# Product Valid At\s*:\s*(.+)$", re.MULTILINE)
_ROW = re.compile(r"^\s*([+-]?\d+(?:\.\d+)?)\s*\|\s*(.+)$")
_QUIET_XRAY = re.compile(r"^#\s+X-RAY Message\s*:\s*Normal X-ray Background\s*$", re.I | re.M)
_QUIET_PROTON = re.compile(r"^#\s+Proton Message\s*:\s*Normal Proton Background\s*$", re.I | re.M)


@dataclass(frozen=True, slots=True)
class DrapGrid:
    observed_at: datetime
    longitude_step: float
    latitude_step: float
    rows: tuple[tuple[float, ...], ...]
    quiet: bool

    def vertical_loss_db(self, latitude: float, longitude: float, frequency_mhz: float) -> float:
        """Return the modeled up-and-down loss through one vertical column."""
        row = min(math.floor((latitude + 90) / self.latitude_step), len(self.rows) - 1)
        column = math.floor((longitude + 180) % 360 / self.longitude_step)
        # The combined grid reports the highest affected frequency at 1 dB.
        return (self.rows[row][column] / frequency_mhz) ** 1.5


def _spacing(centres, extent):
    if len(centres) < 2:
        raise ValueError("D-RAP grid must have at least two cell centres per axis")
    step = extent / len(centres)
    if any(
        not math.isclose(value, -extent / 2 + step * (index + 0.5), abs_tol=1e-6)
        for index, value in enumerate(centres)
    ):
        raise ValueError("D-RAP cell centres do not cover a regular global grid")
    return step


def parse_drap(data: bytes) -> DrapGrid:
    text = data.decode("ascii")
    valid_at = _VALID_AT.search(text)
    if valid_at is None:
        raise ValueError("D-RAP table has no validity time")
    observed_at = datetime.strptime(valid_at[1].strip(), "%Y-%m-%d %H:%M UTC").replace(tzinfo=timezone.utc)

    longitudes = None
    latitudes = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or set(line) == {"-"}:
            continue
        if longitudes is None:
            longitudes = list(map(float, line.split()))
            longitude_step = _spacing(longitudes, 360)
            continue
        match = _ROW.fullmatch(line)
        if match is None:
            raise ValueError("malformed D-RAP latitude row")
        latitude = float(match[1])
        row = tuple(map(float, match[2].split()))
        if latitude in latitudes or len(row) != len(longitudes) or any(not 0 <= haf <= 100 for haf in row):
            raise ValueError("invalid D-RAP latitude row")
        latitudes[latitude] = row
    if longitudes is None:
        raise ValueError("D-RAP table has no longitude header")
    latitude_centres = sorted(latitudes)
    latitude_step = _spacing(latitude_centres, 180)
    quiet = _QUIET_XRAY.search(text) is not None and _QUIET_PROTON.search(text) is not None
    return DrapGrid(observed_at, longitude_step, latitude_step, tuple(latitudes[key] for key in latitude_centres), quiet)
