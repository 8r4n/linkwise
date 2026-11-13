#!/usr/bin/env python3
"""Install, exercise, and remove a Linkwise package on a disposable test host."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tempfile

PACKAGE = "linkwise-modem"
INSTALLER_ID = "io.github.8r4n.linkwise.modem"
GUIDES = ("Install.md", "Station-setup.md")


def run(*args, capture=False):
    return subprocess.run(
        args, check=True, text=True, stdout=subprocess.PIPE if capture else None
    ).stdout


def registered(*args):
    return subprocess.run(
        args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False
    ).returncode == 0


def verify_checksum(package):
    sidecar = package.with_name(package.name + ".sha256")
    expected = sidecar.read_text(encoding="ascii").strip()
    digest = hashlib.sha256(package.read_bytes()).hexdigest()
    if expected != f"{digest}  {package.name}":
        raise RuntimeError(f"SHA-256 mismatch for {package}")


def smoke(package, version, station):
    system = platform.system()
    suffix = package.suffix
    if system == "Darwin" and suffix == ".pkg":
        binaries = Path("/usr/local/bin")
        docs = Path("/usr/local/share/doc") / PACKAGE
        query = ("pkgutil", "--pkg-info", INSTALLER_ID)
        install = ("sudo", "-n", "installer", "-pkg", package, "-target", "/")
        uninstall = ("sudo", "-n", "pkgutil", "--forget", INSTALLER_ID)
    elif system == "Linux" and suffix == ".deb":
        binaries = Path("/usr/bin")
        docs = Path("/usr/share/doc") / PACKAGE
        query = ("dpkg-query", "-W", PACKAGE)
        install = ("sudo", "-n", "apt-get", "-y", "install", package)
        # A package with maintainer scripts can remain in dpkg's database
        # after removal. Purge the test install to clear its registration.
        uninstall = ("sudo", "-n", "apt-get", "-y", "purge", PACKAGE)
    elif system == "Linux" and suffix == ".rpm":
        binaries = Path("/usr/bin")
        docs = Path("/usr/share/doc") / PACKAGE
        query = ("rpm", "-q", PACKAGE)
        # Fedora container images may set tsflags=nodocs. The smoke test
        # needs the packaged %doc files to exercise the full installation.
        install = ("sudo", "-n", "dnf", "-y", "--setopt=tsflags=", "install", package)
        uninstall = ("sudo", "-n", "dnf", "-y", "remove", PACKAGE)
    elif system == "Windows" and suffix == ".exe":
        binaries = Path(os.environ["ProgramFiles"]) / "Linkwise Modem"
        docs = binaries / "docs"
        query = (
            "reg.exe", "query",
            r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall"
            + f"\\{INSTALLER_ID}_is1", "/reg:64",
        )
        flags = ("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART")
        install = (package, *flags, f"/DIR={binaries}")
        uninstall = (binaries / "unins000.exe", *flags)
    else:
        raise RuntimeError(f"{suffix} packaging is unsupported on {system}")

    extension = ".exe" if system == "Windows" else ""
    cli = binaries / f"linkwise{extension}"
    modem = binaries / f"linkwise-modemd{extension}"
    guides = tuple(docs / name for name in GUIDES)
    installed_files = (cli, modem, *guides)
    if system == "Windows":
        installed_files += (binaries / "Configure-Winlink.ps1", docs / "Winlink-Quick-Start.md")
    else:
        man_root = binaries.parent / "share/man"
        man_pages = (man_root / "man1/linkwise-modemd.1", man_root / "man5/linkwise-station.5")
        if system == "Linux":
            man_pages = tuple(page.with_suffix(page.suffix + ".gz") for page in man_pages)
        installed_files += man_pages
    mac_app = Path("/Applications/Linkwise Station.app")
    if system == "Darwin" and mac_app.exists():
        raise RuntimeError("Use a clean test host without the Linkwise Station app")
    if (
        any(path.exists() or path.is_symlink() for path in installed_files)
        or (system == "Windows" and binaries.exists())
        or registered(*query)
    ):
        raise RuntimeError("Use a clean test host without an existing Linkwise installation")

    try:
        run(*install)
        if not registered(*query):
            raise RuntimeError("The package manager did not register the installation")
        if not all(path.is_file() for path in installed_files):
            raise RuntimeError("The installed binaries or guides are missing")
        if run(modem, "--version", capture=True).strip() != f"linkwise-modemd {version}":
            raise RuntimeError("The installed modem version does not match the package")
        run(modem, "--help")
        run(modem, "--check-config", station)
        run(modem, "--simulate")
        if system == "Windows":
            run("sc.exe", "qc", "LinkwiseModem")
            run("powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                Path(__file__).with_name("test-windows-station.ps1"))
    finally:
        if system == "Darwin":
            run("sudo", "-n", "rm", "-f", *installed_files)
            run("sudo", "-n", "rm", "-rf", mac_app)
        if registered(*query) or (system == "Windows" and uninstall[0].is_file()):
            run(*uninstall)

    if system == "Windows" and registered("sc.exe", "query", "LinkwiseModem"):
        raise RuntimeError("Uninstall left the Windows modem service registered")
    if any(path.exists() for path in installed_files) or registered(*query):
        raise RuntimeError("Uninstall left the package or its files behind")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    args = parser.parse_args()
    package = args.package.resolve()
    version = re.fullmatch(r"linkwise-modem[-_](\d+\.\d+\.\d+)[-_].+", package.name)
    if not version:
        parser.error(f"unrecognized Linkwise package filename: {package.name}")
    verify_checksum(package)
    configuration = json.dumps(
        {
            "version": 1,
            "callsign": "W1AW",
            "input": {"name": "Test input", "channel": 0},
            "output": {"name": "Test output", "channel": 0},
        }
    )
    with tempfile.TemporaryDirectory(prefix="linkwise-station-") as directory:
        station = Path(directory) / "station.json"
        station.write_text(configuration, encoding="utf-8")
        smoke(package, version[1], station)
        if station.read_text(encoding="utf-8") != configuration:
            raise RuntimeError("Uninstall modified the station configuration")
    print(f"Package smoke test passed: {package.name}")


if __name__ == "__main__":
    main()
