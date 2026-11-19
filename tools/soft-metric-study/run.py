"""Research-only soft metrics; production acquisition/FEC/CRC remain unchanged.

Usage: python run.py STUDY_ROOT OUTPUT_DIRECTORY
Requires numpy and prebuilt release rustfft/thiserror dependencies.
"""
import csv
import hashlib
import json
import shutil
import subprocess
import sys
import time
import wave
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[2]
STUDY, OUT = map(Path, sys.argv[1:])
OUT.mkdir(parents=True, exist_ok=False)
PROTOCOL = """Retrospective screen; all recordings were previously inspected.
Development: Nov15 sweep. Validation: Nov17 sweep, different transmissions.
No tuning on validation. Select highest development exact count, ties prefer
baseline then tone weighting then symbol weighting then combined.
Metrics: baseline; tone noise whitening; symbol reliability; combined;
pilot frequency tracking; impulsive-sample clipping.
Estimate each tone's normalized noise floor as its median over DATA columns.
Floor = max(estimate, 0.1 * median of tone estimates, 1e-12).
Tone whitening divides power by that floor, multiplies by median floor.
Symbol reliability = clamp(frame median of symbol noise / symbol noise, .1, 10).
Symbol noise = median of eight candidate tone powers in each DATA column.
Only level2 changes; acquisition .20, timing/CFO, max-log FEC and CRC fixed.
Exact KISS SHA256 oracle; unexpected payloads, clean controls and runtime logged.
This is offline frame-conditioned noise estimation, not a causal noise tracker.
SNR is recording power / injected 3kHz noise, not absolute RF SNR.
No production promotion without independent signals and mixed-stock interop.
Tracking: choose residual CFO -12,-6,0,6,12Hz using pilot power independently
in first/second frame halves; linearly interpolate those two CFO estimates.
This tests carrier tracking only, not a sample-clock tracking loop.
Clipping: bound aligned samples to six times median absolute frame amplitude.
This is a robust amplitude limiter, not a symbol-erasure decoder.
"""
(OUT / 'protocol.txt').write_text(PROTOCOL)
(OUT / 'revision.txt').write_text(subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True))
prototype = OUT / 'prototype'
prototype.mkdir()
modem = prototype / 'modem'
modem.mkdir()
for name in ('bw500', 'waveform'):
    shutil.copytree(REPO / 'rust/src/modem' / name, modem / name)
for name in ('bw500', 'waveform', 'coding', 'bw500_level4', 'bw500_waveform', 'receiver'):
    shutil.copy(REPO / f'rust/src/modem/{name}.rs', modem / f'{name}.rs')
path = modem / 'bw500.rs'
source = path.read_text()
needle = '    let mut coded = vec![0.0; mode.channel.len()];'
assert source.count(needle) == 1
prepass = '''
    let method = std::env::var("SOFT_METHOD").unwrap_or_default();
    let active = mode.level == Level::Two && (method == "tone" || method == "symbol" || method == "combined");
    fn median(mut x: Vec<f64>) -> f64 {
        x.sort_by(f64::total_cmp);
        let n = x.len();
        if n % 2 == 0 { (x[n/2-1] + x[n/2]) * 0.5 } else { x[n/2] }
    }
    let clipped;
    let samples = if mode.level == Level::Two && method == "clip" {
        let cap = 6.0 * median(samples.iter().map(|x| (*x as f64).abs()).collect()).max(1e-12);
        clipped = samples.iter().map(|x| x.clamp(-cap as f32, cap as f32)).collect::<Vec<_>>();
        clipped.as_slice()
    } else { samples };
    let base_hz = tones[0].quadrature[1].1.atan2(tones[0].quadrature[1].0)
        * SAMPLE_RATE_HZ as f64 / std::f64::consts::TAU
        - mode.first_bin as f64 * SAMPLE_RATE_HZ as f64 / mode.symbol as f64;
    let mut drift = [0.0; 2];
    if mode.level == Level::Two && method == "track" {
        for (half, estimate) in drift.iter_mut().enumerate() {
            let mut best = f64::NEG_INFINITY;
            for hz in [-12.0, -6.0, 0.0, 6.0, 12.0] {
                let shifted = mode.tones_at(base_hz + hz);
                let mut score = 0.0;
                for (index, (column, chunk)) in mode.columns.iter().zip(samples.chunks_exact(mode.symbol)).enumerate() {
                    if index * 2 / mode.columns.len() != half { continue; }
                    if let Column::Reference(bin) = column {
                        score += shifted[*bin].power(chunk) / (energy(chunk) * mode.symbol as f64 / 2.0).max(f64::EPSILON);
                    }
                }
                if score > best { best = score; *estimate = hz; }
            }
        }
    }
    let mut floors = vec![1.0; tones.len()];
    let mut frame_noise = 1.0;
    if active {
        let mut rows = vec![Vec::new(); tones.len()];
        let mut noises = Vec::new();
        for (column, chunk) in mode.columns.iter().zip(samples.chunks_exact(mode.symbol)) {
            if let Column::Data(bins) = column {
                let norm = (energy(chunk) * mode.symbol as f64 / 2.0).max(f64::EPSILON);
                let powers = bins.map(|b| tones[b].power(chunk) / norm);
                noises.push(median(powers.to_vec()));
                for (b, p) in bins.iter().zip(powers) { rows[*b].push(p); }
            }
        }
        for (floor, row) in floors.iter_mut().zip(rows) {
            *floor = if row.is_empty() { 1e-12 } else { median(row) };
        }
        let common = median(floors.clone()).max(1e-12);
        for floor in &mut floors { *floor = floor.max(common * 0.1).max(1e-12); }
        frame_noise = median(noises).max(1e-12);
    }
    let common = median(floors.clone());
'''
source = source.replace(needle, prepass + needle)
# Keep the prepass's tone samples fixed; track only the DATA demodulation pass.
loop = '    for (column, chunk) in mode.columns.iter().zip(samples.chunks_exact(mode.symbol)) {\n        if let Column::Data(bins) = column {'
assert source.count(loop) == 1
source = source.replace(loop, '''    for (index, (column, chunk)) in mode.columns.iter().zip(samples.chunks_exact(mode.symbol)).enumerate() {
        let shifted;
        let tones = if mode.level == Level::Two && method == "track" {
            let fraction = (index as f64 / (mode.columns.len()-1) as f64 - 0.25) * 2.0;
            shifted = mode.tones_at(base_hz + drift[0] + fraction * (drift[1]-drift[0]));
            shifted.as_slice()
        } else { tones };
        if let Column::Data(bins) = column {''')
needle = '            let powers = bins.map(|bin| tones[bin].power(chunk) / norm);'
assert source.count(needle) == 1
source = source.replace(needle, '''
            let raw = bins.map(|bin| tones[bin].power(chunk) / norm);
            let weight = if active && (method == "symbol" || method == "combined") {
                (frame_noise / median(raw.to_vec()).max(1e-12)).clamp(0.1, 10.0)
            } else { 1.0 };
            let powers: [f64; 8] = std::array::from_fn(|i| {
                let whiten = if active && (method == "tone" || method == "combined") {
                    common / floors[bins[i]]
                } else { 1.0 };
                raw[i] * weight * whiten
            });''')
path.write_text(source)
(prototype / 'main.rs').write_text('''
mod modem { pub mod coding; pub mod waveform; pub mod bw500; pub mod bw500_level4; pub mod bw500_waveform; pub mod receiver; }
fn main() {
 let raw=std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
 let samples:Vec<_>=raw.as_chunks::<4>().0.iter().map(|b|f32::from_le_bytes(*b)).collect();
 let mut receiver=modem::receiver::Receiver::new().with_pilot_acquisition();
 let report=|e:modem::receiver::Reception| for f in e.frames {
   if let Some(payload)=f.kiss_generic_payload() {
     for b in payload {print!("{b:02x}");} println!();
   } else {println!("OTHER");}
 };
 for chunk in samples.chunks(997) {receiver.push(chunk,report).unwrap();}
 receiver.finish(report);
}
''')
deps = REPO / 'rust/target/release/deps'
command = [str(Path.home()/'.cargo/bin/rustc'), '--edition=2024', '-O', str(prototype/'main.rs'), '-L', f'dependency={deps}']
for name in ('rustfft', 'thiserror', 'serde', 'serde_json'):
    libs = sorted(deps.glob(f'lib{name}-*.rlib'), key=lambda p:p.stat().st_mtime, reverse=True)
    assert libs, name
    command += ['--extern', f'{name}={libs[0]}']
command += ['-o', str(prototype/'replay')]
with (OUT/'build.log').open('w') as log:
    subprocess.run(command, stderr=log, check=True)
import os
methods = tuple(os.environ.get('SOFT_METHODS','baseline,tone,symbol,combined,track,clip').split(','))
assert set(methods) <= {'baseline','tone','symbol','combined','track','clip'} and methods[0]=='baseline'
results = []
temp = OUT/'input.f32'
def run(audio, method):
    audio.astype('<f4').tofile(temp)
    start = time.perf_counter()
    output = subprocess.check_output([str(prototype/'replay'), str(temp)], env=dict(os.environ, SOFT_METHOD=method), text=True, timeout=120)
    hashes = {hashlib.sha256(bytes.fromhex(line)).hexdigest() if line != 'OTHER' else 'OTHER' for line in output.splitlines()}
    return hashes, time.perf_counter()-start
summary = {}
for split, batchname in (('development','sweep'), ('validation','fresh-sweep-20261117')):
    manifest = json.loads((STUDY/'snr-sweep'/batchname/'manifest.json').read_text())
    for row in manifest:
        wav = STUDY/'snr-sweep'/batchname/(row['name']+'.wav')
        assert hashlib.sha256(wav.read_bytes()).hexdigest() == row['wav_sha256']
        with wave.open(str(wav)) as f:
            assert f.getframerate() == 48000 and f.getsampwidth() == 2
            pcm = np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').reshape(-1,f.getnchannels())
            if pcm.shape[1] == 2: assert np.array_equal(pcm[:,0],pcm[:,1])
            audio = pcm[:,0].astype('<f4')/32768
        for method in methods:
            hashes, elapsed = run(audio,method)
            results.append(dict(split=split,name=row['name'],source=row['source'],snr=row['nominal_snr_db'],method=method,
                                exact=int(row['expected_sha256'] in hashes),unexpected=len(hashes-{row['expected_sha256']}),seconds=elapsed))
    summary[split] = {}
    baseline = {r['name']:r['exact'] for r in results if r['split']==split and r['method']=='baseline'}
    for method in methods:
        rows = [r for r in results if r['split']==split and r['method']==method]
        noisy = [r for r in rows if r['snr'] is not None]
        summary[split][method] = dict(exact=sum(r['exact'] for r in noisy),trials=len(noisy),clean=sum(r['exact'] for r in rows if r['snr'] is None),
            gains=sum(r['exact']>baseline[r['name']] for r in noisy),losses=sum(r['exact']<baseline[r['name']] for r in noisy),
            unexpected=sum(r['unexpected'] for r in rows),seconds=sum(r['seconds'] for r in rows),
            per_snr={str(s):sum(r['exact'] for r in noisy if r['snr']==s) for s in sorted({r['snr'] for r in noisy})})
    if split=='development':
        selected = max(methods,key=lambda m:(summary[split][m]['exact'],-methods.index(m)))
        (OUT/'selection.json').write_text(json.dumps(dict(selected=selected,development=summary[split]),indent=2))
    print(split,json.dumps(summary[split]),flush=True)
controls=[]
for seed in (991101,991102):
    rng=np.random.default_rng(seed)
    n=48000*30
    bins=np.fft.rfftfreq(n,1/48000)
    spectrum=np.fft.rfft(rng.standard_normal(n));spectrum[(bins<300)|(bins>3300)]=0
    audio=np.fft.irfft(spectrum,n=n);audio*=.035/np.sqrt(np.mean(audio**2))
    if seed==991102: audio+=.2*np.sin(2*np.pi*1500*np.arange(n)/48000)
    for method in methods:
        hashes,elapsed=run(audio,method)
        controls.append(dict(seed=seed,method=method,seconds=30,frames=len(hashes),runtime=elapsed))
temp.unlink()
with (OUT/'results.csv').open('w') as f:
    writer=csv.DictWriter(f,fieldnames=results[0]);writer.writeheader();writer.writerows(results)
summary['selected']=selected
summary['controls']=controls
(OUT/'summary.json').write_text(json.dumps(summary,indent=2))
print('SELECTED',selected,flush=True)
