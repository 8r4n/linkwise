"""Install the pinned offline Cesium distribution and regenerate Rust replay fixtures."""
import argparse
import base64
import hashlib
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parent
VERSION = '1.130.0'
INTEGRITY = 'OgmmjStUoRhLVA2YnMt6eK3p2vwePZ2dw4ukpze8cVdkUWlxM6rtboldOgZvg1LLnpeEpcGOB5h6+sNuT6a56w=='


def install():
    vendor = ROOT / 'vendor'
    if (vendor / 'Cesium.js').exists():
        return
    with tempfile.TemporaryDirectory(dir=ROOT) as directory:
        archive = Path(directory) / 'cesium.tgz'
        url = f'https://registry.npmjs.org/cesium/-/cesium-{VERSION}.tgz'
        urllib.request.urlretrieve(url, archive)
        digest = base64.b64encode(hashlib.sha512(archive.read_bytes()).digest()).decode()
        if digest != INTEGRITY:
            raise ValueError('Cesium archive checksum mismatch')
        build = Path(directory) / 'build'
        build.mkdir()
        with tarfile.open(archive) as package:
            for member in package:
                prefix = 'package/Build/Cesium/'
                if member.isfile() and member.name.startswith(prefix):
                    relative = Path(member.name[len(prefix):])
                    if relative.is_absolute() or '..' in relative.parts:
                        raise ValueError('invalid archive path')
                    target = build / relative
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with target.open('wb') as out:
                        shutil.copyfileobj(package.extractfile(member), out)
            (build / 'LICENSE.md').write_bytes(package.extractfile('package/LICENSE.md').read())
        build.rename(vendor)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--replays', action='store_true', help='regenerate bundled traces with Cargo')
    args = parser.parse_args()
    install()
    if not args.replays:
        raise SystemExit(0)
    for name in ('six', 'global-100', 'global-1000'):
        command = ['cargo','run','--quiet','--locked','--manifest-path',str(ROOT.parent/'rust/network/Cargo.toml'),'--','replay',name,'17',str(ROOT/'data/forecast.json')]
        (ROOT/'data'/f'{name}.json').write_bytes(subprocess.check_output(command))
