"""Ordinary mixed-stock regression of an isolated research runtime.

Do not run concurrently with stock WAV replay: both use the same virtual cable.
"""
import hashlib,json,os,shutil,subprocess,sys,time
from pathlib import Path
root=Path(sys.argv[1]);repo=Path(__file__).resolve().parents[2]
u=Path.home()/'Applications/UTM.app/Contents/MacOS/utmctl';vm='Linkwise Windows 11 ARM64'
study=Path.home()/'Documents/Codex/Linkwise VOACAP/2026-11-15-main-retest.sajUbP';guest=r'C:\Users\Public\LinkwiseAudioLab'
binary=repo/'rust/target/release/linkwise-modemd-acceptance';frozen=root/'runtime/linkwise-modemd-acceptance';shutil.copy2(binary,frozen)
assert hashlib.sha256((repo/'rust/target/release/linkwise-modemd').read_bytes()).hexdigest()==(root/'original-daemon-sha256.txt').read_text().strip()
def utm(args,**kwargs):return subprocess.run([str(u),*args],check=True,capture_output=True,**kwargs)
for source,target in [('bridge.ps1','main-wasapi-bridge.ps1'),('bench-wrapper.ps1','main-bridge-wrapper.ps1'),('bench-launch.ps1','main-bridge-launch.ps1')]:
 utm(['file','push',vm,guest+'\\'+target],input=(study/source).read_bytes())
setup=r"$has=Get-NetTCPConnection -LocalPort 8300 -State Listen -ErrorAction SilentlyContinue;if(!$has){Start-ScheduledTask -TaskName LinkwiseAcquisitionVARA};Start-Sleep -Seconds 3;@{listening=[bool](Get-NetTCPConnection -LocalPort 8300 -State Listen -ErrorAction SilentlyContinue)}|ConvertTo-Json|Set-Content C:\Users\Public\LinkwiseAudioLab\acceptance-status.json"
utm(['exec',vm,'--cmd','powershell.exe','-NoProfile','-Command',setup])
for _ in range(15):
 time.sleep(1)
 p=subprocess.run([str(u),'file','pull',vm,guest+'\\acceptance-status.json'],capture_output=True,text=True)
 try:
  if json.loads(p.stdout.lstrip('\ufeff'))['listening']:break
 except (ValueError,KeyError):pass
else:raise RuntimeError('Dedicated stock test listener8300 not ready')
results=[]
for role in ('caller','responder'):
 output=root/('interop-'+role)
 command=[sys.executable,str(repo/'tools/test-vara-interop.py'),'linkwise-'+role,str(output),'--modem',str(frozen),'--profile',str(Path.home()/'Documents/Codex/linkwise-kiwi-field.VtgdnY/profile.json'),
          '--stock-bridge-base','28760','--payload-length','13','--verify-disconnect']
 with (root/('interop-'+role+'.log')).open('w') as log:
  p=subprocess.Popen(command,stdout=log,stderr=subprocess.STDOUT,env={**os.environ,'SOFT_METHOD':'stable'})
  time.sleep(2);utm(['exec',vm,'--cmd','powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',guest+'\\main-bridge-launch.ps1'])
  status=p.wait(timeout=180)
  bridge=subprocess.run([str(u),'file','pull',vm,guest+'\\main-bridge.log'],capture_output=True);(root/('bridge-'+role+'.log')).write_bytes(bridge.stdout)
 result=json.loads((output/'result.json').read_text());results.append(result);print(role,status,result['result'],result.get('reason',''),flush=True)
 if status:break
(root/'interop-summary.json').write_text(json.dumps(dict(results=results,method='stable',binary_sha256=hashlib.sha256(frozen.read_bytes()).hexdigest(),scope='Isolated ordinary13+6+13-byte BW500 mixed transfers; not proof that the new weighted level2 fallback was exercised under interference.'),indent=2))
