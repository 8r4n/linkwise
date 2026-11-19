"""Stress BW500 level-4 cancellation with pilots and unencoded random DATA.

Synthesizes a waveform from the same measured transmit profile while bypassing
FEC encoding for the negative panel. With valid coded bits, the same generator
is checked against an archived stock frame through the normal replay decoder.
"""
import argparse
from hashlib import sha256
import json
from pathlib import Path
import runpy
import subprocess

import numpy as np

study = runpy.run_path(str(Path(__file__).with_name('evaluate-level4-cw.py')))
RATE = study['RATE']
HOP = 512
COLUMNS = 393
START = 2400
LENGTH = 213120


def pilot_layout():
    value = 6_437_280

    def draw(size):
        nonlocal value
        value = (value * 0x43fd43fd + 0xc39ec3) & 0xffffff
        return int(np.float32(value) * np.float32(size / 16_777_216))

    for size in (748, 368):
        seen = set()
        while len(seen) < size:
            seen.add(draw(size))
    result = np.zeros((2, COLUMNS), dtype=bool)
    for band in result:
        while np.count_nonzero(band) < 18:
            band[5 + draw(386)] = True
    result[:, 0] = True
    assert np.count_nonzero(~result) == 748
    return result


class RawCells:
    def __init__(self, profile):
        p = json.loads(profile.read_text())
        phases = dict(p['grid'])
        self.mask = pilot_layout()
        columns = np.arange(COLUMNS)[:, None]
        taps = np.arange(len(p['pulse']))[None, :] - HOP//2
        self.indices = []
        self.carriers = []
        for band, hz in enumerate((1350, 1650)):
            indices = START + 9*HOP + columns*HOP + taps + band*HOP//2
            angle = (2*np.pi/128)*np.array([phases[471*band+c] for c in range(COLUMNS)])[:,None]
            self.indices.append(indices)
            self.carriers.append(.25*np.cos(2*np.pi*hz*indices/RATE-angle)*p['pulse'])
        self.active = slice(int(np.min(self.indices)), int(np.max(self.indices))+1)

    def audio(self, bits):
        assert len(bits) == 748
        transitions = np.ones((2, COLUMNS))
        transitions[~self.mask] = 1-2*bits.astype(float)
        signs = np.cumprod(transitions,axis=1)
        out = np.zeros(LENGTH)
        for indices, carriers, polarity in zip(self.indices,self.carriers,signs):
            out += np.bincount(indices.ravel(),weights=(carriers*polarity[:,None]).ravel(),minlength=LENGTH)
        return out


def run(args):
    render = RawCells(args.profile)
    control_bits = np.fromfile(args.positive_bits,dtype=np.uint8)
    control = render.audio(control_bits)
    # The zero lead of this synthesized source means acquisition starts later
    # than on captured stock bursts. Give both paths the same coarse window.
    coarse = START + 8*HOP
    decode = study['decode']
    trial = study['paired_trial']
    quantize = study['quantize']
    impair = study['impairment']
    out = []
    with subprocess.Popen([str(args.replay),str(args.profile)], stdin=subprocess.PIPE, stdout=subprocess.PIPE) as receiver:
        _,wire,payload = decode(receiver,quantize(control),coarse)
        if wire != args.wire or payload != args.payload:
            raise ValueError('structured generator positive control did not decode to the expected bytes')
        for condition in ('clean','cw'):
            for i in range(100):
                seed = 2223000+i
                bits = np.random.default_rng(seed).integers(0,2,size=748,dtype=np.uint8)
                stimulus = render.audio(bits)
                tone = (1309,1453,1661)[i%3]
                audio = quantize(stimulus) if condition == 'clean' else impair(stimulus,render.active,0,tone,-12,seed+1000)
                result = trial(receiver,audio,coarse,None,None)
                result.pop('baseline'); result.pop('candidate')
                out.append(dict(condition=condition,seed=seed,**result))
        receiver.stdin.close()
    totals={condition:dict(windows=100, baseline_crc_valid=sum(r['baseline_wrong'] for r in out if r['condition']==condition),
                          candidate_crc_valid=sum(r['candidate_wrong'] for r in out if r['condition']==condition),
                          cancellation_attempts=sum(r['estimate_hz'] is not None for r in out if r['condition']==condition))
            for condition in ('clean','cw')}
    report=dict(scope='Real pilot schedule and differential phase relations. Random independent DATA cell signs, no FEC encoding. Not a measured RF false-frame rate.',
                positive_control='Exact host payload and wire body',totals=totals,rows=out,
                profile_sha256=sha256(args.profile.read_bytes()).hexdigest(),replay_sha256=sha256(args.replay.read_bytes()).hexdigest())
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(totals,indent=2))


if __name__ == '__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('profile',type=Path)
    p.add_argument('replay',type=Path)
    p.add_argument('positive_bits',type=Path)
    p.add_argument('wire')
    p.add_argument('payload')
    p.add_argument('output',type=Path)
    run(p.parse_args())
