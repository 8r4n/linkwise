#!/usr/bin/env bash
set -euo pipefail

ISO="$HOME/Downloads/utm-share/Fedora-Everything-netinst-x86_64-43-1.6.iso"
VM_DIR="$HOME/Downloads/utm-share/fedora-qemu"
DISK="$VM_DIR/fedora-workstation-43.qcow2"
REPO_URL="https://download.fedoraproject.org/pub/fedora/linux/releases/43/Everything/x86_64/os/"
EXPECTED_SHA256="f4d06a40ce4fb4a84705e1a1f01ca1328f95a2120d422ba5f719af5df62d0099"
KS_PORT=8765
ACTION="${1:-install}"

usage() {
    cat <<'EOF'
Usage: tools/fedora-qemu-install.sh [install|boot]

install  Create a fresh 64 GiB qcow2 disk and run Fedora Workstation's
         unattended Kickstart install. The existing UTM Linux VM is untouched.
boot     Start the installed Fedora VM from its qcow2 disk.
EOF
}

if [[ "$ACTION" == "--help" || "$ACTION" == "-h" ]]; then
    usage
    exit 0
fi

if [[ "$ACTION" != "install" && "$ACTION" != "boot" ]]; then
    usage >&2
    exit 2
fi

QEMU="$(command -v qemu-system-x86_64 || true)"
QEMU_IMG="$(command -v qemu-img || true)"

if [[ -z "$QEMU" || -z "$QEMU_IMG" ]]; then
    echo "qemu-system-x86_64 and qemu-img must be installed and on PATH." >&2
    exit 1
fi

mkdir -p "$VM_DIR"

if [[ "$ACTION" == "boot" ]]; then
    if [[ ! -f "$DISK" ]]; then
        echo "Fedora disk not found: $DISK" >&2
        exit 1
    fi

    exec "$QEMU" \
        -name Fedora-Workstation-43 \
        -machine q35 \
        -accel tcg,thread=multi \
        -cpu max \
        -smp 2 \
        -m 4096 \
        -drive "file=$DISK,if=virtio,format=qcow2,discard=unmap" \
        -nic user,model=e1000 \
        -display cocoa \
        -serial none
fi

for tool in bsdtar curl python3 shasum; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Required tool not found: $tool" >&2
        exit 1
    fi
done

if [[ ! -f "$ISO" ]]; then
    echo "Fedora netinst ISO not found: $ISO" >&2
    exit 1
fi

if [[ -e "$DISK" ]]; then
    echo "Refusing to overwrite existing VM disk: $DISK" >&2
    echo "Use 'boot' to start it, or move it aside explicitly before reinstalling." >&2
    exit 1
fi

actual_sha256="$(shasum -a 256 "$ISO" | awk '{print $1}')"
if [[ "$actual_sha256" != "$EXPECTED_SHA256" ]]; then
    echo "Fedora ISO SHA-256 mismatch; refusing to boot it." >&2
    echo "Expected: $EXPECTED_SHA256" >&2
    echo "Actual:   $actual_sha256" >&2
    exit 1
fi

qemu_features="$("$QEMU" -accel help 2>&1 || true)"
if ! grep -q 'tcg' <<<"$qemu_features"; then
    echo "QEMU does not report TCG support, which this x86_64 VM requires." >&2
    exit 1
fi

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/fedora-kickstart.XXXXXX")"
SERVER_PID=""
cleanup() {
    if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -f "$WORK_DIR/vmlinuz" "$WORK_DIR/initrd.img" \
        "$WORK_DIR/http/ks.cfg"
    rmdir "$WORK_DIR/http" "$WORK_DIR" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

mkdir "$WORK_DIR/http"
cat >"$WORK_DIR/http/ks.cfg" <<KICKSTART
#version=F43
text
lang en_US.UTF-8
keyboard --xlayouts='us'
timezone Etc/UTC --utc
network --bootproto=dhcp --device=link --activate --onboot=yes --hostname=vara-fedora-qemu
url --url=$REPO_URL
rootpw --lock
selinux --enforcing
firewall --enabled --service=ssh
ignoredisk --only-use=vda
zerombr
clearpart --all --initlabel --drives=vda
autopart --type=lvm
bootloader --timeout=0 --location=mbr
firstboot --enable

%packages
@^workstation-product-environment
%end

shutdown --poweroff
KICKSTART

bsdtar -xOf "$ISO" images/pxeboot/vmlinuz >"$WORK_DIR/vmlinuz"
bsdtar -xOf "$ISO" images/pxeboot/initrd.img >"$WORK_DIR/initrd.img"

if [[ ! -s "$WORK_DIR/vmlinuz" || ! -s "$WORK_DIR/initrd.img" ]]; then
    echo "Could not extract Fedora's installer kernel and initrd." >&2
    exit 1
fi

python3 -m http.server "$KS_PORT" \
    --bind 0.0.0.0 \
    --directory "$WORK_DIR/http" >/dev/null 2>&1 &
SERVER_PID=$!

for attempt in {1..30}; do
    if curl --silent --fail "http://127.0.0.1:$KS_PORT/ks.cfg" >/dev/null; then
        break
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "Could not start the temporary Kickstart server on port $KS_PORT." >&2
        exit 1
    fi
    sleep 1
done

if ! curl --silent --fail "http://127.0.0.1:$KS_PORT/ks.cfg" >/dev/null; then
    echo "Temporary Kickstart server did not become ready." >&2
    exit 1
fi

"$QEMU_IMG" create -f qcow2 "$DISK" 64G

echo "Starting unattended Fedora Workstation installation."
echo "The VM disk is new; the existing UTM Linux VM will not be modified."
echo "Installation uses QEMU TCG and may take a while."

"$QEMU" \
    -name Fedora-Workstation-43-Install \
    -machine q35 \
    -accel tcg,thread=multi \
    -cpu max \
    -smp 2 \
    -m 4096 \
    -drive "file=$DISK,if=virtio,format=qcow2,discard=unmap" \
    -drive "file=$ISO,media=cdrom,readonly=on" \
    -nic user,model=e1000 \
    -kernel "$WORK_DIR/vmlinuz" \
    -initrd "$WORK_DIR/initrd.img" \
    -append "inst.ks=http://10.0.2.2:$KS_PORT/ks.cfg inst.repo=$REPO_URL ip=dhcp inst.text" \
    -display cocoa \
    -serial none
