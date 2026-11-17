#!/usr/bin/env python3
"""Verify the live ISO shares its EFI image between USB and optical boot."""
import argparse
import os
from pathlib import Path
import re
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("architecture", choices=("aarch64", "x86_64"))
    parser.add_argument("iso", type=Path)
    args = parser.parse_args()
    efi = "BOOTAA64.EFI" if args.architecture == "aarch64" else "BOOTX64.EFI"
    paths = (f"/EFI/BOOT/{efi}", "/LiveOS/squashfs.img")
    report = subprocess.check_output(
        ["xorriso", "-indev", str(args.iso),
         "-report_system_area", "plain", "-report_el_torito", "plain",
         "-ls", *paths],
        env={**os.environ, "LC_ALL": "C"}, text=True,
    )
    print(report, end="")

    # xorriso prints the GPT type as little-endian on-disk bytes.
    esp = re.search(r"^GPT type GUID\s*:\s*(\d+)\s+28732ac11ff8d211ba4b00a0c93ec93b$", report, re.M)
    efi_boot = re.search(r"^El Torito boot img\s*:\s*\d+\s+UEFI\s+y\s+.*\s(\d+)$", report, re.M)
    if not esp or not efi_boot:
        raise SystemExit("Expected an EFI System Partition and a bootable UEFI El Torito entry")
    location = re.search(rf"^GPT start and size\s*:\s*{esp[1]}\s+(\d+)\s+(\d+)$", report, re.M)
    if not location or int(location[1]) != 4 * int(efi_boot[1]) or int(location[2]) == 0:
        raise SystemExit("USB and optical boot must share the EFI filesystem")
    if args.architecture == "x86_64" and not re.search(
        r"^El Torito boot img\s*:\s*\d+\s+BIOS\s+y\s", report, re.M
    ):
        raise SystemExit("Expected a bootable x86 BIOS El Torito entry")
    for path in paths:
        if f"'{path}'" not in report:
            raise SystemExit(f"Missing required live-media file {path}")


if __name__ == "__main__":
    main()
