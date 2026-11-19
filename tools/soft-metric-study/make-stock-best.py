"""Make a bounded stock-comparison subset using exactly the trial generator inputs."""
import hashlib,json,os,sys
from pathlib import Path
import numpy as np
from best_common import load,select_sources,trial,write_wav
root=Path(sys.argv[1]);batch=root/'stock';batch.mkdir(exist_ok=False)
study=Path.home()/'Documents/Codex/Linkwise VOACAP/2026-11-15-main-retest.sajUbP'
legacy=study/'snr-sweep/fresh-sweep-20261117';manifest=[];controls=[]
for row in json.loads((legacy/'manifest.json').read_text()):
 if row['nominal_snr_db'] is not None:continue
 os.link(legacy/(row['name']+'.wav'),batch/(row['name']+'.wav'))
 manifest.append(dict(name=row['name'],kind='transport_control',expected_sha256=row['expected_sha256'],wav_sha256=row['wav_sha256']))
 controls.append(row['name'])
sources=select_sources(root);cleans=[]
noise_snrs=tuple(map(int,os.environ.get('STUDY_NOISE_SNRS','-8,-4,0,4').split(',')))
seed_base=int(os.environ.get('STUDY_SEED_BASE','996000'))
replicates=int(os.environ.get('STOCK_REPLICATES','2'))
for i,source in enumerate(sources):
 name=f'source-{i}';pcm=np.rint(load(root,source)*32768).astype('<i2');write_wav(batch/(name+'.wav'),pcm,stereo=True)
 manifest.append(dict(name=name,kind='source_clean',source=source['name'],site=source['site'],expected_sha256=source['expected_kiss_sha256'],wav_sha256=hashlib.sha256((batch/(name+'.wav')).read_bytes()).hexdigest()))
 cleans.append(name)
representatives=[];seen=set()
for i,source in enumerate(sources):
 if source['site'] in seen:continue
 seen.add(source['site']);representatives.append((i,source))
trials=[]
for i,source in representatives:
 for condition,snr in [('noise',s) for s in noise_snrs]+[('tone',0),('echo',0)]:
  for replicate in range(replicates):
   seed=seed_base+i*1000+(snr+8)*20+replicate;pcm,_,meta=trial(root,source,condition,snr,seed)
   name=f'best-{i}-{condition}-{snr:+03d}-{replicate}';write_wav(batch/(name+'.wav'),pcm,stereo=True)
   manifest.append(dict(name=name,kind='impaired',source=source['name'],site=source['site'],condition=condition,snr=snr,seed=seed,expected_sha256=source['expected_kiss_sha256'],
                        wav_sha256=hashlib.sha256((batch/(name+'.wav')).read_bytes()).hexdigest(),**meta))
   trials.append(name)
rng=np.random.default_rng(999001);rng.shuffle(trials);middle=len(trials)//2
queue=controls[:2]+cleans+trials[:middle]+controls[2:4]+trials[middle:]+controls[4:]
(batch/'manifest.json').write_text(json.dumps(manifest,indent=2));(batch/'queue.txt').write_text('\n'.join(queue)+'\n')
(batch/'protocol.txt').write_text('Stock VARA4.9.1;15s startup,3s interclip gaps;six known transport controls opening/middle/end;all selected new clean clips;first new source per site,first2 seeds per frozen condition. Same48k dual-mono PCM as Linkwise generated trials, verify hashes/playback completion/version. No95percent sensitivity claim; no code/waveform changes.\n')
tag=root.name;link=study/'snr-sweep'/tag;link.symlink_to(batch,target_is_directory=True)
(root/'stock-tag.txt').write_text(tag)
print('Stock tag',tag,'queue',len(queue),'impaired',len(trials),'new clean',len(cleans))
