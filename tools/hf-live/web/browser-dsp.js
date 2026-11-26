const FFT_SIZE = 2048;
const HOP = FFT_SIZE / 4;
const BINS = FFT_SIZE / 2 + 1;
const MAX_ROWS = 96;
const FLOOR_DBFS = -120;

export async function loadModem(bandwidth) {
  const response = await fetch(new URL("./bw500_browser.wasm", import.meta.url));
  if (!response.ok) throw Error("The BW500 WebAssembly modem is unavailable.");
  const { instance } = await WebAssembly.instantiate(await response.arrayBuffer());
  const api = instance.exports;
  const rate = api.lw_sample_rate();
  const capacity = api.lw_max_samples();
  if (api.lw_set_mode ? api.lw_set_mode(bandwidth) !== 1 : bandwidth !== 500) {
    throw Error("The browser modem does not support this mode.");
  }
  const payloadCapacity = api.lw_payload_capacity?.() ?? 9;
  function buffer(kind, count) {
    return new Float32Array(api.memory.buffer, api.lw_buffer_ptr(kind), count);
  }
  function mailbox() {
    return new Uint8Array(api.memory.buffer, api.lw_payload_ptr(), payloadCapacity);
  }
  function encode(payload) {
    if (!payload.length || payload.length > payloadCapacity) throw Error("Invalid BW500 family payload size.");
    mailbox().set(payload);
    const count = api.lw_encode_len ? api.lw_encode_len(payload.length) : api.lw_encode();
    if (count <= 0 || count > capacity) throw Error("BW500 could not encode the burst.");
    return buffer(0, count).slice();
  }
  function power(samples) {
    buffer(0, samples.length).set(samples);
    return api.lw_reference_power(0, samples.length);
  }
  function propagate(samples, noiseSeed, pathSeed, start, snr, includeNoise) {
    buffer(0, samples.length).set(samples);
    const count = api.lw_propagate(samples.length, noiseSeed, pathSeed, start, snr, Number(includeNoise));
    if (count < samples.length || count > capacity) throw Error("BW500 channel could not propagate the burst.");
    const signal = buffer(1, count).slice();
    if (!includeNoise) return { signal };
    const noise = buffer(2, count).slice();
    return {
      signal, noise,
      signalPower: api.lw_reference_power(1, count),
      noisePower: api.lw_reference_power(2, count),
    };
  }
  function decode(samples, expected) {
    buffer(0, samples.length).set(samples);
    const status = api.lw_decode(samples.length);
    if (status < 0) throw Error("BW500 receiver rejected the audio samples.");
    if (status !== 1 || (api.lw_decoded_len?.() ?? 9) !== expected.length) return null;
    const decoded = mailbox().slice(0, expected.length);
    return decoded.every((byte, index) => byte === expected[index]) ? decoded : null;
  }
  return { rate, payloadCapacity, encode, power, propagate, decode };
}

const bitReversal = new Uint16Array(FFT_SIZE);
const hann = new Float64Array(FFT_SIZE);
const cos = new Float64Array(FFT_SIZE / 2);
const sin = new Float64Array(FFT_SIZE / 2);
for (let index = 0; index < FFT_SIZE; index++) {
  let reverse = 0;
  for (let bit = 1; bit < FFT_SIZE; bit *= 2) {
    reverse = (reverse << 1) | Number(Boolean(index & bit));
  }
  bitReversal[index] = reverse;
  hann[index] = .5 - .5 * Math.cos(2 * Math.PI * index / (FFT_SIZE - 1));
  if (index < FFT_SIZE / 2) {
    const phase = -2 * Math.PI * index / FFT_SIZE;
    cos[index] = Math.cos(phase);
    sin[index] = Math.sin(phase);
  }
}
const hannSum = hann.reduce((sum, weight) => sum + weight, 0);

function fft(real, imaginary, samples, start) {
  for (let index = 0; index < FFT_SIZE; index++) {
    const sample = bitReversal[index];
    real[index] = (samples[start + sample] ?? 0) * hann[sample];
    imaginary[index] = 0;
  }
  for (let length = 2; length <= FFT_SIZE; length *= 2) {
    const half = length / 2;
    const step = FFT_SIZE / length;
    for (let first = 0; first < FFT_SIZE; first += length) {
      for (let offset = 0; offset < half; offset++) {
        const phase = offset * step;
        const left = first + offset;
        const right = left + half;
        const r = real[right] * cos[phase] - imaginary[right] * sin[phase];
        const i = real[right] * sin[phase] + imaginary[right] * cos[phase];
        real[right] = real[left] - r;
        imaginary[right] = imaginary[left] - i;
        real[left] += r;
        imaginary[left] += i;
      }
    }
  }
}

function dbfs(power) {
  return Math.max(FLOOR_DBFS, Math.round(100 * Math.log10(power)) / 10);
}

export function analyze(samples, rate) {
  const frames = 1 + Math.max(0, Math.floor((samples.length - FFT_SIZE) / HOP));
  const stride = Math.ceil(frames / MAX_ROWS);
  const rows = Math.ceil(frames / stride);
  const spectrum = new Float32Array(BINS);
  const waterfall = new Float32Array(rows * BINS);
  const mean = new Float64Array(BINS);
  const real = new Float64Array(FFT_SIZE);
  const imaginary = new Float64Array(FFT_SIZE);
  let peak = 0;

  for (let frame = 0; frame < frames; frame++) {
    fft(real, imaginary, samples, frame * HOP);
    const raster = frame % stride === 0;
    const offset = frame / stride * BINS;
    for (let bin = 0; bin < BINS; bin++) {
      const scale = (bin === 0 || bin === BINS - 1 ? 1 : 2) / hannSum;
      const power = (real[bin] ** 2 + imaginary[bin] ** 2) * scale ** 2;
      mean[bin] += power;
      peak = Math.max(peak, power);
      if (raster) waterfall[offset + bin] = dbfs(power);
    }
  }
  for (let bin = 0; bin < BINS; bin++) spectrum[bin] = dbfs(mean[bin] / frames);
  return {
    bins: BINS, rows, frequency_start_hz: 0, bin_hz: rate / FFT_SIZE,
    time_start_s: FFT_SIZE / (2 * rate), time_step_s: HOP * stride / rate,
    floor_dbfs: FLOOR_DBFS, ceiling_dbfs: Math.max(0, dbfs(peak)),
    spectrum_dbfs: spectrum, waterfall_dbfs: waterfall,
  };
}
