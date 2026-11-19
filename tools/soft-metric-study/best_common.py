"""Shared deterministic input generation and hash-only scoring for best-site retest."""
import hashlib,json,os,re,subprocess,time,wave
from pathlib import Path
import numpy as np
METHODS=tuple(os.environ.get('STUDY_METHODS','baseline,tone,gated').split(','))
assert METHODS and METHODS[0]=='baseline' and set(METHODS)<={'baseline','tone','gated','iterated','stable'}
def converted_wav(folder, original):
 return folder/'converted'/original['sha256']/original['name']

def load(root,source):
 path=root/source['path'];assert hashlib.sha256(path.read_bytes()).hexdigest()==source['wav_sha256']
 with wave.open(str(path)) as f:
  assert f.getframerate()==48000 and f.getnchannels()==1 and f.getsampwidth()==2
  return np.frombuffer(f.readframes(f.getnframes()),dtype='<i2').astype(float)/32768
def select_sources(root):
 sources=json.loads((root/'sources.json').read_text());chosen=[]
 for site in sorted({r['site'] for r in sources}):
  seen=set()
  for row in sorted((r for r in sources if r['site']==site),key=lambda r:(r['expected_kiss_sha256'],r['name'])):
   if row['expected_kiss_sha256'] in seen:continue
   seen.add(row['expected_kiss_sha256']);chosen.append(row)
   if len(seen)==3:break
 return chosen
def band_noise(n,rng):
 bins=np.fft.rfftfreq(n,1/48000);spectrum=np.fft.rfft(rng.standard_normal(n));spectrum[(bins<300)|(bins>3300)]=0
 return np.fft.irfft(spectrum,n=n)
def quantize(audio):
 gain=min(1,.9/np.max(np.abs(audio)))
 pcm=np.clip(np.rint(audio*gain*32768),-32768,32767).astype('<i2')
 return pcm,pcm.astype('<f4')/32768,gain
def trial(root,source,condition,snr,seed):
 x=load(root,source);rng=np.random.default_rng(seed);active=slice(96000,329472);rms=np.sqrt(np.mean(x[active]**2));signal=x.copy()
 noise=band_noise(len(x),rng);noise*=rms*10**(-snr/20)/np.sqrt(np.mean(noise[active]**2))
 if condition=='tone':signal+=rms*np.sqrt(2)*np.sin(2*np.pi*1453*np.arange(len(x))/48000+.3)
 elif condition=='echo':signal[96:]+=.6*x[:-96]
 else:assert condition=='noise'
 pcm,audio,gain=quantize(signal+noise)
 measured=10*np.log10(np.mean(x[active]**2)/np.mean(noise[active]**2))
 return pcm,audio,dict(input_sha256=hashlib.sha256(pcm.tobytes()).hexdigest(),recording_added_noise_ratio_db=float(measured),common_gain=float(gain))
def invoke(root,audio,method,temp='input.f32'):
 path=root/temp;audio.astype('<f4').tofile(path);start=time.perf_counter()
 result=subprocess.run([str(root/'prototype/replay'),str(path)],env=dict(os.environ,SOFT_METHOD=method),capture_output=True,text=True,check=True,timeout=120)
 seconds=time.perf_counter()-start;wires=set();kiss=set();frames=[]
 for line in result.stdout.splitlines():
  parts=line.split('\t');level,epoch,wire=parts[:3]
  wh=hashlib.sha256(bytes.fromhex(wire)).hexdigest();wires.add(wh)
  if len(parts)==4:kiss.add(hashlib.sha256(bytes.fromhex(parts[3])).hexdigest())
  frames.append(dict(level=int(level),start=int(epoch),wire_sha256=wh))
 attempts,active=map(int,re.search(r'GATES,(\d+),(\d+)',result.stderr).groups())
 return dict(wires=wires,kiss=kiss,seconds=seconds,gate_attempts=attempts,gate_active=active,frames=frames)
def write_wav(path,pcm,stereo=False):
 if stereo:pcm=np.repeat(pcm[:,None],2,axis=1).reshape(-1)
 with wave.open(str(path),'wb') as f:f.setnchannels(2 if stereo else 1);f.setsampwidth(2);f.setframerate(48000);f.writeframes(pcm.astype('<i2').tobytes())
