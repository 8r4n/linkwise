#!/usr/bin/env bash
# Build a private UEFI USB disk from a prepared Rocky live filesystem.
set -euo pipefail
umask 077
here=$(cd "$(dirname "$0")" && pwd)
root=$(realpath "${1:?prepared live filesystem}")
key=$(realpath "${2:?LUKS passphrase file}")
login=$(realpath "${3:?operator password file}")
image=$(realpath -m "${4:?new raw USB path}")
[[ $root != / && -e $root/etc/system-fips && ! -e $image ]]
mountpoint -q "$root"
if ! rpm --root "$root" -q --whatprovides linkwise-full-runtime >/dev/null; then
    echo "Install the signed Linkwise RPM built with --full in the live root first." >&2
    exit 2
fi
[[ $(stat -c '%a' "$key") == 600 && $(stat -c '%a' "$login") == 600 ]]
if [[ ! -s $key || $(wc -l < "$key") != 0 ]]; then
    echo "The LUKS passphrase file must contain the exact passphrase without a newline." >&2
    exit 2
fi
memory_encrypt=
case "$(uname -m)" in
    aarch64) efi=aa64; serial=ttyAMA0; grub_linux=linux; grub_initrd=initrd ;;
    x86_64)
        efi=x64; serial=ttyS0; grub_linux=linuxefi; grub_initrd=initrdefi
        memory_encrypt=' mem_encrypt=on'
        ;;
    *) exit 2 ;;
esac
kernel=$(find "$root/boot" -maxdepth 1 -name 'vmlinuz-5.*' -printf '%f\n' | sort -V | tail -1)
version=${kernel#vmlinuz-}
[[ -n $kernel && -f $root/boot/.$kernel.hmac ]]

work=$(mktemp -d "$(dirname "$image")/usb-compose.XXXXXX")
mkdir "$work"/{efi,data}
module="$root/usr/lib/dracut/modules.d/97linkwise-memory-wipe"
test ! -e "$module"
loop=
mapper="linkwise-build-$$"
cleanup() {
    for path in "$work/data" "$work/efi" "$root/dev" "$root/sys" "$root/proc"; do
        if mountpoint -q "$path"; then umount "$path"; fi
    done
    if [[ -e /dev/mapper/$mapper ]]; then cryptsetup close "$mapper"; fi
    if [[ -n $loop ]]; then losetup -d "$loop"; fi
    if [[ -d $module ]]; then
        for file in module-setup.sh randomize.sh usb-identify.sh linkwise-memory-wipe; do
            if [[ -e $module/$file ]]; then unlink "$module/$file"; fi
        done
        rmdir "$module"
    fi
    rmdir "$work"/{efi,data} "$work"
}
for path in "$root/dev" "$root/sys" "$root/proc"; do ! mountpoint -q "$path" || exit 1; done
trap cleanup EXIT

truncate -s 5G "$image"
sgdisk -n 1:0:+256M -t 1:ef00 -c 1:LINKWISE_BOOT \
    -n 2:0:0 -t 2:8309 -c 2:LINKWISE_LUKS "$image" >/dev/null
loop=$(losetup --find --show --partscan "$image")
udevadm settle
boot="${loop}p1"
data="${loop}p2"
mkfs.vfat -F 32 -n LW_BOOT "$boot"
cryptsetup luksFormat --batch-mode --type luks2 --pbkdf pbkdf2 --hash sha256 \
    --cipher aes-xts-plain64 --key-size 512 --key-file "$key" "$data"
cryptsetup open --key-file "$key" "$data" "$mapper"
mkfs.ext4 -q -m 0 -E nodiscard -L LW_LIVE "/dev/mapper/$mapper"
mount "$boot" "$work/efi"
mount "/dev/mapper/$mapper" "$work/data"
boot_uuid=$(blkid -s UUID -o value "$boot")
luks_uuid=$(cryptsetup luksUUID "$data")

vendor="$root/boot/efi/EFI/rocky"
efi_dir="$work/efi/EFI/BOOT"
install -D "$vendor/shim$efi.efi" "$efi_dir/BOOT${efi^^}.EFI"
install "$vendor/gcd$efi.efi" "$efi_dir/grub$efi.efi"
install "$vendor/mm$efi.efi" "$efi_dir/mm$efi.efi"
install "$root/boot/$kernel" "$root/boot/.$kernel.hmac" "$work/efi/"
grub_hash=$(
    { cat "$login"; cat "$login"; } | LC_ALL=C grub2-mkpasswd-pbkdf2 |
        awk '/grub.pbkdf2.sha512/ {print $NF}'
)
[[ $grub_hash == grub.pbkdf2.sha512.* ]]
cat > "$efi_dir/grub.cfg" <<EOF
set timeout_style=menu
set timeout=1
set default=0
set superusers=lwadmin
password_pbkdf2 lwadmin $grub_hash
search --no-floppy --fs-uuid --set=root $boot_uuid
menuentry 'Start Linkwise Rocky 9 encrypted USB' --unrestricted {
    $grub_linux /$kernel root=live:/dev/mapper/luks-$luks_uuid rw rd.live.image rd.luks.uuid=luks-$luks_uuid fips=1 boot=UUID=$boot_uuid audit=1 audit_backlog_limit=8192 init_on_free=1 page_poison=1 quiet console=tty0 console=$serial,115200$memory_encrypt
    $grub_initrd /linkwise-live-initramfs.img
}
EOF
grub2-script-check "$efi_dir/grub.cfg"
unset grub_hash

install -Dm0755 "$here/shutdown-memory/module-setup.sh" "$module/module-setup.sh"
install -m 0755 "$here/shutdown-memory/randomize.sh" "$module/randomize.sh"
install -m 0644 "$here/shutdown-memory/usb-identify.sh" "$module/usb-identify.sh"
cc -O2 -Wall -Wextra -Werror -fPIE -pie -o "$module/linkwise-memory-wipe" "$here/shutdown-memory/wipe.c"
chmod 0755 "$module/linkwise-memory-wipe"
mount -t proc proc "$root/proc"
mount --bind /sys "$root/sys"
mount --bind /dev "$root/dev"
network_boot_modules='network network-manager network-legacy kernel-network-modules ifcfg url-lib livenet nfs cifs iscsi nbd nvmf fcoe fcoe-uefi ssh-client systemd-network-management systemd-networkd'
chroot "$root" dracut --force --zstd --no-hostonly \
    --omit "plymouth $network_boot_modules" \
    --add 'crypt dmsquash-live fips linkwise-memory-wipe' \
    "/var/tmp/linkwise-live-initramfs.img" "$version"
lsinitrd -m "$root/var/tmp/linkwise-live-initramfs.img" | grep -x 'linkwise-memory-wipe'
install "$root/var/tmp/linkwise-live-initramfs.img" "$work/efi/"
# dracut restores this copy after services stop and before switching roots.
install -m 0600 "$root/var/tmp/linkwise-live-initramfs.img" "$root/boot/initramfs-$version.img"
unlink "$root/var/tmp/linkwise-live-initramfs.img"
umount "$root/dev" "$root/sys" "$root/proc"

mkdir "$work/data"/{LiveOS,state}
cp -a "$root/home" "$root/var" "$work/data/state/"
mksquashfs "$root" "$work/data/LiveOS/squashfs.img" -noappend -comp zstd -Xcompression-level 12
sync
printf 'Created %s\n' "$image"
