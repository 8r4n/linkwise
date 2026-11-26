# Linkwise on Windows with Rocky Linux WSL 2

The development image bundles the unsigned Linkwise `1.rockymail` RPM into
the official Rocky Linux 9.9 WSL filesystem. It includes the Linux modem,
station manager, and mail service files. The image enables systemd and
leaves Linkwise services opt-in. [Rocky Linux WSL images](https://rockylinux.org/download)

## Choose and verify the image

In Windows, open **Settings → System → About** and match **System type** to
the image. Download the corresponding `linkwise-rocky9-wsl-x64` or
`linkwise-rocky9-wsl-arm64` build artifact and extract the image and checksum
into the same directory. [Rocky Linux WSL build workflow](../../.github/workflows/rocky-wsl.yml)

| Windows host | Rocky architecture | Image |
| --- | --- | --- |
| x64 / AMD64 | `x86_64` | `linkwise-rocky9-x64.wsl` |
| ARM64 | `aarch64` | `linkwise-rocky9-arm64.wsl` |

In PowerShell, set `$image` to the filename for your host and verify it
against the corresponding checksum supplied with the build.

```powershell
$image = 'linkwise-rocky9-x64.wsl'
$expected = ((Get-Content -LiteralPath "$image.sha256" -Raw).Trim() -split '\s+')[0]
$actual = (Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash
if ($actual -ine $expected) { throw 'Linkwise image checksum mismatch' }
```

## Install the distribution

On a machine without WSL, run `wsl --install --no-distribution` from an
administrator PowerShell window and restart if Windows requests it. Run
`wsl --version` to check the installed WSL release and `wsl --update` to
update it. Use WSL 2.4.4 or later for direct `.wsl` installation. [Microsoft WSL commands](https://learn.microsoft.com/en-us/windows/wsl/basic-commands) · [Microsoft custom distribution guide](https://learn.microsoft.com/en-us/windows/wsl/build-custom-distro)

From the directory with the verified image, run the following in PowerShell.
Use the distribution name listed by `wsl -l -v` if it differs from
`Linkwise-Rocky9`. The version column should show `2`. Complete Rocky's
username setup on first launch.

```powershell
wsl --install --from-file $image
wsl -l -v
wsl -d Linkwise-Rocky9
```

If the new distribution shows version `1`, run
`wsl --set-version Linkwise-Rocky9 2` before using Linkwise.
[Microsoft WSL commands](https://learn.microsoft.com/en-us/windows/wsl/basic-commands#set-wsl-version-to-1-or-2)

If your WSL supports WSL 2 and systemd but lacks `--from-file`, import the
same verified root filesystem as a tar file. Use an unused distribution name
and a new install directory. An imported distribution may initially open
as root. [Microsoft tar import and user setup](https://learn.microsoft.com/en-us/windows/wsl/use-custom-distro) · [Microsoft systemd requirements](https://learn.microsoft.com/en-us/windows/wsl/systemd)

```powershell
$tar = $image -replace '\.wsl$', '.tar'
if (Test-Path -LiteralPath $tar) { throw 'Choose a directory without an existing tar copy' }
Copy-Item -LiteralPath $image -Destination $tar
$destination = Join-Path $env:LOCALAPPDATA 'WSL\Linkwise-Rocky9'
New-Item -ItemType Directory -Path $destination
wsl --import Linkwise-Rocky9 $destination $tar --version 2
wsl -l -v
wsl -d Linkwise-Rocky9
```

## Check the installed image

Run these commands inside Rocky. Confirm the Rocky architecture matches
the selected image, PID 1 is `systemd`, and the installed RPM reports
release `1.rockymail`. `rpm -V` prints differences if the installed
package files fail verification. The modem simulation runs without a radio.

```sh
cat /etc/rocky-release
uname -m
cat /proc/1/comm
rpm -q --queryformat '%{NAME} %{VERSION}-%{RELEASE} %{ARCH}\n' linkwise-modem
rpm -V linkwise-modem
linkwise --version
linkwise-modemd --version
linkwise-modemd --simulate
systemctl list-unit-files 'linkwise*.service'
```

## Run Linkwise in WSL

For a radio-free managed BW500 trial, start `linkwise` through an MCP stdio
client with `LINKWISE_RADIO_BACKEND=simulated` and an installed station
profile. The two virtual modems use IPv4 loopback sockets within Rocky. For
Windows applications that connect to a Linux TCP listener, WSL provides
localhost forwarding according to the host networking configuration. Keep
the trial's modem ports on loopback.
[Station profiles and MCP tools](../../rust/docs/mcp-usage.md#station-inventory-and-shadow-operation) · [Microsoft WSL networking](https://learn.microsoft.com/en-us/windows/wsl/networking)

Use `linkwise-modemd --audio-devices` to inspect Linux capture and playback
options. Windows audio and USB or serial radio devices require separate WSL
configuration. Physical audio routing, CAT, PTT release, and transmit timing
require qualification on the actual station. [Microsoft USB support](https://learn.microsoft.com/en-us/windows/wsl/connect-usb) · [Linkwise station setup](../../rust/docs/modem-setup.md)

The packaged SMTP and IMAP units require a WSL kernel that supports FIPS
mode and reports `1` in `/proc/sys/crypto/fips_enabled`. They also require
Rocky's OpenSSL FIPS policy and provider. FIPS and production SELinux
qualification remain open for WSL. The Rocky server guide describes the
production target and mail prerequisites.
[Rocky Linux server guide](rocky-server.md#mail-access)

## Build the image locally

Use a native Linux x64 or ARM64 host with Docker. To use Podman, set
`LINKWISE_CONTAINER_RUNTIME=podman`. From the repository root, choose the
source revision and create separate build and image directories.

```sh
revision=$(git rev-parse HEAD)
evidence_dir=$(mktemp -d)
output_dir=$(mktemp -d)
bash tools/rocky9-vm/build-rpm.sh "$revision" "$evidence_dir" wsl
rpm=$(find "$evidence_dir/build-source/installer-artifacts" -type f -name '*-1.rockymail.*.rpm' -print -quit)
test -n "$rpm" && bash tools/rocky-wsl/build-image.sh "$rpm" "$output_dir"
```

Use the `.wsl` image and `.wsl.sha256` file generated in the output
directory. Retain the build revision and log with the artifacts.
