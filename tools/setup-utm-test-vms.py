#!/usr/bin/env python3
"""Prepare Linkwise's ARM64 product test guests in UTM on Apple silicon."""

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import tempfile
import time
import uuid


@dataclass(frozen=True)
class Image:
    filename: str
    url: str
    sha256: str


WINDOWS = Image(
    "Windows11-25H2-ARM64.iso",
    "https://software-static.download.prss.microsoft.com/dbazure/"
    "888969d5-f34g-4e03-ac9d-1f9786c66749/"
    "26200.6584.250915-1905.25h2_ge_release_svc_refresh_CLIENT_CONSUMER_A64FRE_en-us.iso",
    "32cde0071ed8086b29bb6c8c3bf17ba9e3cdf43200537434a811a9b6cc2711a1",
)
GUEST_TOOLS = Image(
    "UTM-Guest-Tools.iso",
    "https://github.com/utmapp/qemu/releases/download/v10.0.2-utm/utm-guest-tools-0.1.271.iso",
    "65b6a69b392ee01dd314c10f3dad9ebbf9c4160be43f5f0dd6bb715944d9095b",
)
UBUNTU = Image(
    "Ubuntu-24.04-ARM64.qcow2",
    "https://cloud-images.ubuntu.com/noble/20261017/noble-server-cloudimg-arm64.img",
    "50e24e1fb79e89116e939d90aa7738f7af48c442fa1bc2510131e0b39c50348e",
)
FEDORA = Image(
    "Fedora-45-ARM64.qcow2",
    "https://download.fedoraproject.org/pub/fedora/linux/releases/45/Cloud/aarch64/images/"
    "Fedora-Cloud-Base-Generic-45-1.4.aarch64.qcow2",
    "1632acc8bc14d45b6c9596a0acc71b070d3748d1abdcf1a519be5c983bb0286c",
)


@dataclass(frozen=True)
class Guest:
    name: str
    image: Image
    memory: int
    disk_size: str
    login: str = ""
    ssh_port: int = 0
    packages: tuple = ()
    loopback_package: str = ""


GUESTS = {
    "windows": Guest("Linkwise Windows 11 ARM64", WINDOWS, 6144, "80G"),
    "ubuntu": Guest(
        "Linkwise Ubuntu 24.04 ARM64", UBUNTU, 4096, "40G", "ubuntu", 2222,
        ("alsa-utils", "build-essential", "curl", "git", "libasound2-dev",
         "pkg-config", "qemu-guest-agent"),
        'apt-get install -y "linux-modules-extra-$(uname -r)"',
    ),
    "fedora": Guest(
        "Linkwise Fedora 45 ARM64", FEDORA, 4096, "40G", "fedora", 2223,
        ("alsa-lib-devel", "alsa-utils", "curl", "gcc", "git", "make",
         "pkgconf-pkg-config", "qemu-guest-agent", "rpm-build"),
        'dnf install -y "kernel-modules-$(uname -r)"',
    ),
}


def run(*command):
    subprocess.run(command, check=True)


def verified_image(image, cache):
    target = cache / image.filename
    candidate = target
    if not target.exists():
        candidate = target.with_suffix(target.suffix + ".part")
        print(f"Downloading {image.filename}", flush=True)
        run("curl", "--fail", "--location", "--retry", "3", "--continue-at", "-",
            "--output", candidate, image.url)
    digest = hashlib.sha256()
    with candidate.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    if digest.hexdigest() != image.sha256:
        raise ValueError(f"Checksum mismatch for {candidate}. Move the file aside and retry.")
    if candidate != target:
        candidate.rename(target)
    return target


def cloud_seed(guest, access, output):
    key = access / f"{guest.login}_ed25519"
    public_key = key.with_suffix(".pub")
    if not key.exists() and not public_key.exists():
        run("ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", guest.name, "-f", key)
    expected_key = subprocess.check_output(("ssh-keygen", "-y", "-f", key), text=True)
    if public_key.read_text().split()[:2] != expected_key.split()[:2]:
        raise ValueError(f"Public key {public_key} does not match {key}")
    user_data = {
        "users": ["default"],
        "ssh_pwauth": False,
        "ssh_authorized_keys": [public_key.read_text().strip()],
        "package_update": True,
        "packages": guest.packages,
        "runcmd": [
            ["usermod", "-aG", "audio", guest.login],
            ["runuser", "-u", guest.login, "--", "bash", "-o", "pipefail", "-c",
             "curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path"],
            ["sh", "-ec",
             f"modinfo snd-aloop >/dev/null 2>&1 || {guest.loopback_package}\n"
             "modprobe snd-aloop\n"
             "echo snd-aloop > /etc/modules-load.d/linkwise-loopback.conf"],
        ],
    }
    with tempfile.TemporaryDirectory(prefix="linkwise-seed-") as directory:
        seed = Path(directory)
        (seed / "meta-data").write_text(
            f"instance-id: {uuid.uuid4()}\nlocal-hostname: linkwise-{guest.login}\n"
        )
        (seed / "user-data").write_text("#cloud-config\n" + json.dumps(user_data) + "\n")
        run("hdiutil", "makehybrid", "-quiet", "-iso", "-joliet",
            "-default-volume-name", "cidata", "-o", output, seed)


def drive(name, kind, interface, readonly):
    return dict(ImageName=name, ImageType=kind, Interface=interface,
                InterfaceVersion=1, ReadOnly=readonly, Identifier=str(uuid.uuid4()).upper())


def vm_config(guest, drives):
    windows = not guest.login
    forwards = []
    if guest.login:
        forwards.append(dict(Protocol="TCP", HostAddress="127.0.0.1",
                             HostPort=guest.ssh_port, GuestPort=22))
    mac = "52:54:00:" + ":".join(f"{byte:02x}" for byte in os.urandom(3))
    return {
        "Backend": "QEMU",
        "ConfigurationVersion": 4,
        "Information": dict(Name=guest.name, IconCustom=False, UUID=str(uuid.uuid4()).upper()),
        "System": dict(Architecture="aarch64", Target="virt", CPU="default",
                       CPUFlagsAdd=[], CPUFlagsRemove=[], CPUCount=4, ForceMulticore=False,
                       MemorySize=guest.memory, JITCacheSize=0),
        "QEMU": dict(DebugLog=False, UEFIBoot=True, RNGDevice=True, BalloonDevice=False,
                     TPMDevice=windows, Hypervisor=True, RTCLocalTime=windows,
                     PS2Controller=False, AdditionalArguments=[]),
        "Input": dict(UsbBusSupport="3.0", UsbSharing=True, MaximumUsbShare=3),
        "Sharing": dict(DirectoryShareMode="None", DirectoryShareReadOnly=True,
                        ClipboardSharing=windows),
        "Display": [dict(Hardware="virtio-ramfb" if windows else "virtio-gpu-pci",
                         DynamicResolution=False, UpscalingFilter="Linear",
                         DownscalingFilter="Linear", NativeResolution=False)],
        "Drive": drives,
        "Network": [dict(Mode="Emulated", Hardware="virtio-net-pci", MacAddress=mac,
                         IsolateFromHost=False, PortForward=forwards)],
        "Serial": [] if windows else [dict(Mode="Ptty", Target="Auto")],
        "Sound": [dict(Hardware="intel-hda")],
    }


def prepare(guest, root, app, qemu_img):
    bundle = root / f"{guest.name}.utm"
    if bundle.exists():
        if not (bundle / "config.plist").is_file():
            raise ValueError(f"Existing path is missing its UTM configuration: {bundle}")
        print(f"Using existing VM {bundle}", flush=True)
        return bundle

    cache = root / "Linkwise Media"
    cache.mkdir(exist_ok=True)
    image = verified_image(guest.image, cache)
    windows = not guest.login
    with tempfile.TemporaryDirectory(prefix=".linkwise-utm-", dir=root) as directory:
        staging = Path(directory) / bundle.name
        data = staging / "Data"
        data.mkdir(parents=True)
        firmware = "edk2-arm-secure-vars.fd" if windows else "edk2-arm-vars.fd"
        shutil.copyfile(app / "Contents/Resources/qemu" / firmware, data / "efi_vars.fd")
        if windows:
            tools = verified_image(GUEST_TOOLS, cache)
            for media in (image, tools):
                os.link(media, data / media.name)
            run(qemu_img, "create", "-f", "qcow2", data / "Windows.qcow2", guest.disk_size)
            drives = [drive(image.name, "CD", "USB", True),
                      drive("Windows.qcow2", "Disk", "NVMe", False),
                      drive(tools.name, "CD", "USB", True)]
        else:
            disk = data / guest.image.filename
            run("cp", "-c", image, disk)
            run(qemu_img, "resize", disk, guest.disk_size)
            run(qemu_img, "check", disk)
            access = root / "Linkwise Access"
            access.mkdir(mode=0o700, exist_ok=True)
            cloud_seed(guest, access, data / "cloud-init.iso")
            drives = [drive(disk.name, "Disk", "VirtIO", False),
                      drive("cloud-init.iso", "Disk", "VirtIO", True)]
        config = staging / "config.plist"
        config.write_bytes(plistlib.dumps(vm_config(guest, drives)))
        run("plutil", "-lint", config)
        staging.rename(bundle)
    print(f"Prepared {bundle}", flush=True)
    return bundle


def start_guest(app, bundle):
    identifier = plistlib.loads((bundle / "config.plist").read_bytes())["Information"]["UUID"]
    ctl = app / "Contents/MacOS/utmctl"
    run("open", "-a", app, bundle)
    for _ in range(10):
        registered = subprocess.check_output((ctl, "list"), text=True, timeout=10)
        if identifier.lower() in registered.lower():
            status = subprocess.check_output((ctl, "status", identifier), text=True, timeout=10)
            if status.strip() != "started":
                subprocess.run((ctl, "start", identifier), check=True, timeout=60)
            return
        time.sleep(1)
    raise ValueError(f"UTM did not register {bundle}. Open it in UTM and try again.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("guest", choices=(*GUESTS, "all"))
    parser.add_argument("--root", type=Path, default=Path.home() / "Virtual Machines")
    parser.add_argument("--utm-app", type=Path, help="path to UTM.app")
    parser.add_argument("--qemu-img", default="qemu-img", help="path to qemu-img")
    parser.add_argument("--start", action="store_true", help="open and start one VM in UTM")
    args = parser.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        parser.error("Run this on an Apple silicon Mac.")
    if args.start and args.guest == "all":
        parser.error("Select one guest with --start.")
    app = args.utm_app or next(
        (path for path in (Path.home() / "Applications/UTM.app", Path("/Applications/UTM.app"))
         if path.is_dir()), None,
    )
    if not app or not (app.expanduser() / "Contents/MacOS/utmctl").is_file():
        parser.error("Install UTM or provide --utm-app.")
    app = app.expanduser().resolve()
    root = args.root.expanduser().resolve()
    root.mkdir(parents=True, exist_ok=True)
    guests = GUESTS.values() if args.guest == "all" else [GUESTS[args.guest]]
    for guest in guests:
        if not (root / f"{guest.name}.utm").exists() and not shutil.which(args.qemu_img):
            parser.error("Install qemu-img or provide --qemu-img.")
        bundle = prepare(guest, root, app, args.qemu_img)
        if args.start:
            start_guest(app, bundle)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        raise SystemExit(f"UTM setup failed: {error}") from error
