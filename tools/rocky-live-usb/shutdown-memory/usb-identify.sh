#!/bin/sh

# Validate only the layout produced by compose-hardened-usb.sh.
linkwise_check_usb() {
    case "$1" in /dev/sd*) ;; *) return 1 ;; esac
    [ -b "$1" ] && [ -b "$1"1 ] && [ -b "$1"2 ] &&
        [ "$(lsblk -dnro TYPE,TRAN "$1")" = "disk usb" ] &&
        [ "$(blkid -p -s UUID -o value "$1"1)" = "$2" ] &&
        [ "$(blkid -p -s TYPE -o value "$1"1)" = vfat ] &&
        [ "$(blkid -p -s UUID -o value "$1"2)" = "$3" ] &&
        [ "$(blkid -p -s TYPE -o value "$1"2)" = crypto_LUKS ]
}
