"""Capture UTC-tagged NOAA grids for the bundled replay. Replays use the saved files."""
import concurrent.futures
import hashlib
import datetime as dt
import gzip
import json
import math
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent
BASE = 'https://services.swpc.noaa.gov'
EPOCH = dt.datetime(2026,10,20,tzinfo=dt.timezone.utc)

def capture(stamp):
    url = f'{BASE}/products/glotec/geojson_2d_urt/glotec_icao_{stamp}.geojson'
    data = subprocess.check_output(['curl','--retry','2','-fsSL',url])
    points = json.loads(data)['features']
    xs, ys = [sorted({p['geometry']['coordinates'][i] for p in points}) for i in (0,1)]
    cells = [None] * (len(xs)*len(ys))
    for feature in points:
        lon, lat = feature['geometry']['coordinates']
        p = feature['properties']
        vals = [p.get(k) for k in ('tec','hmF2','NmF2','quality_flag')]
        if all(isinstance(v,(float,int)) and math.isfinite(v) and v>=0 for v in vals):
            cells[ys.index(lat)*len(xs)+xs.index(lon)] = [round(vals[0],1),round(vals[1]),round(vals[2]/1e9,2),int(vals[3])]
    assert len(xs)==72 and len(ys)==72 and xs[0]==-177.5 and ys[0]==-88.75
    dest=ROOT/'data/grids'/f'{stamp}.json.gz'
    dest.write_bytes(gzip.compress(json.dumps(cells,separators=(',',':')).encode(),mtime=0))
    utc=dt.datetime.strptime(stamp,'%Y%m%dT%H%M%SZ').strftime('%Y-%m-%dT%H:%M:%SZ')
    return dict(utc=utc,source=url,sha256=hashlib.sha256(data).hexdigest(),file='data/grids/'+dest.name)


if __name__ == '__main__':
    (ROOT/'data/grids').mkdir(exist_ok=True)
    # Freeze the latest published interval at or before every replay tick.
    # A sentinel gap at 00:00 is outside this archive, which starts at 00:05.
    wanted=set(range(5,36,10))
    for minute in range(15,192*15+1,15):
        wanted.add(5+10*((minute-5)//10))
    stamps=[(EPOCH+dt.timedelta(minutes=m)).strftime('%Y%m%dT%H%M%SZ') for m in sorted(wanted)]
    with concurrent.futures.ThreadPoolExecutor(6) as pool:
        frames=list(pool.map(capture,stamps))
    (ROOT/'data/weather-index.json').write_text(json.dumps(frames,separators=(',',':')))
