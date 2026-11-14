#!/bin/bash
# Create a fresh VM bundle, verified Rocky media and per-VM credentials.
set -euo pipefail
umask 077
here=$(cd "$(dirname "$0")" && pwd)
base="$HOME/Virtual Machines"
work="$base/Linkwise Rocky9 Evidence"
bundle="$base/Linkwise Rocky 9 ARM64.utm"
utm="$HOME/Applications/UTM.app"
test ! -e "$work"
test ! -e "$bundle"
mkdir -p "$base"
mkdir "$work" "$bundle" "$bundle/Data"
mkfile -n 32g "$bundle/Data/Rocky.raw"
openssl rand -hex 24 > "$work/disk-unlock.txt"
openssl rand -hex 24 > "$work/admin-password.txt"
ssh-keygen -q -t rsa -b 3072 -N '' -f "$work/admin-rsa"
media=https://dl.rockylinux.org/pub/rocky/9
curl -fsSL --retry 3 "$media/isos/aarch64/Rocky-9.8-aarch64-boot.iso" -o "$work/Rocky-9.8-aarch64-boot.iso"
curl -fsSL --retry 3 "$media/BaseOS/aarch64/os/.treeinfo" -o "$work/treeinfo"
for name in vmlinuz initrd.img; do
    curl -fsSL --retry 3 "$media/BaseOS/aarch64/os/images/pxeboot/$name" -o "$work/$name"
done
python3 - "$work" "$bundle" "$here/utm-template.plist" <<'PY'
import configparser, hashlib, plistlib, sys, uuid
from pathlib import Path
work, vm, source = map(Path, sys.argv[1:])
c = plistlib.loads(source.read_bytes())
digests = configparser.ConfigParser()
digests.read(work / 'treeinfo')
expected = {'Rocky-9.8-aarch64-boot.iso': 'c40dd3accd0fb08a9848efcddfb29d0b6ba2a43e58a0d54b711e603c28fe9938'}
for name in ('vmlinuz', 'initrd.img'):
    expected[name] = digests['checksums']['images/pxeboot/' + name].split(':', 1)[1]
for name, digest in expected.items():
    with (work / name).open('rb') as f:
        sha = hashlib.sha256()
        for piece in iter(lambda: f.read(1024 * 1024), b''):
            sha.update(piece)
    if sha.hexdigest() != digest:
        raise SystemExit(f'{name}: SHA-256 mismatch')
c['Information'].update(Name='Linkwise Rocky 9 ARM64', Notes='Linkwise security assessment guest', UUID=str(uuid.uuid4()).upper())
c['Drive'] = [dict(Identifier=str(uuid.uuid4()).upper(), ImageName='Rocky.raw', ImageType='Disk',
                   Interface='VirtIO', InterfaceVersion=1, ReadOnly=False)]
c['Input']['UsbSharing'] = False
c['Sound'] = []
c['System'].update(CPUCount=2, MemorySize=4096)
c['Network'][0]['MacAddress'] = '52:54:00:9b:15:7a'
c['Network'][0]['PortForward'][0]['HostPort'] = 2224
c['Serial'][0]['TcpPort'] = 2234
(vm / 'config.plist').write_bytes(plistlib.dumps(c))
PY
python3 "$here/prepare.py"
open -a "$utm" "$bundle"
python3 "$here/lw-rocky-console.py" "$work/install-live.log" &
serial=$!
"$utm/Contents/MacOS/utmctl" start 'Linkwise Rocky 9 ARM64'
wait "$serial"
