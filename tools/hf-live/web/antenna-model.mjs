// Deterministic angular patterns shared with the native HF simulator.
const RADIUS_KM = 6371;
const DEG = Math.PI / 180;
const BANDS = ["80m", "40m", "20m"];
const PATTERNS = ["isotropic", "omni", "bidirectional", "directional"];
const POLARIZATIONS = ["any", "mixed", "horizontal", "vertical"];
const number = (value, min, max) => Number.isFinite(value) && value >= min && value <= max;
const wrap = (angle) => ((angle % 360) + 360) % 360;

export function parseAntennaCatalog(document) {
  if (document?.schema !== 1 || !Array.isArray(document.profiles)) throw Error("Invalid antenna catalog.");
  const ids = new Set();
  const profiles = document.profiles.map(profile => {
    if (!profile || typeof profile.id !== "string" || !profile.id || ids.has(profile.id)
        || typeof profile.name !== "string" || !profile.name || !["tx-rx", "rx-only"].includes(profile.role)
        || !PATTERNS.includes(profile.pattern) || !POLARIZATIONS.includes(profile.polarization)
        || !number(profile.feedline_loss_db, 0, 20) || !number(profile.switch_s, 0, 60)
        || typeof profile.rotatable !== "boolean" || !number(profile.slew_deg_per_s, 0, 360)
        || profile.rotatable !== (profile.slew_deg_per_s > 0)
        || !profile.bands || typeof profile.bands !== "object" || Array.isArray(profile.bands)) {
      throw Error("Invalid antenna profile.");
    }
    ids.add(profile.id);
    const bands = {};
    for (const [key, row] of Object.entries(profile.bands)) {
      if (!BANDS.includes(key) || !row || !number(row.peak_dbi, -40, 40) || !number(row.elevation_deg, 0, 90)
          || !number(row.azimuth_width_deg, 1, 360) || !number(row.elevation_width_deg, 1, 180)
          || !number(row.azimuth_limit_db, 0, 60) || !number(row.elevation_limit_db, 0, 60)) {
        throw Error(`Invalid band pattern for ${profile.id}.`);
      }
      bands[key] = Object.freeze({ ...row });
    }
    if (!Object.keys(bands).length) throw Error(`No antenna band coverage for ${profile.id}.`);
    return Object.freeze({ ...profile, source: Object.freeze({ ...profile.source }), bands: Object.freeze(bands) });
  });
  const reference = profiles.find(profile => profile.id === "isotropic");
  if (!reference || reference.pattern !== "isotropic" || reference.polarization !== "any"
      || reference.role !== "tx-rx" || reference.feedline_loss_db !== 0
      || BANDS.some(band => !Object.hasOwn(reference.bands, band))) {
    throw Error("The antenna catalog needs an isotropic reference on every band.");
  }
  return Object.freeze({ schema: 1, profiles: Object.freeze(profiles) });
}

export async function loadAntennaCatalog({ signal, fetcher = globalThis.fetch } = {}) {
  const response = await fetcher(new URL("./antenna-profiles.json", import.meta.url), { signal });
  if (!response.ok) throw Error("The antenna catalog is unavailable.");
  return parseAntennaCatalog(await response.json());
}

export function pathGeometry(source, target) {
  const lat1 = source.lat * DEG;
  const lat2 = target.lat * DEG;
  const delta = (target.lon - source.lon) * DEG;
  const haversine = Math.sin((lat2 - lat1) / 2) ** 2 + Math.cos(lat1) * Math.cos(lat2) * Math.sin(delta / 2) ** 2;
  const arc = 2 * Math.asin(Math.sqrt(Math.max(0, Math.min(1, haversine))));
  const distance = RADIUS_KM * arc;
  const undefinedBearing = arc < 1e-10 || Math.abs(Math.PI - arc) < 1e-7;
  const txBearing = undefinedBearing ? null : wrap(Math.atan2(
    Math.sin(delta) * Math.cos(lat2),
    Math.cos(lat1) * Math.sin(lat2) - Math.sin(lat1) * Math.cos(lat2) * Math.cos(delta),
  ) / DEG);
  const rxBearing = undefinedBearing ? null : wrap(Math.atan2(
    -Math.sin(delta) * Math.cos(lat1),
    Math.cos(lat2) * Math.sin(lat1) - Math.sin(lat2) * Math.cos(lat1) * Math.cos(delta),
  ) / DEG);
  const halfHop = arc / (2 * Math.max(1, Math.ceil(distance / 2400)));
  const apex = RADIUS_KM + 300;
  const elevation = Math.atan2(apex * Math.cos(halfHop) - RADIUS_KM, apex * Math.sin(halfHop)) / DEG;
  return { distance_km: distance, tx_bearing_deg: txBearing, rx_bearing_deg: rxBearing, elevation_deg: Math.max(5, elevation) };
}

export function patternGain(profile, band, bearingDeg, elevationDeg, headingDeg = 0) {
  const row = profile?.bands?.[band];
  if (!row) return null;
  if (profile.pattern === "isotropic") return 0;
  let azimuthLoss = 0;
  if (bearingDeg !== null && ["directional", "bidirectional"].includes(profile.pattern)) {
    let angle = Math.abs(wrap(bearingDeg - headingDeg + 180) - 180);
    if (profile.pattern === "bidirectional") angle = Math.min(angle, 180 - angle);
    azimuthLoss = Math.min(row.azimuth_limit_db, 12 * (angle / row.azimuth_width_deg) ** 2);
  }
  const elevationLoss = Math.min(row.elevation_limit_db, 12 * ((elevationDeg - row.elevation_deg) / row.elevation_width_deg) ** 2);
  return row.peak_dbi - azimuthLoss - elevationLoss;
}

export function gainCut(profile, band, elevationDeg, headingDeg = 0) {
  if (!profile?.bands?.[band]) return [];
  return Array.from({ length: 73 }, (_, sample) => {
    const bearing_deg = sample * 5;
    return { bearing_deg, gain_dbi: patternGain(profile, band, bearing_deg, elevationDeg, headingDeg) };
  });
}

export function linkBudget({ geometry, band, tx, rx, conductedPowerW, catalog }) {
  const transmit = catalog.profiles.find(profile => profile.id === tx?.profile_id);
  const receive = catalog.profiles.find(profile => profile.id === rx?.profile_id);
  if (transmit?.role !== "tx-rx" || !receive || !number(conductedPowerW, Number.MIN_VALUE, 1e12)) return null;
  const txGain = patternGain(transmit, band, geometry.tx_bearing_deg, geometry.elevation_deg, tx.heading_deg);
  const rxGain = patternGain(receive, band, geometry.rx_bearing_deg, geometry.elevation_deg, rx.heading_deg);
  if (txGain === null || rxGain === null) return null;
  const txPathGain = txGain - transmit.feedline_loss_db;
  const rxPathGain = rxGain - receive.feedline_loss_db;
  const opposite = (transmit.polarization === "horizontal" && receive.polarization === "vertical")
    || (transmit.polarization === "vertical" && receive.polarization === "horizontal");
  const polarizationLoss = opposite ? 3 : 0;
  return {
    ...geometry,
    tx: { ...tx, gain_dbi: txGain, feedline_loss_db: transmit.feedline_loss_db, polarization: transmit.polarization },
    rx: { ...rx, gain_dbi: rxGain, feedline_loss_db: receive.feedline_loss_db, polarization: receive.polarization },
    polarization_loss_db: polarizationLoss,
    net_gain_db: txPathGain + rxPathGain - polarizationLoss,
    conducted_power_w: conductedPowerW,
    eirp_w: conductedPowerW * 10 ** (txPathGain / 10),
  };
}
