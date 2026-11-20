"""Generate hourly October circuit forecasts using a local VOACAPL Docker image."""
import json
import re
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
IMAGE = 'linkwise-voacap:local'
KEYS = ['REL', 'SNR', 'S DBW', 'MODE', 'V HITE']


def coord(value, lat):
    return f'{abs(value):5.2f}'+('NS' if lat else 'EW')[value < 0]


def generate():
    labels = []
    cards = ['LINEMAX      55','COEFFS    CCIR','TIME          1   24    1    1',
             'MONTH      202610.00','SUNSPOT     83.',
             'ANTENNA       1    1    2   30     0.000[default/isotrope     ]  0.0    0.1000',
             'ANTENNA       2    2    2   30     0.000[default/isotrope     ]  0.0    0.0000']
    for family in ('six','global-100'):
        scene=json.loads((ROOT/'data'/f'{family}.json').read_text())
        for a,b in sorted({(a,b) for _,a,b,_ in scene['contacts'] if a<6 and b<6}):
            x,y=[scene['stations'][i] for i in (a,b)]
            key=f'{"six" if family=="six" else "global"}/{a}/{b}'
            cards+= [f'LABEL     P{len(labels):05}              LINKWISE',
                     f'CIRCUIT   {coord(x["lat"],True)}{coord(x["lon"],False):>10}{coord(y["lat"],True):>10}{coord(y["lon"],False):>10}  S 0',
                     'SYSTEM       1. 145. 3.00  90. 39.0 3.00 0.10','FPROB      1.00 1.00 1.00 0.00',
                     'FREQUENCY '+''.join(f'{f/1e6:5.2f}' for f in scene['frequency_hz']+[0]*7),
                     'METHOD       30    0','EXECUTE']
            labels.append(key)
    forecast={}
    with tempfile.TemporaryDirectory() as temp:
        run=Path(temp)
        (run/'paths.dat').write_text('\n'.join(cards+['QUIT','']))
        subprocess.run(['docker','run','--rm','--network','none','-v',f'{run}:/root/itshfbc/run',IMAGE,
                        'voacapl','/root/itshfbc','paths.dat','paths.out'],check=True,stdout=subprocess.DEVNULL)
        for line in (run/'paths.out').read_text().splitlines():
            label=re.match(r'  P(\d{5})\s+LINKWISE\s+AZIMUTHS',line)
            if label: path=labels[int(label[1])]
            if line.rstrip().endswith(' FREQ'):hour=int(float(line[:6]))%24
            for metric in KEYS:
                if line.rstrip().endswith(' '+metric):
                    for channel in range(4):
                        value=line[11+channel*5:16+channel*5].strip()
                        entry=forecast.setdefault(f'{path}/{hour}/{channel}',{})
                        entry[metric]=value if metric=='MODE' else None if value=='-' else float(value)
    assert len(forecast)==len(labels)*24*4 and all(set(e)==set(KEYS) for e in forecast.values())
    (ROOT/'data/forecast.json').write_text(json.dumps(forecast,separators=(',',':')))


if __name__=='__main__':
    generate()
