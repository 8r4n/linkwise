#!/usr/bin/env python3
"""Stage the Rocky Lorax overrides for hybrid USB boot and RAM mode."""
import argparse
from pathlib import Path
import re
import shutil

from pylorax import find_templates


def replace_once(text, old, new, source):
    if text.count(old) != 1:
        raise SystemExit(f"Unexpected Lorax template in {source}: {old}")
    return text.replace(old, new)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("architecture", choices=("aarch64", "x86_64"))
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    templates = args.destination / "live"
    shutil.copytree(Path(find_templates()) / "live", templates)

    arch = "x86" if args.architecture == "x86_64" else "aarch64"
    boot_template = templates / f"{arch}.tmpl"
    original = ' -eltorito-alt-boot -e {0} -no-emul-boot'
    # Refer to the same FAT image over El Torito and through the disk's GPT.
    hybrid = (
        ' -append_partition 2 0xef {1}/{0} -appended_part_as_gpt'
        ' -eltorito-alt-boot -e --interval:appended_partition_2:all:: -no-emul-boot'
    )
    if arch == "x86":
        old = 'efiargs += "' + original + ' {1}".format(img, hybrid)'
        new = (
            'if img == "images/efiboot.img":\n'
            '            efiargs += "' + hybrid + '".format(img, outroot)\n'
            '        else:\n'
            '            ' + old
        )
    else:
        old = 'efiargs += "' + original + '".format(img)'
        new = 'efiargs += "' + hybrid + '".format(img, outroot)'
    template = replace_once(boot_template.read_text(), old, new, boot_template)
    # Lorax already created the runtime in the output tree.
    template = replace_once(template, "install ${runtime_img} ${LIVEDIR}/squashfs.img",
                            "hardlink ${runtime_img} ${LIVEDIR}/squashfs.img", boot_template)
    boot_template.write_text(template)

    menu = templates / "config_files" / arch / "grub2-efi.cfg"
    config = replace_once(menu.read_text(), "set timeout=60", "set timeout=10", menu)
    start = re.findall(r"^menuentry 'Start @PRODUCT@ @VERSION@'[^\n]*\n[^}]+\n}", config, re.M)
    if len(start) != 1:
        raise SystemExit(f"Could not identify the default launch entry in {menu}")
    ram = replace_once(start[0], "'Start @PRODUCT@ @VERSION@'",
                       "'Start @PRODUCT@ @VERSION@ from RAM'", menu)
    ram = replace_once(ram, "rd.live.image quiet", "rd.live.image rd.live.ram quiet", menu)
    menu.write_text(config.rstrip() + "\n" + ram + "\n")


if __name__ == "__main__":
    main()
