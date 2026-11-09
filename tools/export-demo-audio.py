#!/usr/bin/env python3
"""Export native Rust demo audio as portable PCM16 WAVs plus verification metadata.

Start cargo run --locked --features demo --bin modem-demo in rust/ first.
Uses only the Python standard library; no downloaded stock recordings are copied.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import struct
import urllib.request
import wave


def pcm16(raw):
    if raw[:4] != b"RIFF" or raw[8:12] != b"WAVE":
        raise ValueError("expected RIFF WAV")
    offset, fmt, data = 12, None, None
    while offset + 8 <= len(raw):
        tag, size = struct.unpack_from("<4sI", raw, offset)
        chunk = raw[offset + 8:offset + 8 + size]
        if tag == b"fmt ":
            fmt = struct.unpack_from("<HHIIHH", chunk)
            if fmt[0] == 65534 and len(chunk) >= 40:
                if chunk[24:40] != bytes.fromhex("0300000000001000800000aa00389b71"):
                    raise ValueError("expected IEEE float extensible WAV")
                fmt = (3, *fmt[1:])
        elif tag == b"data":
            data = chunk
        offset += 8 + size + size % 2
    if not fmt or fmt[0:3] != (3, 1, 48000) or fmt[5] != 32 or data is None:
        raise ValueError("expected mono 48 kHz float32 source")
    samples = [max(-32768, min(32767, round(value[0] * 32768)))
               for value in struct.iter_unpack("<f", data)]
    output = io.BytesIO()
    with wave.open(output, "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(48000)
        wav.writeframes(struct.pack(f"<{len(samples)}h", *samples))
    return output.getvalue()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default="http://127.0.0.1:8787")
    parser.add_argument("--output", type=Path,
                        default=Path(__file__).resolve().parents[1] / "docs/media/modem")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    manifest = []
    for level, snr in [(1, None), (2, None), (3, None), (3, 24)]:
        request = dict(source="native", level=level, message="HELLO FROM LINKWISE",
                       snr_db=snr, seed=20261109, dropout=False)
        req = urllib.request.Request(args.url + "/api/run", json.dumps(request).encode(),
                                     {"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=30) as response:
            report = json.load(response)
        if not report["payload_verified"] or report["decoded_text"] != request["message"]:
            raise ValueError("Rust roundtrip did not verify")
        with urllib.request.urlopen(args.url + f'/api/runs/{report["id"]}/audio.wav') as response:
            audio = pcm16(response.read())
        name = f'bw500-level{level}-{"clean" if snr is None else "snr24db"}.wav'
        (args.output / name).write_bytes(audio)
        manifest.append(dict(file=name, request=request, duration_s=report["duration_s"],
                             decoded_text=report["decoded_text"],
                             payload_verified=report["payload_verified"],
                             frames=report["frames"],
                             sha256=hashlib.sha256(audio).hexdigest(),
                             encoding="PCM16 mono 48000 Hz; quantized from Rust float32 export"))
        print(name, f'{report["duration_s"]:.2f}s', "verified before PCM16 conversion")
    (args.output / "audio-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
