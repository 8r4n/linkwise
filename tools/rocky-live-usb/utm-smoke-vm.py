#!/usr/bin/env python3
"""Create an offline UTM QEMU bundle with a virtual Linkwise USB drive."""
import argparse
import os
from pathlib import Path
import plistlib
import shutil
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("iso", type=Path)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--architecture", choices=("aarch64", "x86_64"), default="aarch64")
    parser.add_argument("--serial-port", type=int, default=2249)
    parser.add_argument("--memory-mib", type=int, default=4096)
    parser.add_argument("--writable", action="store_true", help="allow writes to a private raw USB image")
    args = parser.parse_args()
    if not args.iso.is_file() or args.bundle.suffix != ".utm":
        parser.error("supply an existing ISO and a new .utm path")
    if not 1024 <= args.serial_port <= 65535:
        parser.error("serial port must be between 1024 and 65535")
    if args.memory_mib <= 0:
        parser.error("guest memory must be positive")

    template = Path(__file__).resolve().parent.parent / "rocky9-vm/utm-template.plist"
    config = plistlib.loads(template.read_bytes())
    identifier = str(uuid.uuid4()).upper()
    config["Information"].update(Name=args.bundle.stem, UUID=identifier, Notes="Linkwise virtual USB smoke test")
    config["Network"] = []
    config["Serial"][0]["TcpPort"] = args.serial_port
    config["System"].update(Architecture=args.architecture, MemorySize=args.memory_mib)
    if args.architecture == "x86_64":
        config["System"].update(Target="q35", CPU="max")
        config["QEMU"]["Hypervisor"] = False
    config["Drive"] = [
        dict(Identifier=str(uuid.uuid4()).upper(), ImageName="linkwise-usb.raw",
             ImageType="Disk", Interface="USB", InterfaceVersion=1, ReadOnly=not args.writable)
    ]
    args.bundle.mkdir(mode=0o700)
    data = args.bundle / "Data"
    data.mkdir()
    try:
        os.link(args.iso, data / "linkwise-usb.raw")
    except OSError:
        shutil.copyfile(args.iso, data / "linkwise-usb.raw")
    (args.bundle / "config.plist").write_bytes(plistlib.dumps(config))
    print(identifier)


if __name__ == "__main__":
    main()
