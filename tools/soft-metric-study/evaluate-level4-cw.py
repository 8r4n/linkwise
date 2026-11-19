"""Framed level-4 stock replay with a blind, single-tone cancellation fallback.

See the run manifest for exact fixture oracles. Development and later archived
sessions have separate reports. The coarse burst window is supplied to both
methods. No impairment frequency is passed to the cancellation estimator.
"""
import argparse
import csv
from collections import Counter
import hashlib
import json
from pathlib import Path
import subprocess
import time

import numpy as np

RATE = 48_000
SEARCH = (1200, 1800)
MIN_PEAK_TO_LOCAL = 25.0
MAX_HALF_DRIFT_HZ = 0.8
MIN_LINE_FRACTION = 0.08
MAX_PHASE_ERROR = 0.6


def digest(data):
    return hashlib.sha256(data).hexdigest()


def infer_peak(samples, rate):
    """Hann peak with quadratic interpolation and a local-floor test."""
    bins = np.fft.rfftfreq(len(samples), 1 / rate)
    spectrum = abs(np.fft.rfft(samples * np.hanning(len(samples)))) ** 2
    selected = np.flatnonzero((bins >= SEARCH[0]) & (bins <= SEARCH[1]))
    i = selected[np.argmax(spectrum[selected])]
    floor = np.median(spectrum[max(1, i-48):i+49])
    adjacent = np.log(np.maximum(spectrum[i-1:i+2], np.finfo(float).tiny))
    curvature = adjacent[0] - 2 * adjacent[1] + adjacent[2]
    shift = (adjacent[0] - adjacent[2]) / (2 * curvature) if curvature != 0 else 0.0
    return (i + np.clip(shift, -.5, .5)) * rate / len(samples), spectrum[i] / max(floor, 1e-15)


def cancel_tone(audio, coarse_start):
    """Fit a coherent sinusoid after separate frequency checks in both halves."""
    samples = audio[coarse_start:]
    if len(samples) < 2 * RATE:
        return None, None
    halves = np.array_split(samples, 2)
    peaks = [infer_peak(half, RATE) for half in halves]
    if abs(peaks[0][0]-peaks[1][0]) > MAX_HALF_DRIFT_HZ or any(ratio < MIN_PEAK_TO_LOCAL for _, ratio in peaks):
        return None, None
    frequency, _ = infer_peak(samples, RATE)
    indices = np.arange(len(audio)) - coarse_start
    phase = np.exp(2j*np.pi*frequency*indices/RATE)
    fit = 2 * np.mean(samples * phase[coarse_start:].conj())
    split = coarse_start + len(halves[0])
    a = np.mean(audio[coarse_start:split] * phase[coarse_start:split].conj())
    b = np.mean(audio[split:] * phase[split:].conj())
    fraction = abs(fit)**2 / (2 * max(np.mean(samples**2), 1e-15))
    if fraction < MIN_LINE_FRACTION or abs(np.angle(a*b.conjugate())) > MAX_PHASE_ERROR:
        return None, None
    return audio - (fit * phase).real, frequency


def quantize(audio):
    gain = min(1.0, .9 / max(np.max(abs(audio)), 1e-15))
    return np.rint(audio*gain*32768).astype('<i2').astype(np.float32)/32768


def band_noise(n, rng):
    bins = np.fft.rfftfreq(n, 1/RATE)
    spectrum = np.fft.rfft(rng.standard_normal(n))
    spectrum[(bins < 300) | (bins > 3300)] = 0
    return np.fft.irfft(spectrum,n)


def impairment(audio, active, snr, tone_hz, sir, seed):
    rng = np.random.default_rng(seed)
    rms = np.sqrt(np.mean(audio[active]**2))
    noise = band_noise(len(audio), rng)
    noise *= rms*10**(-snr/20) / np.sqrt(np.mean(noise[active]**2))
    if tone_hz:
        tone = np.sin(2*np.pi*tone_hz*np.arange(len(audio))/RATE+rng.uniform(0, 2*np.pi))
        tone *= rms*10**(-sir/20) / np.sqrt(np.mean(tone[active]**2))
    else:
        tone = 0
    return quantize(audio + noise + tone)


def decode(process, audio, start):
    pcm = np.asarray(audio, dtype='<f4')
    process.stdin.write(f'{len(pcm)} {start} KN6MQS\n'.encode()+pcm.tobytes())
    process.stdin.flush()
    row = process.stdout.readline().decode().strip()
    if not row:
        raise RuntimeError('replay process stopped')
    if row == 'MISS':
        return None, None, None
    lo, hi, body, payload = row.split('\t')
    return slice(int(lo),int(hi)), body, payload


def paired_trial(process, audio, coarse, expected, wire):
    original = decode(process,audio,coarse)
    filtered, estimate = cancel_tone(audio,coarse) if original[1] is None else (None,None)
    t = time.perf_counter()
    candidate = decode(process,filtered,coarse) if filtered is not None else original
    elapsed = time.perf_counter() - t if filtered is not None else 0
    return dict(baseline=original[1:] == (wire,expected), candidate=candidate[1:] == (wire,expected),
                baseline_wrong=original[1] is not None and original[1:] != (wire,expected),
                candidate_wrong=candidate[1] is not None and candidate[1:] != (wire,expected),
                estimate_hz=estimate,candidate_seconds=elapsed)


def run(args):
    manifest = json.loads(args.manifest.read_text())
    fixtures = [r for r in manifest['sources'] if r['split'] == args.split]
    rows = []
    with subprocess.Popen([str(args.replay),str(args.profile)],stdin=subprocess.PIPE, stdout=subprocess.PIPE) as process:
        for source in fixtures:
            path = Path(source['file'])
            data = path.read_bytes()
            assert digest(data) == source['source_sha256']
            audio = np.frombuffer(data,dtype='<f4').astype(float)
            coarse = source['coarse_start']
            active,wire,payload = decode(process,quantize(audio),coarse)
            assert active is not None and payload == source['host_payload_hex']
            conditions = [('clean',None,None,None)] + [('awgn',s,None,None) for s in (0,6)]
            conditions += [('cw',0,f,sir) for f in (1309,1453,1661) for sir in (-12,-6,0)]
            for condition,snr,f,sir in conditions:
                n = 1 if condition == 'clean' else args.replicates
                for replicate in range(n):
                    seed = source['seed']+replicate
                    supplied = quantize(audio) if condition == 'clean' else impairment(audio,active,snr,f,sir,seed)
                    result = paired_trial(process,supplied,coarse,payload,wire)
                    rows.append(dict(source=source['id'],condition=condition,snr_db=snr,tone_hz=f,sir_db=sir,seed=seed,**result))
        if args.split == 'later':
            for index in range(100):
                rng=np.random.default_rng(2220900+index)
                n=213120
                noise=band_noise(n,rng)
                cw=np.sqrt(2)*np.sin(2*np.pi*(1309,1453,1661)[index%3]*np.arange(n)/RATE+rng.uniform(0,2*np.pi))
                negative=quantize(.05*(noise / np.std(noise) + cw))
                result=paired_trial(process,negative,2400,None,None)
                result.update(baseline=False,candidate=False)
                rows.append(dict(source='negative',condition='noise_cw',snr_db=None,tone_hz=(1309,1453,1661)[index%3],sir_db=None,seed=2220900+index,**result))
        process.stdin.close()
    with args.output.with_suffix('.csv').open('w',newline='') as f:
        writer=csv.DictWriter(f,fieldnames=rows[0]); writer.writeheader(); writer.writerows(rows)
    cells={}
    for row in rows:
        key = f"{row['condition']}:{row['snr_db']}:{row['tone_hz']}:{row['sir_db']}"
        counts=cells.setdefault(key,Counter())
        counts['trials']+=1
        for k in ('baseline','candidate','baseline_wrong','candidate_wrong'):
            counts[k]+=int(row[k])
        counts['activation']+=int(row['estimate_hz'] is not None)
    report=dict(split=args.split,source_count=len(fixtures),cells=cells,
                scope='Framed archived stock level-4 input with injected 300–3300 Hz noise/CW. Includes unstructured noise+CW negative windows only. In-session qualification pending.',
                manifest_sha256=digest(args.manifest.read_bytes()),profile_sha256=digest(args.profile.read_bytes()),replay_sha256=digest(args.replay.read_bytes()))
    args.output.with_suffix('.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__ == '__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('manifest',type=Path)
    p.add_argument('replay',type=Path)
    p.add_argument('profile',type=Path)
    p.add_argument('output',type=Path)
    p.add_argument('--split',choices=('development','later'),required=True)
    p.add_argument('--replicates',type=int,default=6)
    run(p.parse_args())
