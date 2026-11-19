"""Reproduce unmatched gated outputs and prepare a separate stock diagnostic.

Do not play the diagnostic until the primary stock run has completed.
"""
import hashlib,json,os,subprocess,sys
from pathlib import Path
import numpy as np
from best_common import trial,write_wav
root=Path(sys.argv[1]);events=json.loads((root/'validation-unmatched.json').read_text());sources={s['name']:s for s in json.loads((root/'sources.json').read_text())};results=[]
folder=root/'counterexample';folder.mkdir(exist_ok=False)
for i,event in enumerate(events):
 row=event['trial'];source=sources[row['source']];pcm,audio,meta=trial(root,source,row['condition'],row['snr'],row['seed']);assert meta['input_sha256']==row['input_sha256']
 path=folder/f'counter-{i}.wav';write_wav(path,pcm,stereo=True);temp=folder/'input.f32';audio.tofile(temp)
 result=subprocess.run([str(root/'prototype/replay'),str(temp)],env=dict(os.environ,SOFT_METHOD='gated'),capture_output=True,text=True,check=True)
 outputs=[]
 for line in result.stdout.splitlines():
  fields=line.split('\t');body=bytes.fromhex(fields[2]);expected=bytes.fromhex(source['expected_wire'])
  outputs.append(dict(level=int(fields[0]),start=int(fields[1]),wire_sha256=hashlib.sha256(body).hexdigest(),generic_kiss=len(fields)==4,
                      body_bit_distance=sum(bin(a^b).count('1') for a,b in zip(body,expected)),body_length=len(body),
                      expected_body_length=len(expected),matches_expected=body==expected))
 results.append(dict(trial=row,outputs=outputs,wav=path.name,wav_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),expected_sha256=source['expected_kiss_sha256']))
temp=folder/'input.f32'
if temp.exists():temp.unlink()
(root/'unmatched-adjudication.json').write_text(json.dumps(results,indent=2))
study=Path.home()/'Documents/Codex/Linkwise VOACAP/2026-11-15-main-retest.sajUbP';legacy=study/'snr-sweep/fresh-sweep-20261117'
old=json.loads((legacy/'manifest.json').read_text());manifest=[]
for i in (0,1):
 row=next(r for r in old if r['name']==f'clean-{i}');os.link(legacy/(row['name']+'.wav'),folder/(row['name']+'.wav'))
 manifest.append(dict(name=row['name'],kind='transport_control',expected_sha256=row['expected_sha256'],wav_sha256=row['wav_sha256']))
for i,r in enumerate(results):manifest.append(dict(name=f'counter-{i}',kind='counterexample',expected_sha256=r['expected_sha256'],wav_sha256=r['wav_sha256']))
(folder/'manifest.json').write_text(json.dumps(manifest,indent=2));(folder/'queue.txt').write_text('\n'.join(['clean-0']+[f'counter-{i}' for i in range(len(results))]+['clean-1'])+'\n')
(folder/'protocol.txt').write_text('Diagnostic added after primary validation uncovered unmatched gated frames. No algorithm changes. Replay exact hashed counterexample with known transport controls before/after. Keep separate from the frozen primary stock subset.\n')
(study/'snr-sweep'/(root.name+'-counterexample')).symlink_to(folder,target_is_directory=True)
print(json.dumps(results,indent=2))
