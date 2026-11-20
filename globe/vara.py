"""Build Rust replays from an archived public Winlink VARA HF gateway response."""
import argparse
import json
import math
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
FEATURED = ['EA1URO','GB7LAN','DB0DFR','F5ZFX','HB9T','LA1B']


def distance(a,b):
    lon,lat=math.radians(b['lon']-a['lon']),math.radians(b['lat']-a['lat'])
    return 12742*math.asin(min(1,math.sqrt(math.sin(lat/2)**2+math.cos(math.radians(a['lat']))*math.cos(math.radians(b['lat']))*math.sin(lon/2)**2)))


def make(stations,name,binary):
    n=len(stations)
    for i,s in enumerate(stations): s['id']=i
    # The compact simulator's channel IDs index the receiver's published center frequency.
    freq=sorted({min(s['channels_hz'],key=lambda f:abs(f-7100000)) for s in stations}
                | {f for s in stations if s['name'] in FEATURED for f in s['channels_hz']})
    assert len(freq)<=256
    regional=name=='vara-europe'
    edges=[]
    if regional:
        edges=[(1,0,1),(8,1,2),(25,2,5),(4,0,3),(6,3,4),(9,4,5)]
    else:
        for i,s in enumerate(stations):
            nearby=sorted((distance(s,t),j) for j,t in enumerate(stations) if i!=j)
            # Three path lengths per site provide local, regional and distant candidate contacts.
            for k,target in enumerate((250,1100,2400)):
                j=min(nearby,key=lambda x:abs(x[0]-target))[1]
                for t in range((i+3*k)%12,192,12): edges.append((t,i,j))
    contacts=[]
    for t,a,b in sorted(set(edges)):
        hz=min(stations[b]['channels_hz'],key=lambda f:abs(f-7100000))
        contacts.append(dict(slot=t,**{'from':a},to=b,group=b,channel=freq.index(hz),primary=True,bytes=768))
    offers=[dict(source=0,destination=5,start=0,deadline=20,size=256,weight=1)] if regional else [
        dict(source=i,destination=(i+17)%n,start=i%31,deadline=192,size=128,weight=1+i%3) for i in range(n)]
    scenario=dict(nodes=n,slots=27 if regional else 192,queue_limit=32,outage_percent=0 if regional else 15,contacts=contacts,offers=offers)
    with tempfile.NamedTemporaryFile(mode='w',suffix='.json') as tmp:
        json.dump(scenario,tmp);tmp.flush()
        runs=json.loads(subprocess.check_output([binary,'replay-events',tmp.name,'17']))
    output=dict(version=1,scenario=name,stations=stations,seed=17,utc='2026-10-20T00:05:00Z' if regional else '2026-10-20T00:00:00Z',
                directory_date='2026-11-15',directory_source='https://winlink.org/rmschannels',seconds_per_slot=60 if regional else 900,
                frequency_hz=freq,slots=scenario['slots'],contacts=[[c['slot'],c['from'],c['to'],c['channel']] for c in contacts],
                offers=offers,runs=runs,blocked_contacts=[])
    (ROOT/'data'/f'{name}.json').write_text(json.dumps(output,separators=(',',':')))


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive',type=Path)
    parser.add_argument('--binary',type=Path,default=ROOT.parent/'rust/network/target/debug/linkwise-network')
    args=parser.parse_args()
    sites={}
    for g in json.loads(args.archive.read_text())['Gateways']:
        channels=sorted({c['Frequency'] for c in g['GatewayChannels'] if c['Mode']==50 and c['ServiceCode']=='PUBLIC' and 2e6<=c['Frequency']<=30e6})
        if channels and '15 Nov 2026' in g['LastStatus']:
            site=dict(name=g['Callsign'],lat=g['Latitude'],lon=g['Longitude'],channels_hz=channels,last_report=g['LastStatus'])
            assert -90<=site['lat']<=90 and -180<=site['lon']<=180 and site['name'] not in sites
            sites[site['name']]=site
    make([sites[n].copy() for n in FEATURED],'vara-europe',args.binary)
    make([sites[n] for n in sorted(sites)],'vara-world',args.binary)
