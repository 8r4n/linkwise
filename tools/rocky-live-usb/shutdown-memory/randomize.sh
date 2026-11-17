#!/bin/sh

shred_request=/linkwise-shred-request
usb_busy=
# Device-mapper teardown can take more than one dracut shutdown pass.
for uuid in /sys/block/dm-*/dm/uuid; do
    [ -r "$uuid" ] || continue
    read -r value < "$uuid"
    case "$value" in
        CRYPT-LUKS*)
            [ "$1" = final ] || return 1
            echo "Linkwise shutdown: an encrypted mapping remains active"
            usb_busy=yes
            break
            ;;
    esac
done

if [ -f "$shred_request" ]; then
    . /usr/libexec/linkwise-usb-identify
    read -r disk boot_uuid luks_uuid < "$shred_request"
    if [ -n "$usb_busy" ] || [ -z "$boot_uuid" ] || [ -z "$luks_uuid" ] ||
        ! linkwise_check_usb "$disk" "$boot_uuid" "$luks_uuid"; then
        echo "Linkwise shutdown: boot USB erase refused because identity or teardown could not be verified"
    elif cryptsetup erase --batch-mode "$disk"2 &&
        boot_bytes=$(blockdev --getsize64 "$disk"1) &&
        dd if=/dev/zero of="$disk"1 bs=1M iflag=count_bytes count="$boot_bytes" conv=fsync status=none &&
        wipefs --all --force "$disk" && blockdev --flushbufs "$disk"; then
        echo "Linkwise shutdown: erased USB LUKS keyslots and public boot data"
    else
        echo "Linkwise shutdown: USB erase incomplete"
    fi
fi

if ! /usr/libexec/linkwise-memory-wipe; then
    echo "Linkwise shutdown: memory overwrite stopped early"
fi
return 0
