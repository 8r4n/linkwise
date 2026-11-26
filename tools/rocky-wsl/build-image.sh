#!/bin/bash
# Add a verified Linkwise RPM to the official Rocky Linux 9 WSL filesystem.
set -euo pipefail
rpm=${1:?"server RPM path"}
output=${2:?"existing image output directory"}
runtime=${LINKWISE_CONTAINER_RUNTIME:-docker}
filename=${rpm##*/}
if [[ ! $filename =~ ^linkwise-modem-[0-9]+\.[0-9]+\.[0-9]+-1\.rockymail\.(aarch64|x86_64)\.rpm$ ]]; then
    echo 'Expected a Linkwise Rocky server RPM for ARM64 or x86-64.' >&2
    exit 2
fi
arch=${BASH_REMATCH[1]}
case "$arch" in
    aarch64)
        platform=linux/arm64
        target=arm64
        base_sha256=ab78755389ccb4a88ddee18b9d84edf92bf98b30b3b998d7a1ad32ca65d22d0f
        builder_digest=d7be1c094cc5845ee815d4632fe377514ee6ebcf8efaed6892889657e5ddaaa6
        ;;
    x86_64)
        platform=linux/amd64
        target=x64
        base_sha256=55ca359092f71cb0d87ff199490f2e4375d78889eabb9fefec7956a957cc5c1c
        builder_digest=d644d203142cd5b54ad2a83a203e1dee68af2229f8fe32f52a30c6e1d3c3a9e0
        ;;
esac
if [[ ! -f $rpm || ! -f $rpm.sha256 || ! -d $output ]]; then
    echo 'Provide the RPM, its .sha256 sidecar, and an existing output directory.' >&2
    exit 2
fi
if ! command -v "$runtime" >/dev/null; then
    printf 'Container runtime unavailable: %s\n' "$runtime" >&2
    exit 127
fi
output=$(cd "$output" && pwd)
stage=$(mktemp -d "$output/.linkwise-wsl.XXXXXX")
trap 'rm -rf "$stage"' EXIT
cp -- "$rpm" "$rpm.sha256" "$stage/"
base="Rocky-9-WSL-Base-9.9-20261112.0.$arch.wsl"
if [[ -n ${LINKWISE_WSL_BASE:-} ]]; then
    cp -- "$LINKWISE_WSL_BASE" "$stage/$base"
fi
image="linkwise-rocky9-$target.wsl"
builder="docker.io/library/rockylinux@sha256:$builder_digest"
"$runtime" run --rm --interactive --platform "$platform" -v "$stage:/build:Z" -w /build \
    --device /dev/null:/rootfs/dev/null:rw \
    --device /dev/random:/rootfs/dev/random:r \
    --device /dev/urandom:/rootfs/dev/urandom:r \
    "$builder" bash -se -- "$arch" "$base" "$base_sha256" "$filename" "$image" <<'BUILD'
set -euo pipefail
arch=$1
base=$2
base_sha256=$3
rpm=$4
image=$5
test "$(uname -m)" = "$arch"
if [[ $(cat "$rpm.sha256") != "$(sha256sum "$rpm")" ]]; then
    echo 'Linkwise RPM checksum mismatch.' >&2
    exit 1
fi
metadata=$(rpm -qp --queryformat '%{NAME} %{RELEASE} %{ARCH}' "$rpm")
test "$metadata" = "linkwise-modem 1.rockymail $arch"
version=$(rpm -qp --queryformat '%{VERSION}' "$rpm")
if [[ ! -f $base ]]; then
    curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$base" \
        "https://dl.rockylinux.org/pub/rocky/9.9/images/$arch/$base"
fi
printf '%s  %s\n' "$base_sha256" "$base" | sha256sum -c -
root=/rootfs
tar -xf "$base" -C "$root" --numeric-owner --xattrs --xattrs-include='security.capability'
test -x "$root/usr/libexec/wsl/oobe.sh"
grep -Fxq 'systemd=true' "$root/etc/wsl.conf"
printf 'Rocky WSL base: '
cat "$root/etc/rocky-release"

# Supply DNS and the standard devices for the offline root during the DNF
# transaction. WSL creates its own runtime mounts and resolver at launch.
rm -f "$root/etc/resolv.conf"
cp -L /etc/resolv.conf "$root/etc/resolv.conf"
install -d -m 1777 "$root/tmp"
install -m 0644 "$rpm" "$root/tmp/$rpm"
chroot "$root" /usr/bin/dnf -y --setopt=tsflags= --setopt=localpkg_gpgcheck=0 install "/tmp/$rpm"
chroot "$root" /usr/bin/dnf clean all
rm -f "$root/tmp/$rpm" "$root/etc/resolv.conf"
rm -f "$root"/var/log/dnf*.log "$root/var/log/hawkey.log"

# Keep Rocky's first-launch account setup and assign a distinct Windows name.
sed 's/^defaultName[[:space:]]*=.*/defaultName = Linkwise-Rocky9/' \
    "$root/etc/wsl-distribution.conf" >wsl-distribution.conf
grep -Fxq 'defaultName = Linkwise-Rocky9' wsl-distribution.conf
rm "$root/etc/wsl-distribution.conf"
install -o 0 -g 0 -m 0644 wsl-distribution.conf "$root/etc/wsl-distribution.conf"
test "$(stat -c '%u:%g %a' "$root/etc/wsl.conf")" = '0:0 644'
awk -F: '$2 !~ /^[!*]+$/ { exit 1 }' "$root/etc/shadow"
awk -F: '$1 == "root" && $3 == "0" && $4 == "0" { found = 1 } END { exit !found }' "$root/etc/passwd"
test "$(chroot "$root" /usr/bin/rpm -q --queryformat '%{NAME} %{RELEASE} %{ARCH}' linkwise-modem)" = "$metadata"
chroot "$root" /usr/bin/rpm -V --noscripts linkwise-modem
chroot "$root" /usr/bin/linkwise --version
test "$(chroot "$root" /usr/bin/linkwise-modemd --version)" = "linkwise-modemd $version"
chroot "$root" /usr/bin/linkwise-modemd --simulate
for unit in linkwise-modem linkwise-mail-smtp linkwise-mail-imap; do
    test -f "$root/usr/lib/systemd/system/$unit.service"
    state=$(chroot "$root" /usr/bin/systemctl --root=/ is-enabled "$unit.service" || :)
    test "$state" = disabled
done

tar -C "$root" --numeric-owner --xattrs --xattrs-include='security.capability' \
    --exclude='./dev/*' --exclude='./proc/*' --exclude='./sys/*' --exclude='./run/*' -cf - . |
    gzip -n >"$image"
gzip -t "$image"
sha256sum "$image" >"$image.sha256"
BUILD
mv -- "$stage/$image" "$stage/$image.sha256" "$output/"
printf 'Built %s/%s\n' "$output" "$image"
