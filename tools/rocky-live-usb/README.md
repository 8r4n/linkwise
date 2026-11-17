# Linkwise Rocky 9 live media

The demo ISO boots to XFCE. The hardened USB starts at a console login.
Both include the offline BW500 chat and email demo.
The design follows the boot and compatibility-check workflow documented by
[Red Hat Remote Exams](https://access.redhat.com/community/learn/remote-exams).
The image uses [Rocky's XFCE Kickstart](https://github.com/rocky-linux/kickstarts)
and [Lorax live media composition](https://weldr.io/lorax/livemedia-creator.html).
The ISO's UEFI El Torito entry and its GPT EFI System Partition refer to
the same FAT filesystem, so the same bytes can boot as virtual optical
media or as a virtual USB disk. The live root is a direct read-only SquashFS
with a temporary writable overlay.

## Build in UTM

Create a disposable UEFI Rocky Linux 9 QEMU guest with enough root and
scratch space for Anaconda, the desktop image, and the ISO. A 24 GiB sparse
scratch disk is a starting allocation. Install the compiler, Rust toolchain,
ALSA development libraries and RPM tools for the Linkwise build. Install
`lorax-lmc-novirt`, `lorax-templates-rocky`, `xorriso`, and `pykickstart` for the
image. Attach only the builder's virtual disks while composing. Lorax runs
Anaconda in the guest and needs root access.

On Apple silicon, use an ARM64 guest for the first build and a separate
x86-64 emulated guest for the PC image. The RPM and ISO must target the same
architecture. The [existing Rocky UTM setup](../rocky9-vm/README.md) documents
the UEFI and serial-console configuration.

From a checkout in the builder VM:

```sh
python3 tools/package-modem.py rpm --output-dir /mnt/scratch/rpm
python3 tools/smoke-test-package.py /mnt/scratch/rpm/linkwise-modem-0.1.0-1.aarch64.rpm
profile=$(python3 tools/fetch-modem-profile.py)
sudo tools/rocky-live-usb/compose.sh /path/to/signed.rpm /path/to/public.asc \
  "$profile" /mnt/scratch/linkwise-live
```

Sign the RPM with the intended build key before composing. Use a disposable
test key for local qualification. Keep private keys outside the image.
The script verifies the RPM signature against the supplied public key
and checks the BW500 profile hash before the build. The recipe emits the
rendered Kickstart, exact package inventory, ISO, SHA-256, boot layout
report, and build logs. It verifies the EFI System Partition, the optical
boot catalog, and the expected live filesystem and EFI loader files.
Rocky and EPEL repositories can change between builds. Retain their snapshots
if dependency reproducibility is required.

The profile comes from an external measurement source. Review its
distribution terms and attribution before publishing an image that embeds it.
Use the generated image for private testing while distribution is unresolved.

## Boot qualification

Copy the ISO and checksum back to macOS and verify the digest. Create a
separate UTM guest with no network or installed system disk. The helper
attaches the ISO bytes as a read-only USB mass-storage drive:

```sh
python3 tools/rocky-live-usb/utm-smoke-vm.py /path/to/Linkwise-Rocky9-Live-aarch64.iso \
  "$HOME/Virtual Machines/Linkwise Live Smoke.utm"
```

Open the bundle in UTM. On XFCE login, **Linkwise Station Check** lists
audio devices and USB serial adapters and reports serial access permissions.
Reopen it from the application menu after attaching hardware. To validate a
configuration's 48 kHz capture, output format, and local ports, run
`python3 /usr/share/linkwise-live/station-check.py /path/to/station.json`.
The check sends no audio or PTT commands. Launch **Linkwise BW500 Offline
Demo** to exercise chat and mail.
For an x86-64 ISO, pass `--architecture x86_64` to the helper.
For unattended smoke testing, append `linkwise.live-smoke=1` to the live
kernel command line, then monitor the serial console or the
`linkwise-live-smoke.service` journal. A pass requires both chat directions,
B2F shutdown at both peers, and the expected attachment SHA-256.

For a USB-detach test, select the **Start … from RAM** entry in the UEFI
menu. That entry uses dracut's `rd.live.ram` and needs enough memory for
the compressed system and the running desktop. Use `--memory-mib 12288` for
the UTM test guest. Rocky's early boot limits the `/run` tmpfs to 20% of RAM,
and the entire SquashFS image has to fit in that space. Wait for the desktop,
detach the virtual USB mass-storage device in the QEMU monitor, then run the
offline demo again.

The demo uses virtual audio and loopback sockets. Modem autostart is disabled.
The media masks UDisks and SSH. To check host-disk isolation, attach a second
test disk and confirm it stays unmounted. The desktop has no live installer
entry. To inspect native devices, run `linkwise-modemd --audio-devices` and
follow the bundled modem setup guide. That check does not qualify RF or PTT.

For physical PC qualification, write the x86-64 ISO to an identified spare
USB and boot it from the PC's one-time UEFI boot menu. Repeat the offline
demo and verify that the internal disks stay unmounted. Record the PC model,
firmware, Secure Boot state, audio interface, ISO hash, and test results.
USB writing erases the selected device.

The demo uses the standard modem RPM and a temporary local mailbox.

## Hardened UEFI USB image

The hardened build emits a writable GPT disk image for a single operator.
The EFI partition contains the Rocky bootloader, kernel, kernel HMAC, and
initramfs. An independently provisioned LUKS2 volume contains the read-only
SquashFS and the persistent home and log state. The live overlay and temporary
directories use RAM. Swap is disabled.

Build and sign the full Rocky RPM before composing the hardened USB.
Use `python3 tools/package-modem.py rpm --full` with CMake, Go, the C
compiler, and Rocky's OpenSSL development package available. This RPM
includes native audio, chat, file transfer, station PKI, protected mail,
QSL cards, TLS for the local mail server, and the loopback modem demo.
The image starts mail and modem functions when the operator configures
and launches them.

Hybrid post-quantum chat is compiled in. It also requires the exact
OpenSSL FIPS module accepted by the
[PKI provider check](../../rust/docs/pki.md#require-hybrid-post-quantum-chat).
The Rocky system FIPS provider and the upstream CMVP #5586 module have
separate validation records. Use the upstream provider and its approved
configuration on a qualified operational environment to enable the hybrid
mode. Linkwise refuses the connection if the configured provider fails
its checks.

On the disposable Rocky builder, add `--root-only` to the compose command
to produce a console-only `live-root.img` from Rocky Minimal. Install
`cryptsetup`, `dosfstools`, `gdisk`,
`grub2-tools`, `squashfs-tools`, `usbguard`, `fapolicyd`, and `gcc` on the builder.
USBGuard and fapolicyd supply SELinux labels needed during composition.
Provision separate
operator and LUKS passphrases in mode-0600 private files. The LUKS file
contains the exact passphrase bytes without a newline. The operator file
contains one line and provisions the `lwadmin` console and GRUB account.

```sh
sudo mkdir /mnt/scratch/live-root
sudo mount -o loop /mnt/scratch/linkwise-live/live-root.img /mnt/scratch/live-root
sudo bash tools/rocky-live-usb/harden-live-root.sh /mnt/scratch/live-root /private/operator
sudo bash tools/rocky-live-usb/compose-hardened-usb.sh /mnt/scratch/live-root \
    /private/unlock /private/operator /mnt/scratch/linkwise-hardened.raw
sudo umount /mnt/scratch/live-root
```

Attach a private copy of the raw disk as virtual USB using
`utm-smoke-vm.py --writable`. UTM writes to that image. Unlock LUKS from the
guest console and log in as `lwadmin`. Initialize the AIDE baseline once on
the freshly built image, then run the verification:

```sh
sudo aide --init
sudo install -m 0600 /var/lib/aide/aide.db.new.gz /var/lib/aide/aide.db.gz
sudo restorecon /var/lib/aide/aide.db.gz
sudo bash /usr/share/linkwise-live/live-controls.sh
```

The script checks the
live crypto policy, FIPS provider, SELinux enforcement, LUKS2 header and
storage ancestry, and audit and device controls. It writes the full Rocky
STIG XML, HTML, and scanner logs under `/var/lib/linkwise-live-assessment`
on the encrypted disk.

Run `linkwise chat --help` or `linkwise mail --help` from the console.
The station check and offline demo also run directly:

```sh
python3 /usr/share/linkwise-live/station-check.py
python3 /usr/share/linkwise-live/demo.py
```

### Read and compose email with Mutt

Run `linkwise-mutt YOURCALL` on the hardened console. Use `m` to compose
and Enter to open a message. Address peer mail to `PEERCALL@winlink.org`
and use one recipient per message. Mutt submits outgoing mail to the
local Linkwise SMTP queue and reads received messages through Linkwise
IMAP. Both services run on loopback while Mutt is open.

Run the B2F radio exchange from another console with the same store.
Use `listen` at the receiving station and `exchange` at the calling
station. Mutt's send action queues the message for a later B2F exchange.
For a plain-text file attachment, use `Ctrl-T` on the attachment in
Mutt's compose screen and set its MIME type to `application/octet-stream`
for the Linkwise B2F importer.

```sh
linkwise mail listen "$HOME/.local/share/linkwise/mail" YOURCALL PEERCALL
# Or, at the station initiating the radio session:
linkwise mail exchange "$HOME/.local/share/linkwise/mail" YOURCALL PEERCALL
```

The CLI commands use the modem's default command and data ports. Supply
both ports at the end of the command if your station uses different ones.
Use `linkwise-mutt --store PATH YOURCALL` to select another mailbox store
and pass the same path to `linkwise mail`.

For signed and encrypted mail, run
`linkwise-mutt --pki station-pki.json YOURCALL` with a provisioned
[station profile](../../rust/docs/pki.md). Press `m` to draft and `y` to
send. Mutt sends the draft directly to Linkwise, which signs and encrypts
it before queuing. If the recipient certificate is missing, revoked, or
untrusted, Mutt reports the failure and keeps the draft available for
correction. Mutt's inbox shows the protected envelope. Use
`linkwise mail export --pki station-pki.json --encrypt MAILBOX MESSAGE_ID`
to decrypt and verify received mail.

The live build reuses the VM's sysctl, sudo, PAM, password, and audit rules.
USBGuard blocks devices attached after startup. The USB storage driver is
enabled for the boot disk. The encrypted filesystem supplies bind mounts
for the operator and logs. These share the LUKS2 volume, so the original
VM's separate LVM volumes and audit-space boundary need separate disposition.
Use the scan verdicts and account for live-media differences against the
[original Rocky assessment](../../docs/architecture/niap-rocky9-assessment.md).

### Boot and radio performance

The hardened USB uses a one-second GRUB menu. Press a key during that
interval to stop the countdown. Editing still requires the GRUB operator
password. The initramfs only needs local storage to unlock and mount the
live filesystem, so it excludes network boot components. The console
image starts NetworkManager after switching root and allows local login
while the network configures. Its immutable OS is updated by replacing
the image, so the package metadata refresh timer is disabled. The console
image also disables SSSD, NIS domain setup, standalone DNS trust-anchor
refresh, systemd-boot updates, and automatic kernel crash-dump handling.

The modem and any installed Linkwise IMAP and SMTP services share
`linkwise.slice`. On cgroup v2, it gives them a CPU weight of 1000 and an
I/O weight of 800. Console chat and mail inherit the operator's
`user.slice`, with weights of 500 and 400. These weights apply during
contention. Actual I/O prioritization depends on the storage scheduler.
Start the modem after configuring the station.

For comparable boot measurements, time from a cold VM start to the LUKS
prompt and from passphrase submission to the console login prompt. Record
`systemd-analyze time` and `systemd-analyze critical-chain multi-user.target`
after login. User input time and station-specific audio setup are separate
from those intervals.

### Memory protection

The x86-64 boot entry requests AMD SME with `mem_encrypt=on`. Enable
SME or Intel TME in firmware where the system exposes it. The control
check records the kernel's activation message when present. Consult the
[Linux AMD memory encryption documentation](https://docs.kernel.org/arch/x86/amd-memory-encryption.html)
for the distinction between advertised capability and active SME.

Normal poweroff and reboot use the dracut memory-overwrite hook, with the
USB contents preserved. After dracut unmounts the old root, the hook
releases the live image's remaining mounts and closes the boot LUKS mapping.
It overwrites reclaimable pages with bytes from Linux
`getrandom`, and logs the byte count on the console. It leaves 128 MiB
available for shutdown and caps the overwrite at 60 seconds. Abrupt
power loss bypasses the hook, and Linux keeps kernel and firmware
reserved memory beyond its reach.

`sudo linkwise-shred --destroy-usb` irreversibly erases the running USB's
LUKS keyslots, public EFI partition, and partition table during orderly
shutdown, before the memory overwrite. It verifies the USB transport and
both filesystem identities before erasing. Separate header backups or
copies of the USB remain usable. Flash wear leveling limits claims about
physical cell erasure.
