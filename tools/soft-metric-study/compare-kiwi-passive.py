"""Compare CRC and host payload outcomes on unmodified passive KiwiSDR audio.

Run against a collection directory created by collect-best.py with packet
telemetry. Store hashes and sample positions without publishing decoded text.
"""
import argparse
from collections import Counter
from datetime import datetime, timedelta
import hashlib
from itertools import combinations
import json
import os
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
import time
import wave

import numpy as np

METHODS = ('baseline', 'stable')
SAMPLE_RATE = 48_000


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def continuity(log):
    packets = [json.loads(line[7:]) for line in log.splitlines() if line.startswith('PACKET ')]
    if not packets:
        return dict(packets=0, missing=None, duplicates=None, reverse_or_reset=None,
                    overload=None, first_packet_utc=None)
    increments = [(b[1] - a[1]) % 2**32 for a, b in zip(packets, packets[1:])]
    return dict(packets=len(packets), missing=sum(n - 1 for n in increments if 1 < n < 2**31),
                duplicates=increments.count(0), reverse_or_reset=sum(n >= 2**31 for n in increments),
                overload=sum(bool(packet[3] & 2) for packet in packets), first_packet_utc=packets[0][0])


def decode(replay, f32, method, first_packet):
    started = time.perf_counter()
    result = subprocess.run([str(replay), '-'], input=f32, capture_output=True,
                            env=dict(os.environ, SOFT_METHOD=method), check=True, timeout=120)
    elapsed = time.perf_counter() - started
    counters = [line.split(',') for line in result.stderr.decode().splitlines() if line.startswith('GATES,')]
    if len(counters) != 1:
        raise ValueError('receiver did not report gate telemetry')
    attempts, active = map(int, counters[0][1:])
    frames = []
    for row in result.stdout.decode().splitlines():
        level, sample, wire, *kiss = row.split('\t')
        sample = int(sample)
        when = first_packet + timedelta(seconds=sample/SAMPLE_RATE) if first_packet else None
        frames.append(dict(level=int(level), sample=sample, seconds=sample/SAMPLE_RATE,
                           wire_sha256=sha256(bytes.fromhex(wire)),
                           kiss_sha256=sha256(bytes.fromhex(kiss[0])) if kiss else None,
                           estimated_utc=when.isoformat() if when else None))
    return dict(frames=frames, gate_attempts=attempts, gate_activations=active, replay_wall_seconds=elapsed)


def compare(reference, candidate):
    # Allow small timing shifts without conflating repeated identical frames.
    unmatched = list(candidate)
    lost = []
    for frame in reference:
        match = next((item for item in unmatched if item['level'] == frame['level']
                      and item['wire_sha256'] == frame['wire_sha256']
                      and item['kiss_sha256'] == frame['kiss_sha256']
                      and abs(item['sample'] - frame['sample']) <= SAMPLE_RATE//4), None)
        if match is None:
            lost.append(frame)
        else:
            unmatched.remove(match)
    return dict(retained=len(reference) - len(lost), lost=lost, additional=unmatched)


def cross_receiver_matches(captures):
    sightings = {}
    for capture in captures:
        for recording in capture['recordings']:
            if not recording['usable']:
                continue
            for frame in recording['methods']['baseline']['frames']:
                if frame['estimated_utc']:
                    key = (capture['dial_khz'], frame['wire_sha256'], frame['kiss_sha256'])
                    sightings.setdefault(key, {}).setdefault(capture['site'], []).append(
                        datetime.fromisoformat(frame['estimated_utc'])
                    )
    matches = []
    for (dial, wire, kiss), by_site in sightings.items():
        for (site, left), (other_site, right) in combinations(sorted(by_site.items()), 2):
            offsets = sorted((abs((a-b).total_seconds()), i, j)
                             for i, a in enumerate(left) for j, b in enumerate(right))
            used_left, used_right = set(), set()
            for offset, i, j in offsets:
                if offset > 2.5:
                    break
                if i in used_left or j in used_right:
                    continue
                used_left.add(i)
                used_right.add(j)
                matches.append(dict(sites=[site, other_site], dial_khz=dial, wire_sha256=wire,
                                    kiss_sha256=kiss, arrival_estimates_utc=[left[i].isoformat(), right[j].isoformat()],
                                    arrival_estimate_difference_s=offset))
    return matches


def analyze(root, replay, output):
    captures = []
    with TemporaryDirectory(prefix='linkwise-passive-', dir=root) as tmp:
        temp = Path(tmp)
        for metadata in sorted(root.glob('*/capture.json')):
            capture = json.loads(metadata.read_text())
            folder = metadata.parent
            capture['capture_id'] = folder.name
            log = folder/'capture.log'
            capture['continuity'] = continuity(log.read_text() if log.exists() else '')
            capture['recordings'] = []
            for asset in capture['files']:
                original = folder/asset['name']
                if sha256(original.read_bytes()) != asset['sha256']:
                    raise ValueError(f'original hash changed: {original}')
                with wave.open(str(original)) as wav:
                    rate = wav.getframerate()
                    duration = wav.getnframes()/rate
                wav48 = temp/'derived.wav'
                subprocess.run(['/usr/bin/afconvert', str(original), str(wav48),
                                '-f', 'WAVE', '-d', 'LEI16@48000', '-c', '1'], check=True)
                derived_sha = sha256(wav48.read_bytes())
                with wave.open(str(wav48)) as wav:
                    if (wav.getframerate(), wav.getnchannels(), wav.getsampwidth()) != (SAMPLE_RATE, 1, 2):
                        raise ValueError('expected 48 kHz mono PCM16 derivative')
                    pcm = np.frombuffer(wav.readframes(wav.getnframes()), dtype='<i2')
                raw = (pcm.astype('<f4')/32768).tobytes()
                status = capture['continuity']
                first = datetime.fromisoformat(status['first_packet_utc']) if len(capture['files']) == 1 and status['first_packet_utc'] else None
                methods = {method: decode(replay, raw, method, first) for method in METHODS}
                usable = (len(capture['files']) == 1 and status['packets'] > 1 and capture.get('returncode') == 0
                          and all(status[k] == 0 for k in ('missing','duplicates','reverse_or_reset','overload')))
                capture['recordings'].append(dict(original=asset['name'], source_rate_hz=rate,
                                                  seconds=duration, complete=abs(duration-capture['requested_seconds']) <= 2,
                                                  usable=usable, derivative_sha256=derived_sha, methods=methods,
                                                  paired=compare(methods['baseline']['frames'],methods['stable']['frames'])))
                wav48.unlink()
            captures.append(capture)
    totals = Counter()
    for capture in captures:
        totals['receiver_attempts'] += 1
        for recording in capture['recordings']:
            totals['recordings'] += 1
            totals['recorded_seconds'] += recording['seconds']
            totals['complete_recordings'] += int(recording['complete'])
            if not recording['usable']:
                continue
            totals['usable_recordings'] += 1
            totals['usable_seconds'] += recording['seconds']
            for method in METHODS:
                member = recording['methods'][method]
                totals[f'{method}_frames'] += len(member['frames'])
                totals[f'{method}_kiss_frames'] += sum(bool(row['kiss_sha256']) for row in member['frames'])
                totals[f'{method}_gate_attempts'] += member['gate_attempts']
                totals[f'{method}_gate_activations'] += member['gate_activations']
                totals[f'{method}_replay_wall_seconds'] += member['replay_wall_seconds']
            totals['retained'] += recording['paired']['retained']
            totals['lost'] += len(recording['paired']['lost'])
            totals['additional'] += len(recording['paired']['additional'])
    scope = ('Receive-only KiwiSDR observations with sample-rate conversion and no added impairments. '
             'Transmitter frame counts and exact payload oracle are unknown. CRC-valid candidate-only output requires independent corroboration. '
             'Cross-site counts are paired receiver observations. Timing uses internet packet arrival estimates.')
    report = dict(scope=scope, replay_sha256=sha256(replay.read_bytes()), converter='macOS afconvert LEI16@48000 mono',
                  totals=totals, cross_receiver_matches=cross_receiver_matches(captures), captures=captures)
    output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(totals,indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root',type=Path)
    parser.add_argument('replay',type=Path)
    parser.add_argument('output',type=Path)
    args = parser.parse_args()
    analyze(args.root,args.replay,args.output)
