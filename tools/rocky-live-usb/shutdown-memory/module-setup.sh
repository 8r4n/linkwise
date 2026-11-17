#!/bin/bash

check() { return 255; }
depends() { echo "shutdown"; }

install() {
    inst_binary "$moddir/linkwise-memory-wipe" /usr/libexec/linkwise-memory-wipe
    inst_simple "$moddir/usb-identify.sh" /usr/libexec/linkwise-usb-identify
    inst_multiple blkid blockdev cryptsetup dd findmnt lsblk wipefs
    inst_hook shutdown 99 "$moddir/randomize.sh"
}
