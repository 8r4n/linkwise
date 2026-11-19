"""Bounded research collection, two named shortlist sites at a time.

Usage: python collect-best.py OUTPUT_ROOT SITE1 SITE2 [DIAL_KHZ]
"""
import concurrent.futures,hashlib,json,os,subprocess,sys,time,urllib.request
from datetime import datetime,timezone
from pathlib import Path
root=Path(sys.argv[1]);names=sys.argv[2:4];frequency=sys.argv[4] if len(sys.argv)>4 else '7105'
sites={'chichester':('g8ure.ddns.net',8079),'glava':('sa4bna.hopto.org',8073),
       'westminster':('sdr.hfunderground.com',8073),'free-union':('21748.proxy.kiwisdr.com',80),
       'westminster-beverage':('sdr.hfunderground.com',8076),
       'warrenton':('kiwisdr.k1ra.us',8075),'newton':('24.218.56.55',8073),
       'hameenlinna':('oh3aa.ddns.net',18073)}
client=Path.home()/'Documents/Codex/Linkwise KiwiSDR collection/_tools/kiwiclient'
revision=subprocess.check_output(['git','-C',str(client),'rev-parse','HEAD'],text=True).strip()
assert revision=='4eb733e6b6147f7fbeb97ced64cdac029b202d18'
def utc():return datetime.now(timezone.utc).isoformat()
def record(name):
 host,port=sites[name];folder=root/(name+'-'+frequency+os.environ.get('CAPTURE_TAG',''));folder.mkdir(exist_ok=False)
 row=dict(site=name,receiver=f'http://{host}:{port}',dial_khz=float(frequency),mode='USB',passband_hz=[250,3150],kiwiclient_revision=revision,
          source_url='https://www.varac-hamradio.com/',requested_seconds=180,start_utc=utc())
 try:
  with urllib.request.urlopen(row['receiver']+'/status',timeout=5) as f:raw=f.read().decode()
  (folder/'status.txt').write_text(raw);status=dict(x.split('=',1) for x in raw.splitlines() if '=' in x)
  row.update(antenna=status.get('antenna'),gps=status.get('gps'),receiver_name=status.get('name'),hardware=status.get('sdr_hw'))
  if status.get('status')!='active' or status.get('offline')!='no' or int(status['users'])>=int(status['users_max']):
   row['skip']='Receiver unavailable or full';return
  command=[sys.executable,'-u',str(root/'record.py'),'-s',host,'-p',str(port),'-f',frequency,'-m','usb','-L','250','-H','3150',
           '--ncomp','--OV','--tlimit','180','--connect-timeout','4','--connect-retries','1','--busy-timeout','4','--busy-retries','1',
           '-k','8','--log','info','-q','-d',str(folder),'-u','Linkwise-RX']
  with (folder/'capture.log').open('w') as log:
   p=subprocess.Popen(command,stdout=log,stderr=subprocess.STDOUT,env={**os.environ,'PYTHONPATH':str(client)})
   deadline=time.monotonic()+195
   while p.poll() is None:
    text=(folder/'capture.log').read_text()
    if time.monotonic()>deadline or text.count('Reconnecting after')>=2:
     row['termination_reason']='Wall-clock bound or second connection failure';p.terminate();break
    time.sleep(.25)
   try:p.wait(timeout=4)
   except subprocess.TimeoutExpired:p.kill();p.wait()
  row['returncode']=p.returncode
 except Exception as error:row['failure']=str(error)
 finally:
  row['end_utc']=utc();row['files']=[]
  for path in folder.glob('*.wav'):row['files'].append(dict(name=path.name,sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
  (folder/'capture.json').write_text(json.dumps(row,indent=2))
  print(name,row.get('skip') or row.get('failure') or f"return={row.get('returncode')} WAVs={len(row['files'])}",flush=True)
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:list(pool.map(record,names))
