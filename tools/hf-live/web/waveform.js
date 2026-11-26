const format = new Intl.NumberFormat("en-US", { maximumFractionDigits: 3 });
const rfFormat = new Intl.NumberFormat("en-US", { maximumFractionDigits: 6, useGrouping: false });
const HZ_PER_MHZ = 1_000_000;
const HISTORY_ROWS = 192;
const FLOOR_DBFS = -120;
const DEFAULT_CEILING_DBFS = 18;
const RX_RETRIES = 8;
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
const coarsePointer = matchMedia("(pointer: coarse)");
const paletteColors = {
  spectrum: ["#020204", "#05072a", "#0a1461", "#1440ab", "#1481da", "#0ec6e2", "#30dc65", "#f8e638", "#fb3c23"],
  ocean: ["#060e22", "#18386b", "#168194", "#49bb9e", "#fbe487"],
  gray: ["#000000", "#ffffff"],
};

function toPalette(colors) {
  return colors.map((color, i) => ({
    pos: 100 * i / (colors.length - 1),
    red: 100 * parseInt(color.slice(1, 3), 16) / 255,
    green: 100 * parseInt(color.slice(3, 5), 16) / 255,
    blue: 100 * parseInt(color.slice(5, 7), 16) / 255,
  }));
}

function spectralLevels(values, count) {
  if (!(Array.isArray(values) || values instanceof Float32Array)
      || values.length !== count || !values.every(Number.isFinite)) return null;
  return values instanceof Float32Array ? values : Float32Array.from(values);
}

function decodeAnalysis(value) {
  if (!value) return null;
  const { bins, rows, bin_hz: binHz, time_start_s: start, time_step_s: step,
    floor_dbfs: floor, ceiling_dbfs: ceiling } = value;
  const firstHz = value.frequency_start_hz ?? 0;
  if (!Number.isSafeInteger(bins) || bins < 2 || !Number.isSafeInteger(rows) || rows < 1
      || !Number.isSafeInteger(bins * rows) || !Number.isFinite(binHz) || binHz <= 0
      || !Number.isFinite(firstHz) || firstHz < 0
      || !Number.isFinite(start) || start < 0 || !Number.isFinite(step) || step <= 0
      || !Number.isFinite(floor) || !Number.isFinite(ceiling) || ceiling <= floor) return null;
  const spectrum = spectralLevels(value.spectrum_dbfs, bins);
  const waterfall = spectralLevels(value.waterfall_dbfs, bins * rows);
  return spectrum && waterfall ? { bins, rows, firstHz, binHz, start, step, floor, ceiling, spectrum, waterfall } : null;
}

function decodeCapture(value, station, stage, attemptId = null) {
  const { attempt, sample_count: count, sample_rate_hz: rate, waveform_f32le: encoded, waveform_f32: transferred } = value;
  if (attempt?.source !== station || attempt.id == null || (attemptId !== null && String(attempt.id) !== attemptId)
      || (value.stage ?? "tx") !== stage || !Number.isSafeInteger(count) || count < 1
      || !Number.isFinite(rate) || rate <= 0) throw Error("Invalid waveform capture");
  let samples = transferred;
  if (samples instanceof Float32Array) {
    if (samples.length !== count) throw Error("Incomplete waveform capture transfer");
  } else {
    if (typeof encoded !== "string" || encoded.length !== 4 * Math.ceil(count * 4 / 3)) {
      throw Error("Invalid waveform capture encoding");
    }
    const binary = atob(encoded);
    if (binary.length !== count * 4) throw Error("Incomplete waveform capture");
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < bytes.length; i++) bytes[i] = binary.charCodeAt(i);
    const view = new DataView(bytes.buffer);
    samples = new Float32Array(count);
    for (let i = 0; i < count; i++) samples[i] = view.getFloat32(i * 4, true);
  }
  let peak = 0;
  for (const sample of samples) {
    if (!Number.isFinite(sample)) throw Error("Invalid waveform sample");
    peak = Math.max(peak, Math.abs(sample));
  }

  const diagnosis = {
    ...attempt,
    channel_model: value.channel_model ?? attempt.channel_model,
    measured_snr_db: value.measured_snr_db ?? attempt.measured_snr_db,
    measured_sinr_db: value.measured_sinr_db ?? attempt.measured_sinr_db,
    decode_status: value.decode_status ?? attempt.decode_status,
    profile: value.profile ?? attempt.profile,
    interferers: value.interferers ?? attempt.interferers,
  };
  return { attempt, stage, diagnosis, rate, samples, peak, analysis: decodeAnalysis(value.analysis) };
}

function sameGrid(a, b) {
  return a && b && a.bins === b.bins && a.firstHz === b.firstHz && a.binHz === b.binHz;
}

function projectRf(capture, centerHz) {
  const audioCenterHz = 1500;
  const sidebandLossDb = 20 * Math.log10(2);
  const { analysis, rate } = capture;
  if (!analysis || !Number.isFinite(centerHz) || centerHz <= 0 || analysis.firstHz !== 0) return null;
  if (rate !== 8000 && rate !== 48000) return null;

  const { bins, binHz, floor } = analysis;
  const hasNyquist = Math.abs((bins - 1) * binHz - rate / 2) < 1e-6;
  const end = bins - Number(hasNyquist);
  if (end < 2) return null;

  const usb = rate === 48000;
  const dialHz = usb ? centerHz - audioCenterHz : centerHz;
  const rfBins = usb ? end - 1 : 2 * end - 1;
  const firstHz = usb ? dialHz + binHz : centerHz - (end - 1) * binHz;

  function projectRows(input) {
    const result = new Float32Array(input.length / bins * rfBins);
    for (let src = 0, dest = 0; src < input.length; src += bins, dest += rfBins) {
      if (usb) {
        // Ideal upper sideband keeps the positive audio bins at unity gain.
        result.set(input.subarray(src + 1, src + end), dest);
      } else {
        // A unity cosine mixer splits each non-DC tone between two sidebands.
        // A finite packet's baseband DC component maps to the carrier once.
        const middle = dest + end - 1;
        result[middle] = input[src];
        for (let offset = 1; offset < end; offset++) {
          const level = Math.max(floor, input[src + offset] - sidebandLossDb);
          result[middle - offset] = level;
          result[middle + offset] = level;
        }
      }
    }
    return result;
  }

  const spectrum = projectRows(analysis.spectrum);
  const waterfall = projectRows(analysis.waterfall);
  const ceiling = waterfall.reduce((max, level) => Math.max(max, level), 0);
  return { ...analysis, bins: rfBins, firstHz, centerHz, dialHz,
    mode: usb ? "USB" : "DSB", spectrum, waterfall, ceiling };
}

export function createWaveformPanel({ bandName, bandFrequencyHz, stationName, requestCapture }) {
  const field = (name) => document.getElementById(`waveform-${name}`);
  const panel = document.getElementById("waveform");
  const stationLabel = field("station");
  const compare = field("compare");
  const follow = field("follow");
  const captureView = field("capture");
  const status = field("status");
  const reset = field("reset");
  const mode = field("raster-mode");
  const palette = field("palette");
  const levelRange = field("level-range");
  const rangeLabel = field("range");
  const viewNames = ["signal", "time"];
  const bandHistories = new Map();
  let station = "";
  let capture = null;
  let pair = null;
  let shownStage = "tx";
  let following = true;
  let lastOutcome = null;
  let shown = "signal";
  let request = null;
  let refreshPending = null;
  let receiverRequest = null;
  let receiverTimer = 0;
  let history = null;
  let wave = null;
  let waveCapture = null;
  let spectrum = null;
  let spectrumLayers = null;
  let spectrumCapture = null;
  let waterfall = null;
  let waterfallLayer = null;
  let processedHistory = null;
  let rasterFloor = null;
  let rasterCeiling = null;
  let pushedRows = 0;
  let queuedFrame = 0;
  let playbackFrame = 0;
  let visibleWindows = 0;
  let cursorMHz = null;

  function setStatus(message, failed = false) {
    if (status.textContent !== message) status.textContent = message;
    status.hidden = !message;
    status.dataset.error = String(failed);
  }

  function clearCapture() {
    cancelReceiver();
    cancelAnimationFrame(playbackFrame);
    playbackFrame = 0;
    visibleWindows = 0;
    cursorMHz = null;
    capture = null;
    pair = null;
    compare.hidden = true;
    bandHistories.clear();
    history = null;
    processedHistory = null;
    waveCapture = null;
    spectrumCapture = null;
    spectrumLayers = null;
    clearRaster();
    wave?.deoverlay();
    spectrum?.deoverlay();
    captureView.hidden = true;
    reset.hidden = true;
  }

  function makePlot(host, xlabel, ylabel, extra = {}) {
    const element = field(host);
    // SigPlot consumes touch events for its drag controls. Phones use the
    // explicit frequency buttons and can scroll the page from either plot.
    for (const eventName of ["touchstart", "touchmove", "touchend", "touchcancel"]) {
      element.addEventListener(eventName, (event) => {
        if (coarsePointer.matches) event.stopPropagation();
      }, { capture: true, passive: true });
    }
    const plot = new window.sigplot.Plot(element, {
      all: true, cmode: "RE", legend: false, no_legend_button: true,
      autohide_panbars: true, noreadout: true, xmult: 1, xlabel, ylabel,
      colors: { bg: "#050708", fg: "#e4edf1" }, ...extra,
    });
    new ResizeObserver(() => {
      if (element.clientHeight) plot.checkresize();
    }).observe(element);
    return plot;
  }

  function drawWaveform(resetZoom = false) {
    reset.hidden = false;
    wave ??= makePlot("plot", "Time (s)", "Amplitude");
    if (capture === waveCapture && !resetZoom) {
      wave.checkresize();
      return;
    }
    const { samples, rate, peak } = capture;
    const amplitude = Math.max(.01, peak * 1.08);
    wave.deoverlay();
    wave.change_settings({ xmin: 0, xmax: samples.length / rate, ymin: -amplitude, ymax: amplitude, autox: 0, autoy: 0 });
    wave.overlay_array(samples, {
      type: 1000, format: "SF", xstart: 0, xdelta: 1 / rate, xunits: 1,
    }, { name: capture.stage === "tx" ? "Transmitted samples" : "Received samples", color: "#71e9c1" });
    waveCapture = capture;
    wave.checkresize();
  }

  function prepareHistory(next) {
    const analysis = next.analysis;
    if (!analysis) {
      history = null;
      cursorMHz = null;
      return;
    }
    const band = String(next.attempt.band);
    const { bins, firstHz, binHz } = analysis;
    let entry = following ? bandHistories.get(band) : null;
    if (!sameGrid(entry, analysis)) {
      entry = { bins, firstHz, binHz, data: new Float32Array(HISTORY_ROWS * bins), rows: 0, total: 0, last: null };
      if (following) bandHistories.set(band, entry);
    }
    history = entry;
    const id = String(next.attempt.id);
    if (entry.last === id) visibleWindows = analysis.rows;
    entry.last = id;
  }

  function appendWindows(end) {
    if (end <= visibleWindows) return;
    const { bins, waterfall: levels } = capture.analysis;
    const added = Math.min(end - visibleWindows, HISTORY_ROWS);
    const kept = Math.min(history.rows, HISTORY_ROWS - added);
    history.data.copyWithin(0, (history.rows - kept) * bins, history.rows * bins);
    history.data.set(levels.subarray((end - added) * bins, end * bins), kept * bins);
    history.rows = kept + added;
    history.total += end - visibleWindows;
    visibleWindows = end;
  }

  function displayedWindow() {
    const { waterfall: levels, bins } = capture.analysis;
    const row = Math.max(0, visibleWindows - 1);
    return levels.subarray(row * bins, (row + 1) * bins);
  }

  function readout() {
    if (!capture?.analysis) return;
    const { firstHz, binHz, spectrum: mean } = capture.analysis;
    const levels = displayedWindow();
    const targetHz = cursorMHz === null ? firstHz : cursorMHz * HZ_PER_MHZ;
    let bin = Math.min(levels.length - 1, Math.max(0, Math.round((targetHz - firstHz) / binHz)));
    if (cursorMHz === null) {
      for (let index = 1; index < levels.length; index++) {
        if (levels[index] > levels[bin]) bin = index;
      }
    }
    const label = cursorMHz === null ? "Window peak" : "Cursor";
    field("measurement").value = `${label} · ${rfFormat.format((firstHz + bin * binHz) / HZ_PER_MHZ)} MHz · ${format.format(levels[bin])} dBFS · Mean ${format.format(mean[bin])} dBFS`;
  }

  function updateWindowDisplay() {
    if (shown !== "signal" || !capture?.analysis || !spectrumLayers) return;
    spectrum.reload(spectrumLayers[1], displayedWindow());
    showWindowCount();
    readout();
    queueRaster();
  }

  function finishPlayback() {
    if (playbackFrame) cancelAnimationFrame(playbackFrame);
    playbackFrame = 0;
    if (capture?.analysis) appendWindows(capture.analysis.rows);
    field("playback").textContent = following ? "Latest burst" : "Matched burst";
  }

  function streamWindows() {
    const { rows, start, step } = capture.analysis;
    const began = performance.now();
    function frame(now) {
      playbackFrame = 0;
      const elapsed = (now - began) / 1000;
      const next = Math.min(rows, 1 + Math.floor((elapsed - start) / step));
      if (next > visibleWindows) {
        appendWindows(next);
        updateWindowDisplay();
      }
      if (visibleWindows < rows) playbackFrame = requestAnimationFrame(frame);
      else field("playback").textContent = "Latest burst";
    }
    field("playback").textContent = "Playing burst";
    playbackFrame = requestAnimationFrame(frame);
  }

  function displayFloor() {
    const span = Number(levelRange.value);
    return Number.isFinite(span) ? Math.max(FLOOR_DBFS, displayCeiling() - span) : FLOOR_DBFS;
  }

  function displayCeiling() {
    // Receiver sums can exceed full scale without clipping. Both captures of a
    // burst use the same upper bound, including any higher receiver peak.
    const peak = Math.max(pair?.tx.analysis?.ceiling ?? 0, pair?.rx?.analysis?.ceiling ?? 0);
    return Math.max(DEFAULT_CEILING_DBFS, 6 * Math.ceil(peak / 6));
  }

  function firstFrequency() {
    return history.firstHz / HZ_PER_MHZ;
  }

  function lastFrequency() {
    return (history.firstHz + (history.bins - 1) * history.binHz) / HZ_PER_MHZ;
  }

  function frequencyRange() {
    const { centerHz, mode: projection } = capture.analysis;
    const center = centerHz / HZ_PER_MHZ;
    const halfSpan = (projection === "USB" ? 1500 : 1000) / HZ_PER_MHZ;
    return { xmin: Math.max(firstFrequency(), center - halfSpan), xmax: Math.min(lastFrequency(), center + halfSpan) };
  }

  function updateFrequencyControls({ xmin, xmax }) {
    if (!history || !Number.isFinite(xmin) || !Number.isFinite(xmax)) return;
    if (pair) pair.range = { xmin, xmax };
    const width = lastFrequency() - firstFrequency();
    const span = xmax - xmin;
    const minimum = Math.min(width, 4 * history.binHz / HZ_PER_MHZ);
    const epsilon = 1e-10;
    const rangeText = `${rfFormat.format(xmin)} – ${rfFormat.format(xmax)}\u00a0MHz`;
    if (rangeLabel.value !== rangeText) rangeLabel.value = rangeText;
    field("pan-left").disabled = xmin <= firstFrequency() + epsilon;
    field("pan-right").disabled = xmax >= lastFrequency() - epsilon;
    field("zoom-in").disabled = span <= minimum + epsilon;
    field("zoom-out").disabled = span >= width - epsilon;
  }

  function adjustFrequency(scale, pan) {
    if (!capture?.analysis) return;
    const { xmin, xmax } = spectrum.get_view();
    const first = firstFrequency();
    const last = lastFrequency();
    const span = xmax - xmin;
    const nextSpan = Math.min(last - first, Math.max(4 * history.binHz / HZ_PER_MHZ, span * scale));
    const center = (xmin + xmax) / 2 + pan * span;
    const left = Math.max(first, Math.min(last - nextSpan, center - nextSpan / 2));
    const range = { xmin: left, xmax: left + nextSpan };
    spectrum.set_view(range);
    waterfall.set_view(range);
    updateFrequencyControls(range);
  }

  function makeSignalPlots() {
    if (spectrum) return;
    const zoom = { rubberbox_mode: "horizontal" };
    const label = "RF (MHz)";
    spectrum = makePlot("spectrum", label, "dBFS", zoom);
    waterfall = makePlot("waterfall", label, "Captured window", { ...zoom, rasterSmoothing: false });
    const mimic = { xzoom: true, xpan: true, unzoom: true };
    waterfall.mimic(spectrum, mimic);
    spectrum.mimic(waterfall, mimic);
    spectrum.addListener("mtag", (event) => {
      if (!capture?.analysis || !Number.isFinite(event.x) || Math.abs(event.w || 0) > capture.analysis.binHz / HZ_PER_MHZ) return;
      cursorMHz = event.x;
      readout();
    });
    for (const plot of [spectrum, waterfall]) {
      plot.addListener("zoom", updateFrequencyControls);
      plot.addListener("xpan", updateFrequencyControls);
      plot.addListener("unzoom", (event) => {
        if (event.level === 0 && history) {
          const range = frequencyRange();
          spectrum.set_view(range);
          waterfall.set_view(range);
          updateFrequencyControls(range);
        } else {
          updateFrequencyControls(event);
        }
      });
    }
  }

  function clearRaster() {
    if (queuedFrame) cancelAnimationFrame(queuedFrame);
    queuedFrame = 0;
    waterfall?.deoverlay();
    waterfallLayer = null;
  }

  function queueRaster() {
    if (queuedFrame) return;
    queuedFrame = requestAnimationFrame(() => {
      queuedFrame = 0;
      if (!history || !waterfallLayer || shown !== "signal" || !capture?.analysis) return;
      const retainedStart = history.total - history.rows;
      const start = Math.max(pushedRows, retainedStart) - retainedStart;
      if (start >= history.rows) return;
      const batch = history.data.subarray(start * history.bins, history.rows * history.bins);
      waterfall.push(waterfallLayer, batch, null, true, false);
      pushedRows = history.total;
      if (mode.value !== "scrolling") {
        waterfall.set_view({ ymin: Math.max(0, pushedRows - HISTORY_ROWS), ymax: pushedRows });
      }
    });
  }

  function rebuildRaster({ xmin, xmax }) {
    clearRaster();
    const colors = paletteColors[palette.value];
    const { bins, firstHz, binHz, total, rows } = history;
    const ceiling = displayCeiling();
    const floor = displayFloor();
    waterfall.change_settings({ zmin: floor, zmax: ceiling, autoz: 0, cmap: toPalette(colors) });
    waterfallLayer = waterfall.overlay_pipe({
      type: 2000, format: "SF", subsize: bins, lps: HISTORY_ROWS,
      pipesize: 4 * bins * HISTORY_ROWS,
      xstart: (firstHz - binHz / 2) / HZ_PER_MHZ, xdelta: binHz / HZ_PER_MHZ, xunits: 0,
      ystart: mode.value === "scrolling" ? 0 : total - rows, ydelta: 1, yunits: 0,
    }, { drawmode: mode.value, name: "Captured windows · dBFS" });
    waterfall.set_view({ xmin, xmax });
    rasterFloor = floor;
    rasterCeiling = ceiling;
    pushedRows = total - rows;
    field("floor").textContent = format.format(floor);
    field("ceiling").textContent = `${format.format(ceiling)} dBFS`;
    field("ramp").style.background = `linear-gradient(90deg, ${colors.join(", ")})`;
    // SigPlot allocates the raster image at its first paint after opening the pipe.
    queueRaster();
  }

  function drawSignal() {
    const { analysis } = capture;
    field("analysis-empty").hidden = Boolean(analysis);
    field("analyzer").hidden = !analysis;
    reset.hidden = !analysis;
    if (!analysis) return;
    makeSignalPlots();
    const changedGrid = history !== processedHistory;
    const { firstHz, binHz } = history;
    const ceiling = displayCeiling();
    const floor = displayFloor();
    const levelChanged = rasterFloor !== floor || rasterCeiling !== ceiling;
    const range = pair?.range ?? (changedGrid ? frequencyRange() : spectrum.get_view());
    if (changedGrid || capture !== spectrumCapture) {
      const latest = displayedWindow();
      if (changedGrid || !spectrumLayers) {
        spectrum.deoverlay();
        spectrum.change_settings({ ymin: floor, ymax: ceiling, autoy: 0 });
        const header = { type: 1000, format: "SF", xstart: firstHz / HZ_PER_MHZ, xdelta: binHz / HZ_PER_MHZ, xunits: 0 };
        spectrumLayers = [
          spectrum.overlay_array(analysis.spectrum, header, { name: "Burst mean", color: "#60ef90" }),
          spectrum.overlay_array(latest, header, { name: "Latest window", color: "#f578dd" }),
        ];
      } else {
        spectrum.reload(spectrumLayers[0], analysis.spectrum);
        spectrum.reload(spectrumLayers[1], latest);
      }
      spectrum.set_view(range);
      spectrumCapture = capture;
    }
    if (levelChanged && !changedGrid) {
      spectrum.change_settings({ ymin: floor, ymax: ceiling, autoy: 0 });
      spectrum.set_view(range);
    }
    if (changedGrid || !waterfallLayer || levelChanged || history.total - pushedRows > HISTORY_ROWS) {
      rebuildRaster(range);
    } else {
      queueRaster();
    }
    processedHistory = history;
    const dialLabel = `Dial ${rfFormat.format(analysis.dialHz / HZ_PER_MHZ)} MHz`;
    const model = analysis.mode === "USB" ? "Upper sideband" : "Double sideband";
    field("spectrum-detail").textContent = `${dialLabel} · ${model} · ${format.format(binHz)} Hz per bin · Tap or click to measure`;
    showWindowCount();
    field("waterfall-detail").textContent = following
      ? `Captured windows omit idle gaps. Latest burst used ${format.format(analysis.step)} s steps.`
      : `Capture windows use ${format.format(analysis.step)} s steps. TX and RX share the RF and dBFS scales.`;
    readout();
    updateFrequencyControls(spectrum.get_view());
    spectrum.checkresize();
    waterfall.checkresize();
  }

  function showWindowCount() {
    field("rows").textContent = following
      ? `${history.rows} / ${HISTORY_ROWS} windows` : `${history.rows} windows`;
  }

  function showView(view, moveFocus = false) {
    shown = view;
    for (const item of viewNames) {
      const tab = field(`${item}-tab`);
      const isShown = item === view;
      tab.setAttribute("aria-selected", String(isShown));
      tab.tabIndex = isShown ? 0 : -1;
      field(`${item}-view`).hidden = !isShown;
      if (isShown && moveFocus) tab.focus();
    }
    if (!capture) return;
    if (shown === "signal") drawSignal();
    else {
      reset.hidden = false;
      drawWaveform();
    }
  }

  function showCapture(next, animate = false) {
    const { attempt, samples, rate } = next;
    finishPlayback();
    const parts = [`To ${stationName(String(attempt.target))}`, bandName(String(attempt.band)), `M${attempt.message_id}`];
    if (attempt.waveform_mode) parts.push(attempt.waveform_mode);
    if (Number.isFinite(attempt.power_w)) parts.push(`${format.format(attempt.power_w)} W`);
    const started = attempt.started_at_s ?? attempt.simulated_at_s;
    if (Number.isFinite(started)) parts.push(`${format.format(started)} s sim time`);
    field("route").textContent = parts.join(" · ");
    field("count").textContent = format.format(samples.length);
    field("rate").textContent = `${format.format(rate)} Hz`;
    const airtime = Number.isFinite(attempt.airtime_s) ? attempt.airtime_s : samples.length / rate;
    field("duration").textContent = `${format.format(airtime)} s`;
    const { muf_mhz: muf, snr_adjustment_db: adjustment, drap_absorption_db: drap, forecast } = attempt.space_weather ?? {};
    const weather = [];
    if (Number.isFinite(muf)) weather.push(`Estimated MUF ${format.format(muf)} MHz`);
    if (Number.isFinite(adjustment)) weather.push(`Model SNR adjustment ${format.format(adjustment)} dB`);
    if (Number.isFinite(drap)) weather.push(`D-RAP absorption ${format.format(drap)} dB`);
    const weatherLabel = field("weather");
    weatherLabel.hidden = weather.length === 0;
    weatherLabel.textContent = weather.join(" · ");
    const forecastLabel = field("forecast");
    forecastLabel.hidden = !forecast;
    if (forecast) {
      const { status, snr_median_db: median, basic_muf_mhz: basicMuf } = forecast;
      const label = { fallback: "P.533 fallback to scenario", archived: "Archived P.533 forecast" };
      const prediction = [label[status] ?? "P.533 forecast"];
      if (Number.isFinite(median)) prediction.push(`Median SNR ${format.format(median)} dB`);
      if (Number.isFinite(basicMuf)) prediction.push(`Basic MUF ${format.format(basicMuf)} MHz`);
      forecastLabel.textContent = prediction.join(" · ");
    }
    field("caption").textContent = next.stage === "tx"
      ? "SigPlot · Source samples before the simulated channel"
      : "SigPlot · Receiver input samples after the simulated channel";
    capture = next;
    visibleWindows = 0;
    prepareHistory(next);
    const streaming = following && animate && next.analysis?.rows > 1 && shown === "signal" && !reducedMotion.matches;
    if (next.analysis) appendWindows(streaming ? 1 : next.analysis.rows);
    captureView.hidden = false;
    if (shown === "signal") drawSignal();
    else drawWaveform();
    if (streaming) streamWindows();
    setStatus("");
  }

  function sameAttempt(a, b) {
    return b && String(a.id) === String(b.id) && a.source === b.source && a.target === b.target
      && a.band === b.band && a.started_at_s === b.started_at_s;
  }

  function readStage(value, stationId, stage, attemptId = null) {
    const next = decodeCapture(value, stationId, stage, attemptId);
    next.centerHz = bandFrequencyHz(String(next.attempt.band));
    next.analysis = projectRf(next, next.centerHz);
    return next;
  }

  function renderComparison() {
    if (!pair) return;
    compare.hidden = false;
    for (const stage of ["tx", "rx"]) field(stage).setAttribute("aria-pressed", String(shownStage === stage));
    follow.setAttribute("aria-pressed", String(following));
    field("attempt").textContent = `Attempt #${pair.id}${following ? "" : " · Matched burst"}`;

    const { channel_model: model, profile, measured_snr_db: snr, measured_sinr_db: sinr,
      decode_status: status, interferers, interference } = pair.diagnosis;
    const models = { flat: "Flat fading", "watterson-moderate": "Watterson moderate" };
    const channel = field("channel");
    channel.title = profile ?? "";
    channel.textContent = model === "watterson-moderate" && profile
      ? `Watterson · ${profile.replace("ITU-R F.1487 mid-latitude moderate", "F.1487 moderate")}`
      : profile || models[model] || model || "—";
    const levels = [snr, sinr].map(value => Number.isFinite(value) ? format.format(value) : "—");
    field("snr").textContent = levels.every(value => value === "—") ? "—" : `${levels.join(" / ")} dB`;
    const decoded = field("decode");
    decoded.dataset.decode = status ?? "pending";
    const outcome = { delivered: "Delivered", failed: "Failed", pending: "Pending" }[status] ?? "Pending";
    if (decoded.textContent !== outcome) decoded.textContent = outcome;

    const effects = [];
    if (Array.isArray(interferers) && interferers.length) {
      effects.push(`${interferers.length} overlapping ${interferers.length === 1 ? "burst" : "bursts"}`);
    }
    const injection = { tone: "Tone interference", impulse: "Impulse interference", mixed: "Tone and impulse interference" };
    if (injection[interference]) effects.push(injection[interference]);
    const interferenceNote = field("interference");
    interferenceNote.hidden = effects.length === 0;
    interferenceNote.textContent = effects.join(" · ");
  }

  function renderStage() {
    renderComparison();
    const next = pair[shownStage];
    if (next) {
      showCapture(next);
      return;
    }
    captureView.hidden = true;
    reset.hidden = true;
    switch (pair.rxStatus) {
      case "missing":
        setStatus(`RX capture unavailable for attempt #${pair.id}.`, true);
        break;
      case "error":
        setStatus("Unable to load the matched RX capture. Select RX to retry.", true);
        break;
      case "pending":
        setStatus("Waiting for the receiver to finish this burst.");
        break;
      default:
        setStatus("Loading the matched RX capture…");
    }
  }

  function cancelReceiver() {
    clearTimeout(receiverTimer);
    receiverTimer = 0;
    receiverRequest?.abort();
    receiverRequest = null;
  }

  async function loadReceiver() {
    if (!pair || pair.rx || shownStage !== "rx" || receiverRequest) return;
    clearTimeout(receiverTimer);
    receiverTimer = 0;
    const expected = pair;
    const controller = new AbortController();
    receiverRequest = controller;
    try {
      const response = await requestCapture(station, { stage: "rx", attempt: expected.id, signal: controller.signal });
      if (receiverRequest !== controller) return;
      if (response.status === 204) {
        expected.rxStatus = "pending";
        if (expected.retries < RX_RETRIES) {
          expected.retries++;
          receiverTimer = setTimeout(loadReceiver, 1000);
        }
      } else if (response.status === 404) {
        expected.rxStatus = "missing";
      } else {
        if (!response.ok) throw Error(`HTTP ${response.status}`);
        const next = readStage(await response.json(), expected.tx.attempt.source, "rx", expected.id);
        if (receiverRequest !== controller) return;
        if (!sameAttempt(next.attempt, expected.tx.attempt)) throw Error("Mismatched receiver capture");
        if (!sameGrid(next.analysis, expected.tx.analysis)) next.analysis = null;
        expected.rx = next;
        expected.diagnosis = next.diagnosis;
      }
    } catch {
      if (receiverRequest !== controller) return;
      expected.rxStatus = "error";
    } finally {
      if (receiverRequest === controller) {
        receiverRequest = null;
        renderStage();
      }
    }
  }

  function cancelLatest() {
    request?.abort();
    request = null;
    refreshPending = null;
  }

  async function refresh(force = false, animate = true) {
    if (!following || !station || !window.sigplot?.Plot) return;
    if (request) {
      refreshPending = [Boolean(refreshPending?.[0]) || force, Boolean(refreshPending?.[1]) || animate];
      return;
    }
    const current = station;
    const controller = new AbortController();
    request = controller;
    try {
      const response = await requestCapture(current, {
        stage: "tx", after: force ? null : pair?.id, signal: controller.signal,
      });
      if (request !== controller) return;
      if (response.status === 204) {
        setStatus("");
        return;
      }
      if (response.status === 404) {
        clearCapture();
        setStatus("No transmitted burst captured yet.");
        return;
      }
      if (!response.ok) throw Error(`HTTP ${response.status}`);
      const next = readStage(await response.json(), current, "tx");
      if (request !== controller) return;
      const diagnosis = sameAttempt(next.attempt, lastOutcome) ? lastOutcome : next.diagnosis;
      if (pair && sameAttempt(next.attempt, pair.tx.attempt)) {
        pair.diagnosis = diagnosis;
        renderComparison();
        setStatus("");
        return;
      }
      const restarted = pair && (Number(next.attempt.id) < Number(pair.id)
        || (next.attempt.started_at_s ?? next.attempt.simulated_at_s)
          < (pair.tx.attempt.started_at_s ?? pair.tx.attempt.simulated_at_s));
      if (restarted) {
        finishPlayback();
        bandHistories.clear();
        history = null;
        capture = null;
      }
      const range = !restarted && sameGrid(pair?.tx.analysis, next.analysis) ? pair.range : null;
      if (!range) cursorMHz = null;
      pair = { id: String(next.attempt.id), tx: next, rx: null, diagnosis, range, rxStatus: "loading", retries: 0 };
      renderComparison();
      showCapture(next, animate);
    } catch {
      if (request === controller) {
        setStatus(capture ? "Waveform refresh failed. Showing the last capture." : "Unable to load the latest waveform.", true);
      }
    } finally {
      if (request === controller) {
        request = null;
        const pending = refreshPending;
        refreshPending = null;
        if (pending) void refresh(...pending);
      }
    }
  }

  function notify(attempt) {
    if (attempt.source !== station) return;
    if (attempt.state === "active") {
      void refresh();
      return;
    }
    lastOutcome = attempt;
    if (!pair || !sameAttempt(pair.tx.attempt, attempt)) return;
    pair.diagnosis = attempt;
    renderComparison();
    if (shownStage === "rx") void loadReceiver();
  }

  function switchStage(stage) {
    if (!pair) return;
    if (stage === shownStage) {
      if (stage === "rx") void loadReceiver();
      return;
    }
    finishPlayback();
    following = false;
    cancelLatest();
    cancelReceiver();
    shownStage = stage;
    renderStage();
    if (stage === "rx") void loadReceiver();
  }

  function select(id, force = false) {
    if (id === station) {
      if (force && following) void refresh(true, false);
      if (force && !following && shownStage === "rx") void loadReceiver();
      return;
    }
    cancelLatest();
    station = id;
    shownStage = "tx";
    following = true;
    lastOutcome = null;
    panel.toggleAttribute("data-selected", Boolean(station));
    clearCapture();
    if (!station) {
      stationLabel.textContent = "Select a station on the map or in the filter.";
      setStatus("");
      return;
    }
    stationLabel.textContent = `${stationName(station)} · ${station}`;
    if (!window.sigplot?.Plot) {
      setStatus("The local SigPlot bundle is unavailable.", true);
      return;
    }
    setStatus("Loading the latest transmission…");
    void refresh(false, false);
  }

  field("tx").addEventListener("click", () => switchStage("tx"));
  field("rx").addEventListener("click", () => switchStage("rx"));
  follow.addEventListener("click", () => {
    if (following || !pair) return;
    finishPlayback();
    cancelReceiver();
    following = true;
    shownStage = "tx";
    renderStage();
    void refresh(true, false);
  });

  viewNames.forEach((view, index) => {
    const tab = field(`${view}-tab`);
    tab.addEventListener("click", () => showView(view));
    tab.addEventListener("keydown", (event) => {
      const next = {
        ArrowLeft: (index + viewNames.length - 1) % viewNames.length,
        ArrowRight: (index + 1) % viewNames.length,
        Home: 0, End: viewNames.length - 1,
      };
      if (!Object.hasOwn(next, event.key)) return;
      event.preventDefault();
      showView(viewNames[next[event.key]], true);
    });
  });
  function changeRaster() {
    if (shown === "signal" && capture?.analysis) rebuildRaster(spectrum.get_view());
  }
  mode.addEventListener("change", changeRaster);
  palette.addEventListener("change", changeRaster);
  levelRange.addEventListener("change", () => {
    if (shown === "signal" && capture?.analysis) drawSignal();
  });
  field("pan-left").addEventListener("click", () => adjustFrequency(1, -.3));
  field("zoom-out").addEventListener("click", () => adjustFrequency(2, 0));
  field("zoom-in").addEventListener("click", () => adjustFrequency(.5, 0));
  field("pan-right").addEventListener("click", () => adjustFrequency(1, .3));
  reset.addEventListener("click", () => {
    if (shown === "time") {
      drawWaveform(true);
      return;
    }
    if (!capture?.analysis) return;
    cursorMHz = null;
    if (pair) pair.range = null;
    for (const plot of [spectrum, waterfall]) {
      plot.unzoom();
      plot.set_view(frequencyRange());
    }
    drawSignal();
  });
  return { select, notify };
}
