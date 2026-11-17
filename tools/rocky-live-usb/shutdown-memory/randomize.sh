#!/bin/sh

shred_request=/linkwise-shred-request

release_live_usb() {
    # Dracut moves the old /run to /oldsys/run, then unmounts /oldroot.
    # The read-only SquashFS lower mount and its encrypted backing mount
    # can survive that pass because neither path is under /oldroot.
    if findmnt -rn --mountpoint /oldroot >/dev/null; then
        return 1
    fi
    lower_type=$(findmnt -rn -o FSTYPE --mountpoint /oldsys/run/rootfsbase || :)
    if [ -n "$lower_type" ]; then
        [ "$lower_type" = squashfs ] && umount /oldsys/run/rootfsbase || return 1
    fi
    for live_mount in /live /oldsys/run/initramfs/live; do
        source=$(findmnt -rn -o SOURCE --mountpoint "$live_mount" || :)
        [ -n "$source" ] || continue
        [ "$source" -ef "$boot_mapper" ] && umount "$live_mount" || return 1
    done
    # The late shutdown initramfs has no udev worker to complete a DM cookie.
    DM_DISABLE_UDEV=1 cryptsetup close "$boot_mapper"
}

boot_mapper=$(getarg root=)
boot_mapper=${boot_mapper#live:}
case "$boot_mapper" in
    /dev/mapper/luks-*) ;;
    *) boot_mapper= ;;
esac
if [ -n "$boot_mapper" ] && [ -b "$boot_mapper" ]; then
    release_live_usb || :
fi

usb_busy=
for uuid in /sys/block/dm-*/dm/uuid; do
    [ -r "$uuid" ] || continue
    read -r value < "$uuid"
    case "$value" in
        CRYPT-LUKS*) usb_busy=yes; break ;;
    esac
done
if [ -n "$usb_busy" ]; then
    # Retry after the other dracut hooks have finished releasing devices.
    [ "$1" = final ] || return 1
    echo "Linkwise shutdown: an encrypted mapping remains active"
fi

if [ -f "$shred_request" ]; then
    . /usr/libexec/linkwise-usb-identify
    read -r disk boot_uuid luks_uuid < "$shred_request"
    if [ -n "$usb_busy" ] || [ "$boot_mapper" != "/dev/mapper/luks-$luks_uuid" ] ||
        [ -z "$boot_uuid" ] || [ -z "$luks_uuid" ] ||
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
