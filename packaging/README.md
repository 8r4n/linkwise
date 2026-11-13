# Build Linkwise modem preview installers

Build on the target OS and architecture with Python 3.9 or newer and the
repository's Rust toolchain. The packager compiles the locked release build
with native audio support and bundles the installation and station guides.
Run these commands from the repository root.

| Build host | Prerequisites | Command |
| --- | --- | --- |
| macOS | Xcode Command Line Tools | `python3 tools/package-modem.py pkg` |
| Windows 11 | Rust MSVC toolchain, Visual Studio C++ build tools for the target architecture, Inno Setup 6.3 or newer | `py -3 tools/package-modem.py exe --iscc "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"` |
| Ubuntu | `build-essential libasound2-dev pkg-config dpkg-dev debhelper` | `python3 tools/package-modem.py deb` |
| Fedora | `gcc alsa-lib-devel pkgconf-pkg-config rpm-build make selinux-policy-devel systemd-rpm-macros` | `python3 tools/package-modem.py rpm` |

The default output is `dist/`. Each installer has a matching SHA-256 file.
Build the Linux packages on the oldest distro release you intend to support
and test on each supported release. Use native ARM64 or x64 build tools
for the corresponding package.

The macOS packager accepts `--macos-application-identity` and
`--macos-installer-identity` for Developer ID signing. Submit signed packages
by passing `--macos-notary-profile`. The packager waits for notarization and
validates the stapled ticket. On Windows, use `--windows-signing-thumbprint`
with SignTool available to sign and verify both binaries and the installer.
Regenerate the checksum after any signing or notarization step that changes
the file. Packages include the Apache-2.0 license and dependency notices for the
locked target build.

## Build all targets in GitHub Actions

The [Modem preview packages workflow](../.github/workflows/modem-packages.yml)
builds and tests ARM64 and x64 installers for macOS, Windows 11, Ubuntu 24.04,
and Fedora 45. Fedora builds run in a Fedora container on the matching runner
architecture. Each passing job uploads an installer and its checksum as a
workflow artifact. The workflow runs on pushes to `main` and `package`,
version tags matching `v*`, and relevant pull requests. To build on demand,
open **Actions → Modem preview packages → Run workflow**. Download the
installer and its SHA-256 file from the completed run's **Artifacts** list.

## Test a package locally

On a clean matching test VM or disposable host, run the smoke test with the
installer and its checksum in the same directory. Unix tests require
passwordless administrator access. On Windows, run the test as the user who
will install the preview.

```text
python tools/smoke-test-package.py PATH_TO_INSTALLER
```

The test verifies the checksum, installs the package, checks both installed
guides, validates a temporary station configuration, runs the modem
simulation, and removes the package. It checks that the installed files are
removed and the station configuration survives. Test upgrades separately
with two distinct package versions before distributing them.


The package workflow gates builds on `cargo audit --file rust/Cargo.lock`.
This covers known Rust dependency advisories. Check the target OS and build
environment against their vendor advisories separately before release.

On a booted Linux test guest, configure the packaged system unit to use an
isolated ALSA loopback capture device, then run `systemctl start --no-block
linkwise-modem`. Use `tools/test-station-service.py` with a known 48 kHz
PCM BW500 DATA fixture and the matching playback device to verify a decoded
frame through the installed service. Never select a radio output for this test.
