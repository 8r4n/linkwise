// Isotropic cold-plasma F2 model. Heights and Earth radius are in km.
const R = 6371;
export function densityFraction(height, peak, scale = 50) {
  const z = (height - peak) / scale;
  return Math.exp(0.5 * (1 - z - Math.exp(-z)));
}

// Integrate the spherical Snell invariant p = n r cos(elevation).
// A sqrt-height substitution regularizes the integral at the turning point.
export function ray(elevation, peak, peakDensity, mhz, samples = 96) {
  const ratio = (8.98 ** 2 * peakDensity) / (mhz * 1e6) ** 2;
  const invariant =
    R *
    Math.sqrt(1 - ratio * densityFraction(0, peak)) *
    Math.cos((elevation * Math.PI) / 180);
  const discriminant = (h) =>
    (R + h) ** 2 * (1 - ratio * densityFraction(h, peak)) - invariant ** 2;
  let lo = 0,
    hi = 2;
  while (hi <= peak + 50 && discriminant(hi) > 0) {
    lo = hi;
    hi += 2;
  }
  if (hi > peak + 50) return null;
  for (let i = 0; i < 40; i++) {
    const mid = (lo + hi) / 2;
    if (discriminant(mid) > 0) lo = mid;
    else hi = mid;
  }
  const turn = (lo + hi) / 2,
    du = Math.sqrt(turn) / samples;
  let angle = 0;
  const path = [[0, 0]];
  for (let i = 0; i < samples; i++) {
    const u = (samples - i - 0.5) * du,
      h = turn - u * u;
    angle += (2 * u * du * invariant) / ((R + h) * Math.sqrt(discriminant(h)));
    path.push([angle * R, turn - ((samples - i - 1) * du) ** 2]);
  }
  const distance = 2 * angle * R;
  for (let i = samples - 1; i >= 0; i--)
    path.push([distance - path[i][0], path[i][1]]);
  return { distance, path, elevation, turn };
}

export function solve(distance, peak, peakDensity, mhz) {
  let previous = null;
  for (let elevation = 1; elevation <= 89; elevation += 1) {
    const next = ray(elevation, peak, peakDensity, mhz);
    if (
      next &&
      previous &&
      (previous.distance - distance) * (next.distance - distance) <= 0
    ) {
      let a = previous,
        b = next;
      for (let i = 0; i < 18; i++) {
        const m = ray((a.elevation + b.elevation) / 2, peak, peakDensity, mhz);
        if (!m) return null;
        if ((a.distance - distance) * (m.distance - distance) <= 0) b = m;
        else a = m;
      }
      return Math.abs(a.distance - distance) < 1 ? a : null;
    }
    previous = next;
  }
  return null;
}
