"""Hash-only fresh capture characterization; no known-payload oracle.

Usage: python fresh.py SOFT_EXPERIMENT TRACKING_EXPERIMENT
Expects fresh/HOST/*.wav and capture.log from pinned KiwiClient wrapper.
"""
import hashlib,json,os,re,subprocess,sys,wave
from pathlib import Path
import numpy as np

root,tracking=map(Path,sys.argv[1:]);results=[]
for folder in sorted((root/'fresh').iterdir()):
    if not folder.is_dir():continue
    sources=[p for p in folder.glob('*.wav') if p.name!='48k.wav']
    assert len(sources)==1,sources
    original=sources[0]
    with wave.open(str(original)) as f:
        original_rate=f.getframerate();original_seconds=f.getnframes()/original_rate
    converted=folder/'48k.wav'
    subprocess.run(['/usr/bin/afconvert',str(original),str(converted),'-f','WAVE','-d','LEI16@48000','-c','1'],check=True)
    with wave.open(str(converted)) as f:
        audio=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').astype('<f4')/32768
    temp=folder/'input.f32';audio.tofile(temp)
    packets=[]
    for line in (folder/'capture.log').read_text().splitlines():
        if line.startswith('PACKET '):packets.append(json.loads(line[7:]))
    gaps=sum(((b[1]-a[1]) % 2**32)!=1 for a,b in zip(packets,packets[1:]))
    frames={}
    for method in ('baseline','tone','symbol','combined','track','clip'):
        binary=(tracking if method in ('track','clip') else root)/'prototype/replay'
        lines=subprocess.check_output([str(binary),str(temp)],env=dict(os.environ,SOFT_METHOD=method),text=True,timeout=180).splitlines()
        frames[method]=dict(frames=len(lines),payload_hashes=[hashlib.sha256(bytes.fromhex(line)).hexdigest() if line!='OTHER' else 'OTHER' for line in lines])
    temp.unlink()
    results.append(dict(receiver=folder.name,original=original.name,sha256=hashlib.sha256(original.read_bytes()).hexdigest(),original_rate=original_rate,
                        seconds=original_seconds,packets=len(packets),sequence_discontinuities=gaps,adc_overload_packets=sum(bool(p[3]&2) for p in packets),
                        first_packet_utc=packets[0][0] if packets else None,last_packet_utc=packets[-1][0] if packets else None,
                        note='CRC-valid reception only; no independent payload truth or RF error-rate denominator',methods=frames))
(root/'fresh/results.json').write_text(json.dumps(results,indent=2))
print(json.dumps(results,indent=2))
