#!/usr/bin/env python3
"""Describe public Kiwi antenna installations without inferring causal antenna gains.

Usage: python analyze-kiwisdr-antennas.py DIRECTORY_JS NEW_OUTPUT_DIRECTORY [CACHE]
Reads public /snr histories for a fixed, deliberately diverse example set only.
"""
import concurrent.futures
import csv
from datetime import datetime,timezone,timedelta
import hashlib,json,re,statistics,sys
from pathlib import Path
import urllib.request

source,out=map(Path,sys.argv[1:3]);out.mkdir(parents=True,exist_ok=False)
cache=Path(sys.argv[3]) if len(sys.argv)>3 else None
cache_summary=json.loads((cache/'summary.json').read_text()) if cache and (cache/'summary.json').exists() else {}
cache_examples={r['url']:r for r in cache_summary.get('examples',[])}
raw=source.read_text();body=raw[raw.index('['):raw.rindex(']')+1]
receivers=json.loads(re.sub(r',\s*\]',']',body))
def family(antenna):
 a=antenna.lower()
 if re.search(r'\bor\b|switched|switching|combine|when i|same as|^switched',a):return 'Mixed / switched / ambiguous'
 if re.search(r'dipol.*wellbrook|wellbrook.*dipol|t[23]?fd.*inv.?l|efhw.*inv.?l|loop.*yagi',a):return 'Mixed / switched / ambiguous'
 if 'tci' in a:return 'Professional log-periodic'
 if 'beverage' in a:return 'Beverage'
 if re.search(r't[23]?fd|terminated.*folded',a):return 'Terminated folded dipole'
 if re.search(r'lz1aq|wellbrook|w6lvp|mla.?30|pixel|alford|active loop|magnetic loop|ala.?1530|aaa.?1',a):return 'Active / small receiving loop'
 if re.search(r'loop|skywire',a):return 'Loop (size/type varies)'
 if re.search(r'end.?fed|efhw|efrw|hyend',a):return 'End-fed wire'
 if re.search(r'dipol|doublet|inverted.?v|inv.?v|fd.?4',a):return 'Dipole / doublet / inverted-V'
 if re.search(r'vertical|whip|monopole|gp\b',a):return 'Vertical / electric-field probe'
 if re.search(r'wire|\blw\b|long.?wire',a):return 'Other wire'
 return 'Other / unspecified'

rank=[]
for r in receivers:
 try:score=int(r['snr'].split(',')[1])
 except (KeyError,ValueError,IndexError):continue
 if r.get('status')!='active' or r.get('offline')!='no' or score<=0:continue
 try:lat,lon=map(float,r['gps'].strip('()').split(','))
 except (KeyError,ValueError):continue
 rank.append(dict(id=r['id'],url=r['url'],location=r.get('loc',''),antenna=r.get('antenna',''),latitude=lat,longitude=lon,
                  family=family(r.get('antenna','')),hf_snapshot=score,updated=r.get('updated','')))
rank.sort(key=lambda r:r['hf_snapshot'],reverse=True)
with (out/'receivers.csv').open('w') as f:
 w=csv.DictWriter(f,fieldnames=rank[0]);w.writeheader();w.writerows(rank)
groups={}
for r in rank:
 key=(round(r['latitude'],3),round(r['longitude'],3),' '.join(r['antenna'].lower().split()))
 groups.setdefault(key,r)
stats={}
for category in sorted({r['family'] for r in rank}):
 rows=[r for r in groups.values() if r['family']==category];values=sorted(r['hf_snapshot'] for r in rows)
 stats[category]=dict(receiver_rows=sum(r['family']==category for r in rank),approx_installations=len(rows),median=statistics.median(values),maximum=max(values))

examples=[
 'http://g8ure.ddns.net:8079','http://g8ure.ddns.net:8073','http://sa4bna.hopto.org:8073',
 'http://bern.proxy.kiwisdr.com','http://sdr.autreradioautreculture.com:8073',
 'http://ea8bfk.ddns.net:8073','http://swl-lippi-2.stbach.de',
 'http://kiwisdr.rigiremote.ch','http://oh3aa.ddns.net:18073',
 'http://21748.proxy.kiwisdr.com','http://sdr.hfunderground.com:8073',
 'http://sdr.hfunderground.com:8076','http://sdr.hfunderground.com:8078',
 'http://kiwisdr.kfsdr.com:8073','http://kiwisdr.kfsdr.com:8074','http://kiwisdr.kfsdr.com:8075','http://kiwisdr.kfsdr.com:8076',
 'http://kiwisdr1.sdrutah.org:8073','http://ja5fp2.proxy.kiwisdr.com','http://190.123.83.42:8073',
 'http://sdr-amradioantennas.com:8071','http://rx2.wa2zkd.net:8073','http://ve3nsv.ddns.net:8074',
 'http://kiwi.web-sdr.net','http://hl5ntr.ddns.net:8074','http://zl2ba.proxy.kiwisdr.com',
 'http://22337.proxy.kiwisdr.com','http://191.37.241.167:8073','http://alg.twrmon.net:8073',
]
bands={'hf':(1800,30000),'40m':(7000,7200),'20m':(14000,14350),'80m':(3500,3800)}
def fetch(url):
 r=next((r for r in rank if r['url'].rstrip('/')==url),None)
 result=dict(r or {'url':url})
 safe=re.sub(r'[^A-Za-z0-9.-]','_',url)
 for endpoint in ('status','snr'):
  try:
   cached=cache/(safe+'-'+endpoint+'.txt') if cache else None
   if cached and cached.exists():
    content=cached.read_bytes()
    timestamp=cache_examples.get(url,{}).get(endpoint+'_retrieved_utc')
    retrieved=datetime.fromisoformat(timestamp) if timestamp else datetime.fromtimestamp(cached.stat().st_mtime,timezone.utc)
   else:
    req=urllib.request.Request(url+'/'+endpoint,headers={'User-Agent':'Linkwise-Kiwi-antenna-study/1.0'})
    with urllib.request.urlopen(req,timeout=8) as response:content=response.read(2_000_000)
    retrieved=datetime.now(timezone.utc)
   result[endpoint+'_retrieved_utc']=retrieved.isoformat()
   (out/(safe+'-'+endpoint+'.txt')).write_bytes(content)
   if endpoint=='status':
    status=dict(line.split('=',1) for line in content.decode(errors='replace').splitlines() if '=' in line)
    result['live_status']={k:status.get(k) for k in ('status','offline','antenna','snr','date','uptime','adc_ov')}
   else:
    history=json.loads(content)
    valid=[]
    for sample in history:
     try:date=datetime.strptime(sample['ts'],'%a %b %d %H:%M:%S %Y')
     except (ValueError,KeyError):continue
     valid.append((date,sample))
    if not valid:raise ValueError('No timestamped samples')
    latest=max(t for t,s in valid)
    window=sorted((t,s) for t,s in valid if latest-t<timedelta(hours=24))
    summary={}
    for label,(lo,hi) in bands.items():
     selected=[b for t,s in window for b in s['snr'] if b['lo']==lo and (b['hi']==hi or label=='hf' and b['hi']==32000)]
     values=[b['snr'] for b in selected]
     if values:summary[label]=dict(n=len(values),median=statistics.median(values),min=min(values),max=max(values),ranges_khz=sorted({(b['lo'],b['hi']) for b in selected}))
    result['history']=dict(total_samples=len(history),window_samples=len(window),span_hours=(latest-window[0][0]).total_seconds()/3600,
                           first=str(window[0][0]),last=str(latest),utc_flags=sorted({s.get('utc') for t,s in window}),
                           antenna_selectors=sorted({s.get('ant') for t,s in window}),bands=summary)
  except Exception as e:result[endpoint+'_error']=str(e)
 print(url,'OK' if 'history' in result else result.get('snr_error'),flush=True)
 return result
with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:histories=list(pool.map(fetch,examples))
summary=dict(fetched_utc=datetime.now(timezone.utc).isoformat(),directory_header=raw[:raw.index('var kiwisdr_com')],directory_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
             directory_rows=len(receivers),scored_rows=len(rank),approx_installations=len(groups),family_snapshot=stats,top20=rank[:20],examples=histories,
             limitations=['Snapshot and histories are receiver-system observations, not antenna gain measurements.',
                          'Antenna labels are operator descriptions; switched/mixed installations cannot identify the active antenna.',
                          'Histories use each receiver latest24h; utc=0 is local unspecified time, not simultaneous UTC alignment.',
                          'Sparse activity, propagation, latitude, local noise, filtering, calibration and overload confound ranking.',
                          'Approximate installation grouping uses rounded coordinates and normalized antenna label; it is not proven independence.',
                          'No audio streams, modem trials or controlled A/B antenna swaps were performed.'])
(out/'summary.json').write_text(json.dumps(summary,indent=2))
print(json.dumps(stats,indent=2))
