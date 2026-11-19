"""Prepare independent stock verification of a natural unconditional-metric gain."""
import hashlib,json,os,subprocess,sys,wave
from pathlib import Path
import numpy as np
from best_common import write_wav
root=Path(sys.argv[1]);folder=root/'natural-diagnostic';folder.mkdir(exist_ok=False);manifest=[]
for row in json.loads((root/'continuous-validation.json').read_text()):
 for digest in row['methods']['tone']['gains']:
  with wave.open(str(root/row['capture']/'48k.wav')) as f:pcm=np.frombuffer(f.readframes(f.getnframes()),dtype='<i2')
  temp=folder/'full.f32';(pcm.astype('<f4')/32768).tofile(temp)
  result=subprocess.run([str(root/'prototype/replay'),str(temp)],env=dict(os.environ,SOFT_METHOD='tone'),capture_output=True,text=True,check=True);temp.unlink()
  fields=next(s.split('\t') for s in result.stdout.splitlines() if hashlib.sha256(bytes.fromhex(s.split('\t')[2])).hexdigest()==digest)
  epoch=int(fields[1]);start=epoch-96000;size=504000
  if len(fields)!=4 or start<0 or start+size>len(pcm):continue
  target=folder/'natural-0.wav';write_wav(target,pcm[start:start+size],stereo=True)
  manifest.append(dict(name='natural-0',kind='natural_candidate',expected_sha256=hashlib.sha256(bytes.fromhex(fields[3])).hexdigest(),wav_sha256=hashlib.sha256(target.read_bytes()).hexdigest(),
                       source_capture=row['capture'],sample=epoch,wire_sha256=digest))
study=Path.home()/'Documents/Codex/Linkwise VOACAP/2026-11-15-main-retest.sajUbP';legacy=study/'snr-sweep/fresh-sweep-20261117'
old=json.loads((legacy/'manifest.json').read_text())
for i in (0,1):
 row=next(r for r in old if r['name']==f'clean-{i}');os.link(legacy/(row['name']+'.wav'),folder/(row['name']+'.wav'))
 manifest.append(dict(name=row['name'],kind='transport_control',expected_sha256=row['expected_sha256'],wav_sha256=row['wav_sha256']))
assert any(r['kind']=='natural_candidate' for r in manifest)
(folder/'manifest.json').write_text(json.dumps(manifest,indent=2));(folder/'queue.txt').write_text('clean-0\nnatural-0\nclean-1\n')
(folder/'protocol.txt').write_text('Post-hoc diagnostic: confirm a CRC-valid generic payload found only by the unconditional-whitening continuous pass. No algorithm change. Crop original recording around that frame, compare stock and all Linkwise modes with transport controls. A cropped recovery does not prove identical continuous acquisition performance.\n')
(study/'snr-sweep'/(root.name+'-natural')).symlink_to(folder,target_is_directory=True)
print('Natural diagnostic prepared')
