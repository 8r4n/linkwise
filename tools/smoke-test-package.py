#!/usr/bin/env python3
"""Install, exercise, and remove a Linkwise package on a disposable test host."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import subprocess
import sys
import tempfile

PACKAGE = "linkwise-modem"
INSTALLER_ID = "io.github.8r4n.linkwise.modem"
GUIDES = ("Install.md", "Station-setup.md", "LICENSE", "THIRD-PARTY-NOTICES.txt")
ROCKY_SERVICE_FILES = tuple(map(Path, (
    "/usr/lib/systemd/system/linkwise-modem.service",
    "/usr/lib/systemd/user/linkwise-modem.service",
    "/usr/lib/systemd/system/linkwise-mail-smtp.service",
    "/usr/lib/systemd/system/linkwise-mail-imap.service",
    "/usr/lib/systemd/system-preset/80-linkwise-modem.preset",
    "/usr/lib/systemd/user-preset/80-linkwise-modem.preset",
    "/usr/share/selinux/packages/linkwise_modem.pp",
    "/usr/share/selinux/packages/linkwise_modem_ports.cil",
)))
MANAGED_BW500_TEST = "managed_virtual_bw500_mcp_reaps_aborted_pair_and_verifies_both_transfers"


def run(*args, capture=False, env=None, timeout=None):
    return subprocess.run(
        args, check=True, text=True, stdout=subprocess.PIPE if capture else None,
        env=env, timeout=timeout,
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


def validate_rocky_bw500(package, profile):
    arch = platform.machine()
    if package.suffix != ".rpm" or platform.system() != "Linux" or arch not in {"aarch64", "x86_64"}:
        raise RuntimeError("--rocky-bw500-profile requires an ARM64 or x86-64 Rocky Linux 9 server RPM")
    release = {}
    for line in Path("/etc/os-release").read_text(encoding="utf-8").splitlines():
        key, separator, value = line.partition("=")
        if separator and key in {"ID", "VERSION_ID"}:
            release[key], = shlex.split(value)
    if release.get("ID") != "rocky" or release.get("VERSION_ID", "").split(".")[0] != "9":
        raise RuntimeError("--rocky-bw500-profile requires a Rocky Linux 9 test environment")
    metadata = run("rpm", "-qp", "--queryformat", "%{NAME}\t%{RELEASE}\t%{ARCH}", package, capture=True)
    if metadata != f"{PACKAGE}\t1.rockymail\t{arch}":
        raise RuntimeError(f"--rocky-bw500-profile requires linkwise-modem release 1.rockymail.{arch}")
    profile = profile.resolve(strict=True)
    if not profile.is_file():
        raise RuntimeError("The BW500 waveform profile must be a file")
    return profile


def smoke_rocky_bw500(cli, modem, profile):
    import grp
    import pwd

    owners = run("rpm", "-qf", "--queryformat", "%{NAME}\n",
                 cli, modem, *ROCKY_SERVICE_FILES, capture=True)
    if set(owners.splitlines()) != {PACKAGE}:
        raise RuntimeError("The Rocky executables, units, presets, and SELinux files must belong to linkwise-modem")
    run("rpm", "-V", "--noscripts", PACKAGE)
    account = pwd.getpwnam("linkwise-mail")
    group = grp.getgrnam("linkwise-mail")
    if (account.pw_uid == 0 or group.gr_gid == 0 or account.pw_gid != group.gr_gid
            or account.pw_dir != "/var/lib/linkwise-mail" or Path(account.pw_shell).name != "nologin"):
        raise RuntimeError("The RPM did not install the expected non-login mail service identity")

    try:
        report = run(
            "cargo", "test", "--locked", "--release", "--target", f"{platform.machine()}-unknown-linux-gnu",
            "--features", "audio-device,rocky-mail-tls",
            "--manifest-path", Path(__file__).resolve().parents[1] / "rust/Cargo.toml",
            "--test", "olm_station", MANAGED_BW500_TEST, "--", "--ignored", "--exact",
            capture=True, timeout=480,
            env=dict(os.environ, LINKWISE_BW500_PROFILE=str(profile),
                     LINKWISE_STATION_TEST_CLI=str(cli), LINKWISE_STATION_TEST_MODEMD=str(modem)),
        )
    except subprocess.CalledProcessError as error:
        print(error.stdout or "", end="")
        raise
    print(report, end="")
    if f"test {MANAGED_BW500_TEST} ... ok" not in report.splitlines():
        raise RuntimeError("The required managed BW500 MCP test did not run successfully")
    print("Installed Rocky RPM passed the radio-free managed BW500 MCP smoke test.")


def smoke(package, version, station, rocky_bw500_profile=None):
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
        man_pages = (man_root / "man1/linkwise.1", man_root / "man8/linkwise-modemd.8", man_root / "man5/linkwise-station.5")
        if system == "Linux":
            man_pages = tuple(page.with_suffix(page.suffix + ".gz") for page in man_pages)
        installed_files += man_pages
    if rocky_bw500_profile is not None:
        installed_files += ROCKY_SERVICE_FILES
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
        run(cli, "--help")
        run(sys.executable, Path(__file__).with_name("test-modem-cli.py"), cli)
        run(modem, "--help")
        run(modem, "--check-config", station)
        run(modem, "--simulate")
        if rocky_bw500_profile is not None:
            smoke_rocky_bw500(cli, modem, rocky_bw500_profile)
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
    parser.add_argument("--rocky-bw500-profile", type=Path,
                        help="test the installed Rocky Linux 9 server RPM over managed virtual BW500")
    args = parser.parse_args()
    package = args.package.resolve()
    version = re.fullmatch(r"linkwise-modem[-_](\d+\.\d+\.\d+)[-_].+", package.name)
    if not version:
        parser.error(f"unrecognized Linkwise package filename: {package.name}")
    verify_checksum(package)
    rocky_bw500_profile = args.rocky_bw500_profile
    if rocky_bw500_profile is not None:
        rocky_bw500_profile = validate_rocky_bw500(package, rocky_bw500_profile)
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
        smoke(package, version[1], station, rocky_bw500_profile)
        if station.read_text(encoding="utf-8") != configuration:
            raise RuntimeError("Uninstall modified the station configuration")
    print(f"Package smoke test passed: {package.name}")


if __name__ == "__main__":
    main()
