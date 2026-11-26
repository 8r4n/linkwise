"""Synthetic station locations and illustrative HF path conditions."""

from datetime import timedelta
import math


LOCATIONS = (
    ("Seattle", 47.61, -122.33), ("London", 51.51, -0.13),
    ("São Paulo", -23.55, -46.63), ("Cape Town", -33.92, 18.42),
    ("Tokyo", 35.68, 139.69), ("Sydney", -33.87, 151.21),
    ("Denver", 39.74, -104.99), ("Berlin", 52.52, 13.41),
    ("Lima", -12.05, -77.04), ("Nairobi", -1.29, 36.82),
    ("Mumbai", 19.08, 72.88), ("Auckland", -36.85, 174.76),
    ("Montréal", 45.50, -73.57), ("Reykjavík", 64.15, -21.94),
    ("Santiago", -33.45, -70.67), ("Dakar", 14.72, -17.47),
    ("Seoul", 37.57, 126.98), ("Perth", -31.95, 115.86),
    ("Honolulu", 21.31, -157.86), ("Lisbon", 38.72, -9.14),
    ("Mexico City", 19.43, -99.13), ("Cairo", 30.04, 31.24),
    ("Singapore", 1.35, 103.82), ("Darwin", -12.46, 130.84),
    ("Anchorage", 61.22, -149.90), ("Oslo", 59.91, 10.75),
    ("Buenos Aires", -34.60, -58.38), ("Accra", 5.60, -0.19),
    ("Manila", 14.60, 120.98), ("Hobart", -42.88, 147.33),
    ("San Francisco", 37.77, -122.42), ("Rome", 41.90, 12.50),
    ("Bogotá", 4.71, -74.07), ("Windhoek", -22.56, 17.08),
    ("Ulaanbaatar", 47.92, 106.92), ("Christchurch", -43.53, 172.64),
    ("Halifax", 44.65, -63.58), ("Athens", 37.98, 23.73),
    ("Quito", -0.18, -78.47), ("Tunis", 36.81, 10.18),
    ("Jakarta", -6.21, 106.85), ("Suva", -18.12, 178.45),
    ("Phoenix", 33.45, -112.07), ("Prague", 50.08, 14.44),
    ("Montevideo", -34.90, -56.16), ("Lusaka", -15.42, 28.28),
    ("Bangkok", 13.76, 100.50), ("Nouméa", -22.27, 166.46),
)

BANDS = (
    {"id": "80m", "name": "80 m", "frequency_mhz": 3.6},
    {"id": "40m", "name": "40 m", "frequency_mhz": 7.1},
    {"id": "20m", "name": "20 m", "frequency_mhz": 14.1},
)
BAND_FREQUENCIES = {band["id"]: band["frequency_mhz"] for band in BANDS}
EARTH_RADIUS_KM = 6371

OFFSETS = ((0, 0), (0.35, 0.45), (-0.45, 0.6), (0.6, -0.3), (-0.2, -0.5))
MAX_STATIONS = len(LOCATIONS) * len(OFFSETS)


def make_stations(count):
    stations = []
    for index in range(count):
        group, site = divmod(index, len(LOCATIONS))
        name, lat, lon = LOCATIONS[site]
        north, east = OFFSETS[group]
        stations.append({
            "id": f"SIM-{index + 1:03d}",
            "name": f"{name} {group + 1}" if group else name,
            "lat": lat + north,
            "lon": lon + east,
        })
    return stations


def _unit_vector(station):
    lat, lon = math.radians(station["lat"]), math.radians(station["lon"])
    return math.cos(lat) * math.cos(lon), math.cos(lat) * math.sin(lon), math.sin(lat)


def _path_points(source, target, central_angle, fractions):
    start, end = _unit_vector(source), _unit_vector(target)
    for fraction in fractions:
        before = math.sin((1 - fraction) * central_angle)
        after = math.sin(fraction * central_angle)
        x, y, z = (before * a + after * b for a, b in zip(start, end))
        yield math.degrees(math.atan2(z, math.hypot(x, y))), math.degrees(math.atan2(y, x))


def _band_support_offset(frequency, muf):
    ratio = frequency / max(0.1, muf)
    return 4 * (1 - ratio) if ratio <= 1 else -min(18, 15 * math.log2(ratio))


def _drap_path_loss(source, target, frequency, central_angle, hop_count, half_hop, cells, weather):
    # Approximate where each ray leg crosses the D region at 70 km. The
    # vertical D-RAP value already includes both the upward and downward leg.
    loss = 0.0
    for hop, cell in enumerate(cells):
        height = cell.hmf2_km if cell is not None else 300
        leg_fraction = min(1, 70 / height) / 2
        fractions = ((hop + leg_fraction) / hop_count, (hop + 1 - leg_fraction) / hop_count)
        legs = _path_points(source, target, central_angle, fractions)
        losses = [weather.drap_vertical_loss_db(lat, lon, frequency) for lat, lon in legs]
        if any(value is None for value in losses):
            return None
        radius = EARTH_RADIUS_KM + height
        elevation = math.atan2(radius * math.cos(half_hop) - EARTH_RADIUS_KM, radius * math.sin(half_hop))
        # The obliquity approximation diverges near the horizon.
        loss += sum(losses) / (2 * math.sin(max(math.radians(10), elevation)))
    return loss


def _live_adjustment(source, target, band, central_angle, weather, baseline_muf=None):
    if weather is None:
        return 0, None
    distance_km = EARTH_RADIUS_KM * central_angle
    hop_count = max(1, math.ceil(distance_km / 2400))
    half_hop = central_angle / (2 * hop_count)
    horizontal = EARTH_RADIUS_KM * math.sin(half_hop)
    curve_height = EARTH_RADIUS_KM * (1 - math.cos(half_hop))
    midpoints = list(_path_points(source, target, central_angle, ((hop + 0.5) / hop_count for hop in range(hop_count))))
    cells = [weather.cell(lat, lon) for lat, lon in midpoints]
    status = weather.status()
    correction = 0.0
    details = {}

    # The secant law uses a simplified F2 shell per hop. The weakest hop
    # controls the modeled circuit. F2 support alone is not a link budget.
    frequency = BAND_FREQUENCIES[band]
    if all(cell is not None for cell in cells):
        hop_limits = []
        for cell in cells:
            critical_mhz = math.sqrt(cell.nmf2_m3 / 1.24e10)
            secant = math.hypot(1, horizontal / (cell.hmf2_km + curve_height))
            hop_limits.append(critical_mhz * min(3.5, secant))
        muf = min(hop_limits)
        supported = sum(cell.quality_flag > 0 for cell in cells)
        f2_offset = _band_support_offset(frequency, muf)
        if baseline_muf is not None:
            f2_offset -= _band_support_offset(frequency, baseline_muf)
            # Background-only cells carry no independent update over a prior.
            confidence = sum(cell.quality_flag / 5 for cell in cells) / hop_count
        else:
            confidence = 0.7 + 0.3 * supported / hop_count
        correction += f2_offset * confidence
        details.update(muf_mhz=round(muf, 2), glotec_at=status["glotec"]["observed_at"],
                       sampled_hops=hop_count, supported_hops=supported,
                       glotec_confidence=round(confidence, 2))

    drap = _drap_path_loss(source, target, frequency, central_angle, hop_count, half_hop, cells, weather)
    if drap is not None:
        correction -= drap
        details.update(drap_at=status["drap"]["observed_at"], drap_absorption_db=round(drap, 2),
                       drap_quiet_reference_at=status["drap"]["quiet_reference_at"])

    if weather.kp is not None:
        if baseline_muf is None and drap is None:
            latitude = max(abs(lat) for lat, _ in midpoints)
            high_latitude = max(0, (latitude - 40) / 50)
            correction -= 1.75 * high_latitude * max(0, weather.kp - 3)
        details.update(kp=weather.kp, kp_at=status["kp"]["observed_at"])

    if details:
        details["snr_adjustment_db"] = round(correction, 2)
    return correction, details or None


def path_conditions(source, target, band, elapsed, rng, weather=None, *, forecast=None, slow_offset_db=None):
    lat1, lat2 = math.radians(source["lat"]), math.radians(target["lat"])
    lon_delta = math.radians(target["lon"] - source["lon"])
    arc = math.sin((lat2 - lat1) / 2) ** 2
    arc += math.cos(lat1) * math.cos(lat2) * math.sin(lon_delta / 2) ** 2
    central_angle = 2 * math.asin(math.sqrt(min(1, arc)))
    distance_km = EARTH_RADIUS_KM * central_angle

    # Use the same clock for forecast lookup and the fallback path estimate.
    utc = 13 + elapsed / 3600
    now = None
    if weather is not None:
        now = weather.now
    elif forecast is not None:
        now = forecast.started_at + timedelta(seconds=elapsed)
    if now is not None:
        utc = now.hour + now.minute / 60 + now.second / 3600
    daylight = sum(
        max(0, math.cos((utc + point["lon"] / 15 - 12) * math.pi / 12))
        for point in (source, target)
    ) / 2
    if band == "80m":
        base = 11 - 10 * daylight - distance_km / 1250
    elif band == "40m":
        base = 11 - 3 * daylight - distance_km / 2400
    else:
        base = 4 + 9 * daylight - distance_km / 2600
    prediction = None
    if forecast is not None:
        prediction = forecast.at(source, target, band, now)
    correction, details = _live_adjustment(
        source, target, band, central_angle, weather,
        baseline_muf=prediction.basic_muf_mhz if prediction is not None else None,
    )
    if forecast is not None:
        details = details or {}
        details["forecast"] = forecast.describe(prediction)
    if prediction is not None:
        snr_db = max(-100, min(100, prediction.snr_median_db + correction + (slow_offset_db or 0)))
    else:
        offset = rng.gauss(0, 5.5) if slow_offset_db is None else slow_offset_db
        snr_db = max(-25, min(25, base + correction + offset))
    return snr_db, rng.uniform(1, 8), details
