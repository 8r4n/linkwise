"""Finalize finished best-site captures and make framed validation clips.

Usage: python prepare-best.py EXPERIMENT_ROOT
Safe to repeat while other sites are still recording; unfinished sites are skipped.
"""
from datetime import datetime,timedelta
import hashlib,json,os,re,subprocess,sys,wave
from pathlib import Path
import numpy as np
root=Path(sys.argv[1]);clips=root/'clips';clips.mkdir(exist_ok=True);captures=[];sources=[]
for metadata in sorted(root.glob('*/capture.json')):
 folder=metadata.parent;row=json.loads(metadata.read_text());log=(folder/'capture.log').read_text() if (folder/'capture.log').exists() else ''
 packets=[json.loads(s[7:]) for s in log.splitlines() if s.startswith('PACKET ')]
 increments=[(b[1]-a[1])%2**32 for a,b in zip(packets,packets[1:])]
 row.update(packet_count=len(packets),sequence_gaps=sum(1<n<2**31 for n in increments) if packets else None,
            missing_packets=sum(n-1 for n in increments if 1<n<2**31) if packets else None,
            sequence_resets_or_duplicates=sum(n==0 or n>=2**31 for n in increments) if packets else None,
            adc_overload_packets=sum(bool(p[3]&2) for p in packets),recordings=[])
 for original_meta in row['files']:
  original=folder/original_meta['name'];assert hashlib.sha256(original.read_bytes()).hexdigest()==original_meta['sha256']
  with wave.open(str(original)) as f:rate=f.getframerate();duration=f.getnframes()/rate
  converted=folder/'48k.wav'
  if not converted.exists():subprocess.run(['/usr/bin/afconvert',str(original),str(converted),'-f','WAVE','-d','LEI16@48000','-c','1'],check=True)
  with wave.open(str(converted)) as f:pcm=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2');audio=pcm.astype('<f4')/32768
  temp=root/'prepare.f32';audio.tofile(temp)
  result=subprocess.run([str(root/'prototype/replay'),str(temp)],env=dict(os.environ,SOFT_METHOD='baseline'),capture_output=True,text=True,check=True,timeout=120)
  temp.unlink();frames=[]
  for line in result.stdout.splitlines():
   parts=line.split('\t');level,start,wire=parts[:3];kiss=parts[3] if len(parts)==4 else None
   frame=dict(level=int(level),start=int(start),wire=wire,wire_sha256=hashlib.sha256(bytes.fromhex(wire)).hexdigest(),kiss=kiss,
              kiss_sha256=hashlib.sha256(bytes.fromhex(kiss)).hexdigest() if kiss else None)
   if packets:frame['estimated_utc']=(datetime.fromisoformat(packets[0][0])+timedelta(seconds=int(start)/48000)).isoformat()
   frames.append(frame)
  row['recordings'].append(dict(original=original.name,original_sha256=original_meta['sha256'],sample_rate_hz=rate,seconds=duration,frames=frames))
  for i,frame in enumerate(frames):
   if frame['level']!=2 or not frame['kiss']:continue
   name=f"{folder.name}-frame-{i}";base=frame['start']-96000;size=504000
   segment=pcm[max(0,base):min(len(pcm),base+size)];before=max(0,-base);after=size-before-len(segment)
   clip=np.pad(segment,(before,after));assert len(clip)==size
   target=clips/(name+'.wav')
   with wave.open(str(target),'wb') as f:f.setnchannels(1);f.setsampwidth(2);f.setframerate(48000);f.writeframes(clip.tobytes())
   expected=[f['wire_sha256'] for f in frames if base<=f['start'] and f['start']+233472<=base+size]
   if frame['wire_sha256'] not in expected:expected.append(frame['wire_sha256'])
   sources.append(dict(name=name,site=row['site'],receiver=row['receiver'],antenna=row.get('antenna'),dial_khz=row['dial_khz'],
                       path=str(target.relative_to(root)),wav_sha256=hashlib.sha256(target.read_bytes()).hexdigest(),
                       expected_wire_sha256=frame['wire_sha256'],expected_wire=frame['wire'],expected_kiss_sha256=frame['kiss_sha256'],
                       expected_kiss=frame['kiss'],allowed_wire_sha256=expected,start_in_original=frame['start'],frame_start_in_clip=96000,
                       estimated_utc=frame.get('estimated_utc'),padding_samples=[before,after],reference='clean baseline CRC; stock qualification pending'))
 if not row['recordings'] and not(row.get('failure') or row.get('skip')):row['failure']='No WAV/audio produced; '+row.get('termination_reason','see capture log')
 captures.append(row)
(root/'captures.json').write_text(json.dumps(captures,indent=2))
(root/'sources.json').write_text(json.dumps(sources,indent=2))
print(json.dumps([dict(site=r['site'],files=len(r['recordings']),seconds=sum(w['seconds'] for w in r['recordings']),frames=sum(len(w['frames']) for w in r['recordings']),gaps=r['sequence_gaps'],failure=r.get('skip') or r.get('failure')) for r in captures],indent=2))
print('Source clips',len(sources),'unique payloads',len({r['expected_kiss_sha256'] for r in sources}))
