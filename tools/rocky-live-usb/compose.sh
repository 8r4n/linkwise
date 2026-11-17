#!/usr/bin/env bash
# Run as root in a disposable Rocky 9 VM with lorax-lmc-novirt installed.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
rpm=$(realpath "${1:?signed Linkwise RPM}")
signer=$(realpath "${2:?public RPM signing key}")
profile=$(realpath "${3:?local BW500 measurement profile}")
output=$(realpath -m "${4:?new output directory}")
root_only=${5:-}
[[ -z $root_only || $root_only == --root-only ]]
arch=$(uname -m)
case "$arch" in aarch64|x86_64) ;; *) echo "Unsupported architecture: $arch" >&2; exit 2 ;; esac
test "$(rpm -E '%{rhel}')" = 9
test "$(rpm -qp --qf '%{NAME} %{ARCH}' "$rpm")" = "linkwise-modem $arch"
test "$(id -u)" = 0
# Paths substituted into the Kickstart are literal shell words.
case "$output" in *[!a-zA-Z0-9/_.-]*) echo "Use a simple absolute output path." >&2; exit 2 ;; esac
mkdir "$output"
mkdir "$output"/{payload,payload/share,rpmdb,tmp}
rpmkeys --dbpath "$output/rpmdb" --import "$signer"
LC_ALL=C rpmkeys --dbpath "$output/rpmdb" --checksig --verbose "$rpm" | tee "$output/rpm-signature.log"
grep -Eq 'Signature.*: OK$' "$output/rpm-signature.log"
if [[ -n $root_only ]] && ! rpm -qp --provides "$rpm" | grep '^linkwise-full-runtime = ' >/dev/null; then
    echo "The hardened USB requires a signed Linkwise RPM built with --full." >&2
    exit 2
fi

payload="$output/payload"
install -m 0644 "$rpm" "$payload/linkwise.rpm"
install -m 0644 "$signer" "$payload/signer.asc"
printf '%s  %s\n' 7fe1eed44ca50f20cb7f64e1e70b9e21d8fe9c348a920609b9888bfdde82bb48 "$profile" | sha256sum -c
install -m 0644 "$profile" "$payload/share/bw500-level4.json"
install -m 0644 "$here/demo.py" "$here/station-check.py" "$repo/tools/test-varac-chat.py" \
    "$repo/tools/test-virtual-bw500.py" "$repo/rust/docs/modem-setup.md" "$payload/share/"
install -m 0644 "$here/linkwise-live.desktop" "$here/linkwise-station-check.desktop" \
    "$here/linkwise-live-smoke.service" "$payload/"
if [[ -n $root_only ]]; then
    efi=x64
    if [[ $arch == aarch64 ]]; then efi=aa64; fi
    source="$here/linkwise-minimal.ks.in"
    sed "s/@ARCH@/$arch/g; s/@EFI@/$efi/g" "$source" > "$output/linkwise-live.ks"
else
    python3 "$here/prepare-lorax.py" "$arch" "$output/templates"
    revision=515f6821c382578a70671fac765c9e0f701bf8fd
    source="https://raw.githubusercontent.com/rocky-linux/kickstarts/$revision/Rocky-9-XFCE-$arch.ks"
    curl --fail --location --silent --show-error --retry 3 "$source" -o "$output/rocky-xfce.ks"
    # Keep the final root partition and defer relabeling to the final post script.
    sed -e 's|http://dl.rockylinux.org/|https://dl.rockylinux.org/|g' \
        -e '/^part \/ --fstype="ext4" --size=5120$/d' \
        -e 's|^part / --size=|part / --fstype="ext4" --size=|' \
        -e '\|^/usr/sbin/fixfiles -R -a restore$|d' \
        "$output/rocky-xfce.ks" > "$output/linkwise-live.ks"
fi
sed "s|@PAYLOAD@|$payload|g; s|@INVENTORY@|$output/packages.txt|g" "$here/linkwise-live.ks.in" >> "$output/linkwise-live.ks"
ksvalidator -v RHEL9 "$output/linkwise-live.ks"

{
    printf 'Linkwise checkout: %s\n' "$(git -C "$repo" rev-parse HEAD)"
    printf 'Kickstart source: %s\n' "$source"
    sha256sum "$rpm" "$signer" "$profile"
    rpm -q lorax-lmc-novirt anaconda-tui lorax-templates-rocky
} > "$output/build-inputs.txt"

iso="Linkwise-Rocky9-Live-$arch.iso"
console=ttyS0
if [ "$arch" = aarch64 ]; then console=ttyAMA0; fi
if [[ -n $root_only ]]; then
    livemedia-creator --make-iso --no-virt --image-only --ks "$output/linkwise-live.ks" \
        --tmp "$output/tmp" --logfile "$output/livemedia.log"
    shopt -s nullglob
    images=("$output"/tmp/lmc-disk-*.img)
    test "${#images[@]}" = 1
    mv "${images[0]}" "$output/live-root.img"
    printf '%s\n' "$output/live-root.img"
    exit 0
fi
livemedia-creator --make-iso --no-virt --iso-only --squashfs-only --iso-name "$iso" \
    --project 'Linkwise Rocky Linux' --releasever 9 --volid "Linkwise-Live-$arch" \
    --extra-boot-args "console=$console,115200 console=tty0" \
    --lorax-templates "$output/templates" \
    --ks "$output/linkwise-live.ks" --tmp "$output/tmp" \
    --logfile "$output/livemedia.log"
# Lorax copies the entire ISO when --resultdir is supplied. Its staging
# directory is on the same filesystem, so move the finished image instead.
shopt -s nullglob
images=("$output"/tmp/lmc-result-*/"$iso")
if (( ${#images[@]} != 1 )); then
    echo "Expected one finished $iso in $output/tmp" >&2
    exit 1
fi
mkdir "$output/iso"
mv "${images[0]}" "$output/iso/$iso"
python3 "$here/verify-media.py" "$arch" "$output/iso/$iso" > "$output/boot-layout.txt"
(cd "$output/iso" && sha256sum "$iso" > "$iso.sha256")
printf '%s\n' "$output/iso/$iso"
