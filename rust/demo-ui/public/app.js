"use strict";
const $ = (id) => document.getElementById(id);
let config, source = "native", level = 3, result, busy = false;
let raster, wave, spectrum, spectrumLayer;
let streamController;
const themePreference = matchMedia("(prefers-color-scheme: dark)");
function applyTheme(theme) {
  document.documentElement.dataset.theme = theme;
  const dark = theme === "dark";
  $("theme-toggle").setAttribute("aria-pressed", String(dark));
  $("theme-toggle").title = dark ? "Switch to light mode" : "Switch to dark mode";
  for (const plot of [raster, wave, spectrum]) {
    plot?.change_settings({colors:{bg:dark ? "#06121b" : "#081a25",fg:dark ? "#aec6cf" : "#91abb9"}});
  }
}
$("theme-toggle").addEventListener("click", () => {
  const theme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
  try { localStorage.setItem("linkwise-theme", theme); } catch { /* Session-only preference when storage is unavailable. */ }
  applyTheme(theme);
});
themePreference.addEventListener("change", event => {
  let saved;
  try { saved = localStorage.getItem("linkwise-theme"); } catch { /* Use system preference. */ }
  if (saved !== "light" && saved !== "dark") applyTheme(event.matches ? "dark" : "light");
});
applyTheme(document.documentElement.dataset.theme || "light");
const palette = [
  {pos:0,red:4,green:8,blue:15}, {pos:25,red:10,green:18,blue:40},
  {pos:50,red:10,green:43,blue:53}, {pos:75,red:35,green:72,blue:60},
  {pos:100,red:100,green:90,blue:40}
];
const plotOptions = {
  all: true, cmode: "RE", legend: false, no_legend_button: true,
  autohide_panbars: true, colors: {bg:"#081a25",fg:"#91abb9"},
  cross: true, autox: 0, autoy: 0, xmin:0, xmax:3000,
};
function status(text, error = false) { $("request-state").textContent = text; $("request-state").classList.toggle("error",error); }
function updateControls() {
  $("native-fields").hidden = source !== "native";
  $("reference-fields").hidden = source !== "reference";
  for (const name of ["native","reference"]) {
    $("source-"+name).classList.toggle("selected",source === name);
    $("source-"+name).setAttribute("aria-pressed",String(source === name));
  }
  const active = config?.levels.find(item=>item.level===level);
  for (const button of document.querySelectorAll("[data-level]")) {
    const number = Number(button.dataset.level);
    button.classList.toggle("selected",number===level);
    button.setAttribute("aria-pressed",String(number===level));
    button.disabled = config ? !config.levels.find(item=>item.level===number)?.available : false;
    if (button.disabled) button.title = "Run python3 tools/fetch-modem-profile.py to enable level 4, then restart the demo.";
  }
  if (active) {
    $("level-help").textContent = `${active.frame_bytes} bytes per frame · up to ${active.max_frames} frame${active.max_frames===1?"":"s"}`;
    const bytes = new TextEncoder().encode($("message").value).length;
    $("byte-count").textContent = `${bytes} / ${active.max_bytes} B`;
    $("byte-count").style.color = bytes > active.max_bytes ? "#b34d33" : "";
  }
  $("snr").disabled = !$("noise").checked;
  $("snr-value").textContent = `${$("snr").value} dB`;
  $("mode-note").textContent = source === "native" ? "Local codec loopback. No RF transmission or live VARA session." : "Published stock VARA recording. Receive-only replay through the Rust decoder.";
  $("run-label").textContent = busy ? "Running Rust DSP…" : source==="native" ? "Run modem" : "Decode recording";
  $("run").disabled = busy || !config || (source==="reference" && !$("fixture").value);
}
function clearPlot(plot) { if (plot) plot.deoverlay(); }
function setupPlots() {
  if (!window.sigplot) throw new Error("The local SigPlot bundle could not load.");
  raster = new sigplot.Plot($("waterfall"), {...plotOptions, ylabel:"Time (s)",xlabel:"Frequency (Hz)",cmap:palette,autoz:0,zmin:-100,zmax:0,ymin:0,ymax:6,rasterSmoothing:false});
  wave = new sigplot.Plot($("waveform"), {...plotOptions,xlabel:"Time (s)",ylabel:"Amplitude",xmin:0,xmax:6,ymin:-0.2,ymax:0.2});
  spectrum = new sigplot.Plot($("spectrum"), {...plotOptions,xlabel:"Frequency (Hz)",ylabel:"dBFS",ymin:-110,ymax:0});
  applyTheme(document.documentElement.dataset.theme);
  const observer = new ResizeObserver(() => {
    requestAnimationFrame(()=>[raster,wave,spectrum].forEach(plot=>plot?.checkresize()));
  });
  ["waterfall","waveform","spectrum"].forEach(id=>observer.observe($(id)));
}
function setSlice(row) {
  if (!result) return;
  const analysis=result.analysis;
  row=Math.max(0,Math.min(analysis.rows-1,Number(row)));
  const values=new Float32Array(analysis.waterfall_dbfs.slice(row*analysis.bins,(row+1)*analysis.bins));
  spectrum.reload(spectrumLayer,values);
  $("slice").value=String(row);
  const time=analysis.time_start_s+row*analysis.time_step_s;
  $("slice-time").textContent=`${time.toFixed(2)} s`;
  $("spectrum-caption").textContent=`At ${time.toFixed(3)} s · ${analysis.bin_hz.toFixed(2)} Hz bins`;
}
function td(text, className) { const cell=document.createElement("td");cell.textContent=text;if(className)cell.className=className;return cell; }
function render(report) {
  result=report;
  const a=report.analysis;
  $("plot-empty").hidden=true;
  clearPlot(raster);clearPlot(wave);clearPlot(spectrum);
  raster.change_settings({xmin:0,xmax:3000,ymin:0,ymax:report.duration_s,autox:0,autoy:0,zmin:-100,zmax:0,autoz:0});
  raster.overlay_array(new Float32Array(a.waterfall_dbfs),{type:2000,format:"SF",subsize:a.bins,xstart:0,xdelta:a.bin_hz,xunits:3,ystart:a.time_start_s,ydelta:a.time_step_s,yunits:1},{name:"Received audio · dBFS"});
  const amplitude=Math.max(0.15,Math.min(2,report.peak*1.12));
  wave.change_settings({xmin:0,xmax:report.duration_s,ymin:-amplitude,ymax:amplitude,autox:0,autoy:0});
  wave.overlay_array(new Float32Array(a.envelope.min),{type:1000,format:"SF",xstart:0,xdelta:a.envelope.step_s,xunits:1},{name:"Minimum",color:"#20a5a2"});
  wave.overlay_array(new Float32Array(a.envelope.max),{type:1000,format:"SF",xstart:0,xdelta:a.envelope.step_s,xunits:1},{name:"Maximum",color:"#65d9b4"});
  spectrumLayer=spectrum.overlay_array(new Float32Array(a.bins).fill(-120),{type:1000,format:"SF",xstart:0,xdelta:a.bin_hz,xunits:3},{name:"Selected spectrum",color:"#73ddbb"});
  $("slice").max=String(a.rows-1);$("slice").disabled=false;setSlice(a.selected_row);
  $("metric-frames").replaceChildren(document.createTextNode(`${report.matched_frames}/${report.expected_frames}`),Object.assign(document.createElement("small"),{textContent:"verified"}));
  $("metric-duration").replaceChildren(document.createTextNode(report.duration_s.toFixed(2)),Object.assign(document.createElement("small"),{textContent:"seconds"}));
  $("metric-elapsed").replaceChildren(document.createTextNode(report.elapsed_ms.toFixed(0)),Object.assign(document.createElement("small"),{textContent:"ms"}));
  $("verification-badge").className=`verification ${report.payload_verified?"ok":"fail"}`;
  $("verification-badge").textContent=report.payload_verified?"PAYLOAD VERIFIED":report.decoded_frames?"PARTIAL DECODE":"NO VALID FRAMES";
  $("result-light").className=`result-light ${report.payload_verified?"ok":"fail"}`;
  $("decoded-payload").textContent=report.decoded_text ?? (report.frames.length?report.frames.map(frame=>frame.text).join("\n"):"No CRC-valid payload.");
  $("result-detail").textContent=report.payload_verified?`${report.decoded_frames} CRC-valid frame${report.decoded_frames===1?"":"s"}. Every payload byte matches the expected input.`:`${report.decoded_frames} of ${report.expected_frames} expected frames decoded. Missing bytes remain unverified.`;
  $("result-source").textContent=report.source==="native"?"Native loopback":"Stock audio replay";
  $("result-level").textContent=`BW500 · level ${report.level}`;
  $("result-channel").textContent=report.measured_snr_db!==null?`${report.measured_snr_db.toFixed(1)} dB SNR${report.dropout?" + dropout":""}`:report.dropout?"Dropout":"Clean";
  $("result-candidates").textContent=String(report.candidate_count);
  $("plot-description").textContent=`${report.source==="native"?"Native Rust TX → RX":"Stock recording → Rust RX"} · ${a.fft_size}-point Hann STFT`;
  const audioUrl=`/api/runs/${encodeURIComponent(report.id)}/audio.wav`;
  $("download").href=audioUrl;$("download").classList.remove("disabled");$("download").setAttribute("aria-disabled","false");
  $("audio").pause();$("audio").src=audioUrl;
  const rows=$("frame-rows");rows.replaceChildren();
  for (const frame of report.frames) {
    const tr=document.createElement("tr");
    tr.append(td(frame.slot===null?"?":String(frame.slot+1)),td(`L${frame.level}`),td(`${frame.start_s.toFixed(3)}–${frame.end_s.toFixed(3)} s`),td(`0x${frame.control.toString(16).padStart(2,"0")}`),td(frame.payload_hex,"hex"),td(frame.matches_expected?"EXACT":"MISMATCH",frame.matches_expected?"matched":""));
    tr.querySelector(".hex").title=frame.payload_hex;rows.append(tr);
  }
  if (!report.frames.length) {const tr=document.createElement("tr");const cell=td("No CRC-valid frames returned by the receiver.");cell.colSpan=6;tr.append(cell);rows.append(tr);}
  $("event-log").replaceChildren();
  for (const event of report.events) {const li=document.createElement("li");const time=document.createElement("time");time.textContent=`${event.at_ms.toFixed(1)} ms`;const detail=document.createElement("span");detail.textContent=`${event.stage.toUpperCase()} · ${event.detail}`;li.append(time,detail);$("event-log").append(li);}
  $("trace-meta").textContent=`${report.sample_count.toLocaleString()} samples · seed ${report.seed}`;
  $("provenance").textContent=report.input_sha256?`Original reference WAV SHA-256: ${report.input_sha256}. Any selected impairments are applied after source verification.`:"Synthetic native waveform. The demo knows the message length and frame schedule for comparison; it does not implement a live ARQ or Winlink delivery session.";
  status(`Run complete · ${report.elapsed_ms.toFixed(0)} ms · ${report.payload_verified?"payload verified":"verification incomplete"}`);
}
function renderProgress(data, first) {
  const a = data.analysis;
  if (first) {
    result = undefined;
    $("audio").pause();$("audio").removeAttribute("src");$("audio").load();
    $("download").removeAttribute("href");$("download").classList.add("disabled");$("download").setAttribute("aria-disabled","true");
    $("verification-badge").className="verification neutral";
    $("verification-badge").textContent="RECEIVING SAMPLES";
    $("result-light").className="result-light";
    $("frame-rows").replaceChildren();$("event-log").replaceChildren();
    $("metric-elapsed").textContent="Streaming";
    $("result-source").textContent=source==="native"?"Native loopback":"Stock audio replay";
    $("result-level").textContent="Receiving";$("result-channel").textContent="Pending final analysis";$("result-candidates").textContent="—";
    $("slice").disabled=true;
    clearPlot(raster);clearPlot(wave);clearPlot(spectrum);
  }
  $("plot-empty").hidden=true;
  raster.change_settings({xmin:0,xmax:3000,ymin:0,ymax:data.duration_s,autox:0,autoy:0,zmin:-100,zmax:0,autoz:0});
  clearPlot(raster);
  raster.overlay_array(new Float32Array(a.waterfall_dbfs),{type:2000,format:"SF",subsize:a.bins,xstart:0,xdelta:a.bin_hz,xunits:3,ystart:a.time_start_s,ydelta:a.time_step_s,yunits:1});
  clearPlot(wave);
  const peak=Math.max(.15,...a.envelope.min.map(Math.abs),...a.envelope.max.map(Math.abs));
  wave.change_settings({xmin:0,xmax:data.duration_s,ymin:-peak*1.1,ymax:peak*1.1,autox:0,autoy:0});
  for(const [key,color] of [["min","#20a5a2"],["max","#65d9b4"]])wave.overlay_array(new Float32Array(a.envelope[key]),{type:1000,format:"SF",xstart:0,xdelta:a.envelope.step_s,xunits:1},{color});
  clearPlot(spectrum);
  spectrum.overlay_array(new Float32Array(a.waterfall_dbfs.slice(-a.bins)),{type:1000,format:"SF",xstart:0,xdelta:a.bin_hz,xunits:3},{color:"#73ddbb"});
  const seconds=data.received_samples/48000;
  $("metric-duration").textContent=`${seconds.toFixed(2)} s`;
  $("metric-frames").textContent=`${data.decoded_frames} CRC-valid`;
  $("decoded-payload").textContent=data.decoded_text||"Waiting for a complete CRC-valid frame…";
  $("result-detail").textContent="Streaming receiver output. Exact payload verification runs after end of stream.";
  $("plot-description").textContent=`Streaming Rust RX · ${data.received_samples.toLocaleString()} / ${data.total_samples.toLocaleString()} samples`;
  $("spectrum-caption").textContent=`Latest received window · ${seconds.toFixed(2)} s`;
  $("provenance").textContent="Local audio paced at 48 kHz through the Rust receiver; no RF or live VARA session.";
  status(`Receiving · ${Math.round(100*data.received_samples/data.total_samples)}% · ${data.decoded_frames} CRC-valid frames`);
}
async function execute(event) {
  event?.preventDefault();if(busy||!config)return;
  busy=true;document.body.classList.add("busy");updateControls();
  const streaming=$("streaming").checked;
  streamController=new AbortController();$("stop-stream").hidden=!streaming;
  status(streaming?"Starting audio stream…":"Running the Rust encoder, channel and receiver…");
  try {
    const response=await fetch(streaming?"/api/stream":"/api/run",{method:"POST",signal:streamController.signal,headers:{"Content-Type":"application/json"},body:JSON.stringify({source,level,message:$("message").value,fixture:$("fixture").value||null,snr_db:$("noise").checked?Number($("snr").value):null,dropout:$("dropout").checked,seed:20261109})});
    if(!response.ok){const data=await response.json();throw new Error(data.error||`Request failed (${response.status})`);}
    if(!streaming){render(await response.json());return;}
    const reader=response.body.getReader(),decoder=new TextDecoder();let pending="",first=true,complete=false;
    while(true){
      const {value,done}=await reader.read();pending+=decoder.decode(value||new Uint8Array(),{stream:!done});
      let index;
      while((index=pending.indexOf("\n"))>=0){
        const line=pending.slice(0,index);pending=pending.slice(index+1);if(!line)continue;
        const message=JSON.parse(line);
        if(message.type==="error")throw new Error(message.error);
        if(message.type==="progress"){renderProgress(message.data,first);first=false;}
        if(message.type==="complete"){render(message.data);complete=true;}
      }
      if(done)break;
    }
    if(!complete)throw new Error("Stream ended before final verification.");
  } catch(error) {
    if(error.name==="AbortError"){
      status("Stream stopped · payload not verified");
      $("verification-badge").className="verification neutral";$("verification-badge").textContent="STREAM STOPPED";
    }else status(error.message,true);
  }
  finally {streamController=undefined;$("stop-stream").hidden=true;busy=false;document.body.classList.remove("busy");updateControls();}
}
$("stop-stream").addEventListener("click",()=>streamController?.abort());
async function init() {
  try {
    const response=await fetch("/api/config");if(!response.ok)throw new Error("Rust backend is unavailable.");config=await response.json();
    $("sigplot-version").textContent=config.sigplot_version;
    for(const fixture of config.fixtures) {const option=document.createElement("option");option.value=fixture.available?fixture.id:"";option.textContent=fixture.label+(fixture.available?"":" · unavailable");option.disabled=!fixture.available;$("fixture").append(option);}
    const first=config.fixtures.find(item=>item.available);if(first)$("fixture").value=first.id;
    else {$("reference-help").textContent="Fetch recordings with: python3 tools/test-modem.py. Then reload this page.";}
    setupPlots();updateControls();await execute();
  } catch(error) {status(error.message,true);}
}
for(const name of ["native","reference"])$("source-"+name).addEventListener("click",()=>{source=name;updateControls();status("Settings changed · run to apply");});
for(const button of document.querySelectorAll("[data-level]"))button.addEventListener("click",()=>{level=Number(button.dataset.level);updateControls();status("Settings changed · run to apply");});
for(const id of ["message","snr","noise","dropout","fixture"])$(id).addEventListener("input",()=>{updateControls();status("Settings changed · run to apply");});
$("run-form").addEventListener("submit",execute);
$("slice").addEventListener("input",event=>setSlice(event.target.value));
$("audio").addEventListener("timeupdate",()=>{if(result&&!$("audio").paused)setSlice(Math.round(($("audio").currentTime-result.analysis.time_start_s)/result.analysis.time_step_s));});
init();
