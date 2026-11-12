"""Create a PRBS9 file using the polynomial in MIL-STD-188-110C Appendix B."""
from pathlib import Path
import hashlib
import json
import sys

out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
items = []
for peer, seed in (("linkwise", 0x1FF), ("vara", 0x155)):
    state = seed
    bits = []
    visited = set()
    for i in range(512):
        if i < 511:
            assert state and state not in visited
            visited.add(state)
        else:
            assert state == seed
        bits.append(state & 1)
        feedback = ((state >> 7) ^ (state >> 6) ^ (state >> 4) ^ state) & 1
        state = (state >> 1) | (feedback << 8)
    payload = bytes(sum(bits[i + bit] << (7 - bit) for bit in range(8))
                    for i in range(0, len(bits), 8))
    filename = f"prbs9-{peer}.bin"
    (out / filename).write_bytes(payload)
    items.append({"filename": filename, "seed": hex(seed), "bytes": len(payload),
                  "sha256": hashlib.sha256(payload).hexdigest()})
manifest = {"polynomial": "x^9 + x^7 + x^6 + x^4 + 1", "period_bits": 511,
            "output": "512 bits per file, packed MSB-first, with the first bit repeated at the end",
            "register": "emit bit 0; shift right; insert bit 7 XOR bit 6 XOR bit 4 XOR bit 0 at bit 8",
            "attachments": items}
(out / "PRBS9 manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
