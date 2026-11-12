#!/usr/bin/env python3
"""Prepare the 30-burst native KiwiSDR pilot or verify its local WAV oracle.

File/loopback operations only. Never controls radios, PTT, or public receivers.
"""
import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import subprocess
import urllib.request
import urllib.parse
import wave

ROOT = Path(__file__).resolve().parents[1]
PRBS9_SEED = 0x1FF
BASE32 = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"


def prepare(args):
    target = urllib.parse.urlsplit(args.url)
    if target.scheme != "http" or target.hostname not in ("127.0.0.1", "localhost", "::1"):
        raise ValueError("the preparation service must be a local HTTP demo")
    destination = args.output
    if destination.exists() and any(destination.iterdir()):
        raise ValueError("output must be a new or empty directory; refusing to overwrite evidence")
    destination.mkdir(parents=True, exist_ok=True)
    spec = importlib.util.spec_from_file_location("audio_export", ROOT / "tools/export-demo-audio.py")
    export = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(export)
    entries, start = [], 30.0
    state = PRBS9_SEED
    for repetition in range(10):
        for level in (1, 2, 3):
            sequence = len(entries) + 1
            # A fixed-width ID and six ASCII symbols fit the 9-byte level-1
            # payload. Carry the PRBS state forward across the ordered block.
            phase_seed = state
            symbols = []
            for _ in range(6):
                index = 0
                for _ in range(5):
                    index = (index << 1) | (state & 1)
                    feedback = ((state >> 7) ^ (state >> 6) ^ (state >> 4) ^ state) & 1
                    state = (state >> 1) | (feedback << 8)
                symbols.append(BASE32[index])
            message = f"K{sequence:02d}" + "".join(symbols)
            request = dict(source="native", level=level, message=message,
                           snr_db=None, dropout=False, seed=20261109)
            req = urllib.request.Request(args.url.rstrip('/') + '/api/run',
                                         json.dumps(request).encode(), {'Content-Type': 'application/json'})
            with urllib.request.urlopen(req, timeout=30) as response:
                report = json.load(response)
            if not report['payload_verified'] or report['decoded_text'] != message or len(report['frames']) != 1:
                raise ValueError(f"source verification failed at burst {sequence}")
            with urllib.request.urlopen(args.url.rstrip('/') + f"/api/runs/{report['id']}/audio.wav", timeout=30) as response:
                audio = export.pcm16(response.read())
            filename = f"burst-{sequence:03d}-level{level}.wav"
            (destination / filename).write_bytes(audio)
            with wave.open(str(destination / filename)) as wav:
                duration = wav.getnframes() / wav.getframerate()
            if not 0 < duration < 20:
                raise ValueError("burst duration outside plan limit")
            entries.append(dict(burst_id=f"B{sequence:03d}", file=filename,
                                prbs9_phase_seed=f"0x{phase_seed:03x}",
                                request=request, frames=report['frames'],
                                duration_s=duration, start_offset_s=start,
                                end_offset_s=start+duration,
                                sha256=hashlib.sha256(audio).hexdigest()))
            start += duration + (2.0 if sequence < 30 else 0.0)
    schedule = dict(status='planned; no RF performed', order='L1,L2,L3 repeated ten times',
                    bursts=30, pre_roll_s=30, minimum_interburst_guard_s=2,
                    post_roll_s=30, last_burst_end_s=start,
                    waveform_seed=20261109,
                    payload_pattern=dict(polynomial='x^9 + x^7 + x^6 + x^4 + 1',
                                         initial_seed='0x1ff', taps='0x0d1',
                                         encoding='K + two-digit ID + six Base32 symbols; 5 bits per symbol, MSB-first',
                                         order='continuous right-shifting register, emitting bit 0'),
                    capture_seconds=math.ceil(start+30),
                    timing_note='Offsets are relative to capture start. Actual RF timing must be independently logged; source guards are additional to interburst quiet time.')
    (destination / 'manifest.json').write_text(json.dumps(entries, indent=2)+'\n')
    (destination / 'schedule.json').write_text(json.dumps(schedule, indent=2)+'\n')
    print(json.dumps(schedule))


def verify(args):
    entries = json.loads(args.manifest.read_text())
    if not isinstance(entries, list) or not entries:
        raise ValueError('expected a nonempty waveform manifest list')
    records = []
    for entry in entries:
        result = dict(file=entry['file'], passed=False)
        try:
            name = Path(entry['file'])
            if name.is_absolute() or len(name.parts) != 1:
                raise ValueError('manifest filenames must be local basenames')
            path = args.manifest.parent / name
            if hashlib.sha256(path.read_bytes()).hexdigest() != entry['sha256']:
                raise ValueError('WAV SHA-256 mismatch')
            with wave.open(str(path)) as wav:
                if (wav.getnchannels(), wav.getframerate(), wav.getsampwidth()) != (1, 48000, 2):
                    raise ValueError('expected mono 48 kHz PCM16')
            proc = subprocess.run([str(args.decoder.resolve()), 'decode-wav', str(path.resolve())],
                                  capture_output=True, text=True, timeout=30, check=True)
            decoded = json.loads(proc.stdout)
            keys = ('level', 'control', 'payload_hex')
            if not entry['frames']:
                raise ValueError('missing expected-frame oracle')
            expected = [tuple(frame[key] for key in keys) for frame in entry['frames']]
            actual = [tuple(frame[key] for key in keys) for frame in decoded['bw500_data_frames']]
            if actual != expected:
                raise ValueError('decoded frame count/order/level/control/payload differs from oracle')
            result.update(passed=True, decoded_frames=len(actual), decoder_report=decoded)
        except (ValueError, KeyError, OSError, subprocess.SubprocessError, wave.Error) as error:
            result['error'] = str(error)
        records.append(result)
    report = dict(passed=all(r['passed'] for r in records), files=len(records),
                  verified_frames=sum(r.get('decoded_frames', 0) for r in records), results=records)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ('passed','files','verified_frames')}))
    return 0 if report['passed'] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('prepare');p.add_argument('--url', default='http://127.0.0.1:8789');p.add_argument('--output', type=Path, required=True)
    p = sub.add_parser('verify');p.add_argument('--manifest', type=Path, required=True);p.add_argument('--decoder', type=Path, required=True);p.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        return prepare(args) if args.command == 'prepare' else verify(args)
    except (ValueError, OSError, KeyError) as error:
        parser.exit(2, f'{error}\n')


if __name__ == '__main__':
    raise SystemExit(main())
