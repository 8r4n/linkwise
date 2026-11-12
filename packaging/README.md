# Build Linkwise modem preview installers

Build on the target OS and architecture with Python 3.9 or newer and the
repository's Rust toolchain. The packager compiles the locked release build
with native audio support and bundles the installation and station guides.
Run these commands from the repository root.

| Build host | Prerequisites | Command |
| --- | --- | --- |
| macOS | Xcode Command Line Tools | `python3 tools/package-modem.py pkg` |
| Windows 11 | Rust MSVC toolchain, Visual Studio C++ build tools for the target architecture, Inno Setup 6.3 or newer | `py -3 tools/package-modem.py exe --iscc "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"` |
| Ubuntu | `build-essential libasound2-dev pkg-config dpkg-dev` | `python3 tools/package-modem.py deb` |
| Fedora | `gcc alsa-lib-devel pkgconf-pkg-config rpm-build` | `python3 tools/package-modem.py rpm` |

The default output is `dist/`. Each installer has a matching SHA-256 file.
Build the Linux packages on the oldest distro release you intend to support
and test on each supported release. Use native ARM64 or x64 build tools
for the corresponding package.

The macOS packager accepts `--macos-installer-identity` to sign the outer
package. Public macOS distribution also needs the appropriate executable
signing and notarization. Sign public Windows installers with Authenticode.
Regenerate the checksum after any signing or notarization step that changes
the file. The Fedora package uses `LicenseRef-Unspecified` until the project
specifies its distribution terms.

## Build all targets in GitHub Actions

The [Modem preview packages workflow](../.github/workflows/modem-packages.yml)
builds and tests ARM64 and x64 installers for macOS, Windows 11, Ubuntu 24.04,
and Fedora 45. Fedora builds run in a Fedora container on the matching runner
architecture. Each passing job uploads an installer and its checksum as a
workflow artifact. The workflow runs for relevant pull requests and supports
manual dispatch.

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
