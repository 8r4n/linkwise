# Linkwise product test VMs in UTM

Use these ARM64 guests on an Apple silicon Mac to test native Linkwise builds.
Use Windows for installer and Winlink Express testing, Ubuntu for Debian-family
package and ALSA testing, and Fedora for RPM and ALSA testing.

## Prepare the guests

Install UTM 5, Python 3, and QEMU's `qemu-img`. If you use Homebrew,
`brew install qemu` supplies `qemu-img`. Run the setup tool from the repository
root:

```sh
python3 tools/setup-utm-test-vms.py all
python3 tools/setup-utm-test-vms.py ubuntu --start
```

Use `windows` or `fedora` in place of `ubuntu` to start a different guest.
Keep the Mac unlocked when starting a guest. If macOS asks whether your
terminal may control UTM, you can also open the generated `.utm` bundle and
start it directly in UTM.

The tool stores bundles, verified installation media, and Linux SSH keys
under `~/Virtual Machines`. It reuses existing bundles. To prepare a separate
set, pass `--root` and stop any guests already using the same SSH ports before
starting the new ones. Use `--utm-app` and `--qemu-img` for nonstandard
installation paths. The guest disks grow as needed. Allow enough free space
for Windows installation and updates as well as the downloaded images.

| Guest | RAM | Virtual disk | Login |
| --- | ---: | ---: | --- |
| Windows 11 Pro 25H2 ARM64 | 6 GiB | 80 GiB | UTM console |
| Ubuntu Server 24.04 LTS ARM64 | 4 GiB | 40 GiB | `ubuntu` at `127.0.0.1:2222` |
| Fedora Cloud 45 ARM64 | 4 GiB | 40 GiB | `fedora` at `127.0.0.1:2223` |

The tool pins the Windows ISO and UTM Guest Tools by SHA-256. Ubuntu and
Fedora hashes come from the publishers' release checksums. When refreshing
an image, update its URL and hash together and verify first boot again.

## Finish Windows setup

1. Start the Windows guest. Press a key when prompted to boot from CD or DVD.
2. Select Windows 11 Pro, review the license terms, and install onto the
   single unallocated 80 GB virtual disk.
3. Let the guest reboot from its disk. Complete the account and privacy
   screens. Use a Windows license appropriate for the test environment.
4. If Guest Tools did not install automatically, run the guest-tools
   installer from the attached tools disc and reboot.
5. Confirm the ARM64 system type in **Settings > System > About**. Use a
   clean snapshot for installer, upgrade, and uninstall testing.

This guest can exercise native ARM64 Linkwise packages and applications
supported by Windows on Arm. Qualify native Windows x86_64 separately.

## Connect to Linux

On first boot, cloud-init installs Rust, build tooling, and ALSA development
dependencies. Fedora also gets `rpm-build`. The SSH forwards listen on the
Mac's loopback interface. Keep the generated private keys local.

```sh
ssh -i "$HOME/Virtual Machines/Linkwise Access/ubuntu_ed25519" -p 2222 ubuntu@127.0.0.1
ssh -i "$HOME/Virtual Machines/Linkwise Access/fedora_ed25519" -p 2223 fedora@127.0.0.1
```

In the guest, wait for provisioning and verify the environment:

```sh
cloud-init status --wait
uname -m
. "$HOME/.cargo/env"
cargo --version
pkg-config --modversion alsa
```

After copying the Linkwise source into the guest, run the modem tests from
the checkout:

```sh
cd rust
cargo test --locked --features audio-device --test modem_runtime
cargo run --locked --features audio-device --bin linkwise-modemd -- --audio-devices
```

The script enables the ALSA loopback module for a device-free 48 kHz station
check. Verify it is loaded with `aplay -l` before running the check.

On Ubuntu, use `hw:CARD=Loopback,DEV=1` for input and
`hw:CARD=Loopback,DEV=0` for output. On Fedora, use
`sysdefault:CARD=Loopback` for both. Set both channels to `0` in your
[station configuration](../../rust/docs/modem-setup.md). Run
`linkwise-modemd --check-station station.json`. This exercises native capture,
format validation, and port binding inside the VM. Radio hardware, PTT, and
on-air Winlink exchange need separate tests.

## Verify playback through a native loopback

The ignored Rust integration test plays a BW500 request on an explicitly
named virtual output and decodes the samples from its paired input. Use
the ALSA loopback in the Linux guests. Keep radio interfaces out of the
test route.

Create a temporary ALSA configuration with the following contents. The
two devices use opposite ends of subdevice 0 at 48 kHz.

```text
</usr/share/alsa/alsa.conf>
pcm.linkwise_capture {
    type plug
    slave { pcm "hw:Loopback,1,0"; format S16_LE; rate 48000; channels 2; }
    hint { show on; description "Linkwise isolated capture"; }
}
pcm.linkwise_playback {
    type plug
    slave { pcm "hw:Loopback,0,0"; format S16_LE; rate 48000; channels 2; }
    hint { show on; description "Linkwise isolated playback"; }
}
```

From `rust/`, replace the ALSA configuration path with the file you created.

```sh
ALSA_CONFIG_PATH=/path/to/loopback.conf \
LINKWISE_LOOPBACK_INPUT=linkwise_capture \
LINKWISE_LOOPBACK_OUTPUT=linkwise_playback \
cargo test --locked --features audio-device --test native_audio_loopback -- --ignored
```
