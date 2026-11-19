import contextlib, importlib.util, io, json, tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
spec=importlib.util.spec_from_file_location('interop','tools/test-vara-interop.py'); mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod)
connected=b'CONNECTED KN6MQS W1A 500\r'
class Wire:
    def __init__(self,trial,role,command):self.trial,self.role,self.command=trial,role,command
    def sendall(self,payload):
        if self.command:
            report=connected if payload.startswith(b'CONNECT ') else b'DISCONNECTED\r' if payload==b'DISCONNECT\r' else b''
            for commands in self.trial.commands:commands.extend(report)
            return
        self.trial.writes.append((self.role,payload)); target=self.trial.received[1-self.role]
        target.extend(payload)
        if self.trial.fault=='wrong' and len(self.trial.writes)==1:target[-1]^=1
        if self.trial.fault=='duplicate' and len(self.trial.writes)==4:target.extend(payload)
        self.trial.commands[self.role].extend(b'BUFFER 0\r')
with tempfile.TemporaryDirectory() as root,contextlib.redirect_stdout(io.StringIO()),patch.object(mod.time,'sleep'):
    for case,sender in [('linkwise-responder',0),('linkwise-caller',1)]:
        for fault in (None,'wrong','duplicate'):
            args=SimpleNamespace(case=case,stock_awgn_seed=1,stock_binary_seed=90,bandwidth=500,payload_length=13,reply_length=6,late_caller_write_delay=None,verify_disconnect=True,verify_reconnect=True,drop_stock_ack=None,drop_linkwise_ack=None,replacement=None,raise_stock_rate=None,disconnect_from='caller',output=Path(root))
            t=mod.Trial(args,io.StringIO());t.writes=[];t.fault=fault
            try:t.exchange([(Wire(t,r,True),Wire(t,r,False),None) for r in (0,1)])
            except mod.FixtureFailure:
                assert fault
            else:
                assert not fault
                assert [r for r,payload in t.writes]==[sender,1-sender,sender,sender]
                assert all(payload.startswith(b'\x00\xff') for _,payload in t.writes[:3])
                oracle=json.loads((Path(root)/'host-oracle.json').read_text())
                assert oracle['sender_role']==sender and t.expected_streams==t.received
print('Both roles: exact binary and reconnect pass, wrong and duplicate DATA rejected (synthetic host fixture).')
