"""Shared catalog and link-pattern geometry for the software HF simulator."""

from dataclasses import asdict, dataclass
import json
import math
from pathlib import Path
from types import MappingProxyType

from stations import BAND_FREQUENCIES, EARTH_RADIUS_KM


CATALOG_PATH = Path(__file__).with_name("antenna-profiles.json")
ANTENNA_PRESETS = ("kiwi-inspired",)
REFERENCE_POWER_W = 100.0
PATTERNS = {"isotropic", "omni", "bidirectional", "directional"}
POLARIZATIONS = {"any", "mixed", "horizontal", "vertical"}


def number(value, minimum, maximum):
    return type(value) in (float, int) and minimum <= value <= maximum


@dataclass(frozen=True, slots=True)
class BandPattern:
    peak_dbi: float
    elevation_deg: float
    azimuth_width_deg: float
    elevation_width_deg: float
    azimuth_limit_db: float
    elevation_limit_db: float

    def __post_init__(self):
        fields = ((self.peak_dbi, -40, 40), (self.elevation_deg, 0, 90),
                  (self.azimuth_width_deg, 1, 360), (self.elevation_width_deg, 1, 180),
                  (self.azimuth_limit_db, 0, 60), (self.elevation_limit_db, 0, 60))
        if any(not number(*bounds) for bounds in fields):
            raise ValueError("invalid antenna band pattern")


@dataclass(frozen=True, slots=True)
class Profile:
    id: str
    name: str
    role: str
    polarization: str
    pattern: str
    bands: MappingProxyType
    switch_s: float
    rotatable: bool
    slew_deg_per_s: float
    feedline_loss_db: float


def parse_catalog(document):
    if not isinstance(document, dict) or document.get("schema") != 1 or not isinstance(document.get("profiles"), list):
        raise ValueError("expected antenna catalog schema 1")
    profiles = {}
    for item in document["profiles"]:
        if not isinstance(item, dict):
            raise ValueError("invalid antenna profile")
        identity, name = item.get("id"), item.get("name")
        pattern, role = item.get("pattern"), item.get("role")
        polarization = item.get("polarization")
        timings = item.get("switch_s"), item.get("rotatable"), item.get("slew_deg_per_s")
        feedline_loss = item.get("feedline_loss_db")
        bands = item.get("bands")
        if (
            not isinstance(identity, str) or not identity or identity in profiles
            or not isinstance(name, str) or not name
            or not isinstance(pattern, str) or pattern not in PATTERNS or role not in ("tx-rx", "rx-only")
            or not isinstance(polarization, str) or polarization not in POLARIZATIONS
            or not isinstance(bands, dict) or not bands or not set(bands) <= set(BAND_FREQUENCIES)
            or not number(timings[0], 0, 60) or type(timings[1]) is not bool
            or not number(timings[2], 0, 360) or timings[1] != (timings[2] > 0)
            or not number(feedline_loss, 0, 20)
        ):
            raise ValueError("invalid antenna profile metadata")
        try:
            patterns = {band: BandPattern(**value) for band, value in bands.items()}
        except TypeError as error:
            raise ValueError(f"invalid band pattern for {identity}") from error
        profiles[identity] = Profile(identity, name, role, polarization, pattern,
                                     MappingProxyType(patterns), *timings, feedline_loss)
    reference = profiles.get("isotropic")
    if (
        reference is None or reference.role != "tx-rx" or reference.pattern != "isotropic"
        or reference.feedline_loss_db != 0
        or reference.polarization != "any" or set(reference.bands) != set(BAND_FREQUENCIES)
    ):
        raise ValueError("antenna catalog needs the isotropic reference on all bands")
    return MappingProxyType(profiles)


def load_catalog(path=CATALOG_PATH):
    return parse_catalog(json.loads(Path(path).read_bytes()))


@dataclass(frozen=True, slots=True)
class Mount:
    profile_id: str = "isotropic"
    heading_deg: float = 0


@dataclass(frozen=True, slots=True)
class StationAntennas:
    tx: Mount = Mount()
    rx: Mount = Mount()
    power_w: float | None = None


@dataclass(frozen=True, slots=True)
class Geometry:
    distance_km: float
    tx_bearing_deg: float | None
    rx_bearing_deg: float | None
    elevation_deg: float


def path_geometry(source, target):
    lat1, lon1 = map(math.radians, (source["lat"], source["lon"]))
    lat2, lon2 = map(math.radians, (target["lat"], target["lon"]))
    delta = lon2 - lon1
    haversine = math.sin((lat2 - lat1) / 2) ** 2 + math.cos(lat1) * math.cos(lat2) * math.sin(delta / 2) ** 2
    arc = 2 * math.asin(math.sqrt(max(0, min(1, haversine))))
    distance = EARTH_RADIUS_KM * arc
    if arc < 1e-10 or abs(math.pi - arc) < 1e-7:
        tx_bearing = rx_bearing = None
    else:
        tx_bearing = math.degrees(math.atan2(
            math.sin(delta) * math.cos(lat2),
            math.cos(lat1) * math.sin(lat2) - math.sin(lat1) * math.cos(lat2) * math.cos(delta),
        )) % 360
        rx_bearing = math.degrees(math.atan2(
            -math.sin(delta) * math.cos(lat1),
            math.cos(lat2) * math.sin(lat1) - math.sin(lat2) * math.cos(lat1) * math.cos(delta),
        )) % 360
    # Approximate the horizon angle with equal 2400 km-or-shorter F2 hops.
    half_hop = arc / (2 * max(1, math.ceil(distance / 2400)))
    apex = EARTH_RADIUS_KM + 300
    elevation = math.degrees(math.atan2(apex * math.cos(half_hop) - EARTH_RADIUS_KM, apex * math.sin(half_hop)))
    return Geometry(distance, tx_bearing, rx_bearing, max(5, elevation))


def pattern_gain(profile, band, bearing_deg, elevation_deg, heading_deg):
    row = profile.bands.get(band)
    if row is None:
        return None
    if profile.pattern == "isotropic":
        return 0.0
    azimuth_loss = 0.0
    if bearing_deg is not None and profile.pattern in ("directional", "bidirectional"):
        angle = abs((bearing_deg - heading_deg + 180) % 360 - 180)
        if profile.pattern == "bidirectional":
            angle = min(angle, 180 - angle)
        azimuth_loss = min(row.azimuth_limit_db, 12 * (angle / row.azimuth_width_deg) ** 2)
    elevation_loss = min(
        row.elevation_limit_db,
        12 * ((elevation_deg - row.elevation_deg) / row.elevation_width_deg) ** 2,
    )
    return row.peak_dbi - azimuth_loss - elevation_loss


@dataclass(frozen=True, slots=True)
class EndpointGain:
    profile_id: str
    heading_deg: float
    gain_dbi: float
    feedline_loss_db: float
    polarization: str


@dataclass(frozen=True, slots=True)
class LinkBudget:
    distance_km: float
    tx_bearing_deg: float | None
    rx_bearing_deg: float | None
    elevation_deg: float
    tx: EndpointGain
    rx: EndpointGain
    polarization_loss_db: float
    net_gain_db: float
    conducted_power_w: float
    eirp_w: float

    def describe(self):
        return asdict(self)


def link_budget(catalog, geometry, band, tx, rx, conducted_power_w):
    tx_profile, rx_profile = catalog[tx.profile_id], catalog[rx.profile_id]
    if tx_profile.role != "tx-rx":
        return None
    tx_gain = pattern_gain(tx_profile, band, geometry.tx_bearing_deg, geometry.elevation_deg, tx.heading_deg)
    rx_gain = pattern_gain(rx_profile, band, geometry.rx_bearing_deg, geometry.elevation_deg, rx.heading_deg)
    if tx_gain is None or rx_gain is None:
        return None
    # HF polarization changes through the ionosphere. This bounded scenario
    # term illustrates a mismatch and does not claim a field measurement.
    polarization_loss = 3.0 if {tx_profile.polarization, rx_profile.polarization} == {"horizontal", "vertical"} else 0.0
    tx_path_gain = tx_gain - tx_profile.feedline_loss_db
    rx_path_gain = rx_gain - rx_profile.feedline_loss_db
    return LinkBudget(
        geometry.distance_km, geometry.tx_bearing_deg, geometry.rx_bearing_deg, geometry.elevation_deg,
        EndpointGain(tx.profile_id, tx.heading_deg, tx_gain, tx_profile.feedline_loss_db, tx_profile.polarization),
        EndpointGain(rx.profile_id, rx.heading_deg, rx_gain, rx_profile.feedline_loss_db, rx_profile.polarization),
        polarization_loss, tx_path_gain + rx_path_gain - polarization_loss,
        conducted_power_w, conducted_power_w * 10 ** (tx_path_gain / 10),
    )


def _mount(data, catalog, *, transmit):
    if data is None:
        return Mount()
    if not isinstance(data, dict) or set(data) - {"profile_id", "heading_deg"}:
        raise ValueError("antenna endpoint needs profile_id and optional heading_deg")
    identity, heading = data.get("profile_id"), data.get("heading_deg", 0)
    if not isinstance(identity, str) or identity not in catalog or not number(heading, 0, 360):
        raise ValueError("unknown antenna profile or invalid heading")
    if transmit and catalog[identity].role != "tx-rx":
        raise ValueError(f"{identity} is a receive-only antenna profile")
    return Mount(identity, heading % 360)


@dataclass(frozen=True, slots=True)
class AntennaConfiguration:
    catalog: MappingProxyType
    stations: MappingProxyType
    mode: str
    catalog_json: str

    def supported(self, source, target, band):
        sender, receiver = self.stations[source], self.stations[target]
        return band in self.catalog[sender.tx.profile_id].bands and band in self.catalog[receiver.rx.profile_id].bands

    def snapshot(self):
        return {
            "enabled": True,
            "catalog": json.loads(self.catalog_json),
            "stations": [
                {"station": identity, "txPort": mounts.tx.profile_id, "rxPort": mounts.rx.profile_id,
                 "txHeading": mounts.tx.heading_deg, "rxHeading": mounts.rx.heading_deg}
                for identity, mounts in self.stations.items()
            ],
        }


def load_configuration(stations, *, preset=None, path=None):
    catalog_json = CATALOG_PATH.read_text(encoding="utf-8")
    catalog = parse_catalog(json.loads(catalog_json))
    known = [station["id"] for station in stations]
    if preset is not None and preset not in ANTENNA_PRESETS:
        raise ValueError(f"unknown antenna preset {preset}")
    default = dict.fromkeys(known, StationAntennas())
    if preset == "kiwi-inspired":
        tx_types = ("dipole-hf", "vertical-hf", "t2fd")
        rx_types = ("dipole-hf", "active-loop", "beverage", "tci-530", "tci-532", "tci-527b")
        station_mounts = {
            identity: StationAntennas(Mount(tx_types[index % len(tx_types)], (index * 53) % 360),
                                      Mount(rx_types[index % len(rx_types)], (index * 71) % 360))
            for index, identity in enumerate(known)
        }
        return AntennaConfiguration(catalog, MappingProxyType(station_mounts), preset, catalog_json)
    if path is not None:
        document = json.loads(Path(path).read_bytes())
        if not isinstance(document, dict) or document.get("schema") != 1 or not isinstance(document.get("stations"), dict):
            raise ValueError("expected antenna configuration schema 1 with a stations object")
        for identity, entry in document["stations"].items():
            if identity not in default or not isinstance(entry, dict) or set(entry) - {"tx", "rx", "power_w"}:
                raise ValueError(f"invalid antenna configuration for {identity}")
            power = entry.get("power_w")
            if power is not None and not number(power, 0.1, 1000):
                raise ValueError("conducted power_w must be between 0.1 and 1000")
            default[identity] = StationAntennas(_mount(entry.get("tx"), catalog, transmit=True),
                                               _mount(entry.get("rx"), catalog, transmit=False), power)
    return AntennaConfiguration(catalog, MappingProxyType(default), "configured", catalog_json)
