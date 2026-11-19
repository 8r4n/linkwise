import json
import struct
from datetime import datetime, timezone
from kiwi.client import KiwiSDRStream
import kiwirecorder

base = KiwiSDRStream._process_aud
def observe(self, body):
    flags, sequence, smeter = struct.unpack("<BI", body[:5]) + struct.unpack(">H", body[5:7])
    print("PACKET", json.dumps([datetime.now(timezone.utc).isoformat(), sequence, smeter / 10 - 127, flags]), flush=True)
    return base(self, body)

KiwiSDRStream._process_aud = observe
kiwirecorder.main()
