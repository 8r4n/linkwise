#!/usr/bin/env python3
"""Build a native Linkwise modem preview installer."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET

from modem_licenses import notices

ROOT = Path(__file__).resolve().parents[1]
PACKAGE_ID = "io.github.8r4n.linkwise.modem"
PROGRAMS = ("linkwise", "linkwise-modemd")
FORMATS = {"Darwin": ("pkg",), "Windows": ("exe",), "Linux": ("deb", "rpm")}
ARCHITECTURES = {
    "arm64": ("arm64", "aarch64", "arm64"),
    "x86_64": ("amd64", "x86_64", "x64os"),
}


def run(*args, cwd=ROOT, capture=False, env=None):
    return subprocess.run(
        args, cwd=cwd, env=env, check=True, text=True, stdout=subprocess.PIPE if capture else None
    ).stdout


def stage(binaries, docs, binary_dir, suffix, guide, third_party):
    binaries.mkdir(parents=True, exist_ok=True)
    docs.mkdir(parents=True, exist_ok=True)
    for program in PROGRAMS:
        name = program + suffix
        shutil.copyfile(binary_dir / name, binaries / name)
        (binaries / name).chmod(0o755)
    for source, name in (
        (ROOT / "LICENSE", "LICENSE"),
        (ROOT / "docs/install" / f"{guide}.md", "Install.md"),
        (ROOT / "rust/docs/modem-setup.md", "Station-setup.md"),
    ):
        shutil.copyfile(source, docs / name)
        (docs / name).chmod(0o644)


    (docs / "THIRD-PARTY-NOTICES.txt").write_text(third_party, encoding="utf-8")


def stage_man_pages(directory, *, compress=False):
    for name in ("linkwise.1", "linkwise-modemd.8", "linkwise-station.5"):
        source = ROOT / "docs/man" / name
        section = directory / ("man" + source.suffix[1:])
        section.mkdir(parents=True, exist_ok=True)
        destination = section / (name + ".gz" if compress else name)
        if compress:
            destination.write_bytes(gzip.compress(source.read_bytes(), mtime=0))
        else:
            shutil.copyfile(source, destination)
        destination.chmod(0o644)


def stage_linux_units(payload):
    for scope, source in (("system", "linkwise-modem.service"), ("user", "linkwise-modem-user.service")):
        directory = payload / "usr/lib/systemd" / scope
        directory.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / "packaging/systemd" / source, directory / "linkwise-modem.service")


def build_deb(work, binary_dir, version, architecture, output, third_party):
    debian = work / "debian"
    payload = debian / "linkwise-modem"
    stage(payload / "usr/bin", payload / "usr/share/doc/linkwise-modem", binary_dir, "", "ubuntu", third_party)
    stage_man_pages(payload / "usr/share/man", compress=True)
    stage_linux_units(payload)
    (debian / "control").write_text(
        "Source: linkwise-modem\n\nPackage: linkwise-modem\nArchitecture: any\n",
        encoding="utf-8",
    )
    control = payload / "DEBIAN"
    control.mkdir()
    dependencies = run(
        "dpkg-shlibdeps", "-O", *((payload / "usr/bin" / name).relative_to(work) for name in PROGRAMS),
        cwd=work, capture=True,
    ).strip().removeprefix("shlibs:Depends=")
    if not dependencies or dependencies.startswith("shlibs:"):
        raise RuntimeError("dpkg-shlibdeps did not report shared-library dependencies")
    (control / "control").write_text(
        f"Package: linkwise-modem\nVersion: {version}\nArchitecture: {architecture}\n"
        "Maintainer: Linkwise project\n"
        "Section: hamradio\nPriority: optional\n"
        f"Depends: init-system-helpers, {dependencies}\n"
        "Homepage: https://github.com/8r4n/linkwise\n"
        "Description: Linkwise modem preview\n"
        " Station diagnostics and local modem simulations for amateur radio.\n",
        encoding="utf-8",
    )
    (debian / "changelog").write_text(
        f"linkwise-modem ({version}) unstable; urgency=medium\n\n"
        "  * Build Linkwise modem.\n\n"
        " -- Linkwise project <noreply@github.com>  Thu, 12 Nov 2026 00:00:00 +0000\n",
        encoding="utf-8",
    )
    (debian / "compat").write_text("13\n", encoding="ascii")
    env = dict(os.environ, DEB_RULES_REQUIRES_ROOT="no")
    # --no-start suppresses debhelper's stop script as well. Stop only on
    # removal and restart only an already running service on upgrade.
    (debian / "linkwise-modem.prerm").write_text(
        '#!/bin/sh\nset -e\n'
        'if [ "$1" = remove ] && [ -d /run/systemd/system ]; then\n'
        '    deb-systemd-invoke stop linkwise-modem.service >/dev/null\n'
        'fi\n#DEBHELPER#\n', encoding="ascii",
    )
    (debian / "linkwise-modem.postinst").write_text(
        '#!/bin/sh\nset -e\n#DEBHELPER#\n'
        'if [ "$1" = configure ] && [ -n "$2" ] && [ -d /run/systemd/system ]; then\n'
        '    systemctl try-restart linkwise-modem.service\n'
        'fi\n', encoding="ascii",
    )
    run("dh_installsystemd", "--no-enable", "--no-start", cwd=work, env=env)
    run("dh_installsystemduser", "--no-enable", cwd=work, env=env)
    run("dh_installdeb", cwd=work, env=env)
    run("dpkg-deb", "--root-owner-group", "--build", payload, output)


def build_rpm(work, binary_dir, version, architecture, output, third_party):
    payload = work / "payload"
    stage(payload / "usr/bin", payload / "usr/share/doc/linkwise-modem", binary_dir, "", "fedora", third_party)
    stage_man_pages(payload / "usr/share/man")
    stage_linux_units(payload)
    shutil.copytree(ROOT / "packaging/selinux", work / "selinux")
    shutil.copyfile(ROOT / "packaging/systemd/80-linkwise-modem.preset", work / "80-linkwise-modem.preset")
    run(
        "rpmbuild", "-bb", "--define", f"_topdir {work}", "--define",
        f"linkwise_version {version}", "--target", architecture,
        ROOT / "packaging/linkwise-modem.spec",
    )
    shutil.copyfile(next((work / "RPMS" / architecture).glob("*.rpm")), output)


def build_pkg(work, binary_dir, version, architecture, output, identity, application_identity, third_party):
    payload = work / "payload"
    stage(
        payload / "usr/local/bin", payload / "usr/local/share/doc/linkwise-modem",
        binary_dir, "", "macos", third_party,
    )
    stage_man_pages(payload / "usr/local/share/man")
    macos = ROOT / "packaging/macos"
    app = payload / "Applications/Linkwise Station.app"
    contents = app / "Contents"
    executables = contents / "MacOS"
    agents = contents / "Library/LaunchAgents"
    executables.mkdir(parents=True)
    agents.mkdir(parents=True)
    shutil.copyfile(macos / "Info.plist", contents / "Info.plist")
    agent = "io.github.8r4n.linkwise.modem.station.plist"
    shutil.copyfile(macos / agent, agents / agent)
    shutil.copy2(payload / "usr/local/bin/linkwise-modemd", executables)
    cpu = "arm64" if architecture == "arm64" else "x86_64"
    run("swiftc", "-target", f"{cpu}-apple-macos13", macos / "Station.swift",
        "-o", executables / "LinkwiseStation")
    if application_identity:
        sign = ("codesign", "--force", "--options", "runtime", "--timestamp",
                "--sign", application_identity)
        for item in (executables / "linkwise-modemd", payload / "usr/local/bin/linkwise-modemd", app):
            run(*sign, "--entitlements", macos / "Station.entitlements", item)
        run(*sign, payload / "usr/local/bin/linkwise")
        run("codesign", "--verify", "--deep", "--strict", app)
    component = work / "linkwise-modem-component.pkg"
    run(
        "pkgbuild", "--root", payload, "--ownership", "recommended", "--identifier",
        PACKAGE_ID, "--version", version, "--install-location", "/", component,
    )
    distribution = ET.Element("installer-gui-script", {"minSpecVersion": "2"})
    ET.SubElement(distribution, "title").text = "Linkwise Modem Preview"
    ET.SubElement(distribution, "options", {"hostArchitectures": architecture, "customize": "never"})
    ET.SubElement(distribution, "domains", {"enable_localSystem": "true"})
    ET.SubElement(distribution, "welcome", {"file": "welcome.html"})
    outline = ET.SubElement(distribution, "choices-outline")
    ET.SubElement(outline, "line", {"choice": "linkwise-modem"})
    choice = ET.SubElement(distribution, "choice", {"id": "linkwise-modem", "visible": "false"})
    ET.SubElement(choice, "pkg-ref", {"id": PACKAGE_ID})
    ET.SubElement(distribution, "pkg-ref", {"id": PACKAGE_ID, "version": version}).text = component.name
    xml = work / "distribution.xml"
    ET.ElementTree(distribution).write(xml, encoding="utf-8", xml_declaration=True)
    signing = ("--sign", identity) if identity else ()
    run(
        "productbuild", "--distribution", xml, "--package-path", work, "--resources",
        ROOT / "packaging/macos", *signing, output,
    )


def build_exe(work, binary_dir, version, architecture, output, compiler, thumbprint, third_party):
    stage(work, work, binary_dir, ".exe", "windows", third_party)
    shutil.copy2(ROOT / "packaging/windows/Configure-Winlink.ps1", work)
    shutil.copy2(ROOT / "docs/install/winlink-quick-start.md", work / "Winlink-Quick-Start.md")
    if thumbprint:
        for program in PROGRAMS:
            sign_windows(work / (program + ".exe"), thumbprint)
    run(
        compiler, f"/DAppVersion={version}", f"/DAppArchitecture={architecture}",
        f"/DStageDir={work}", f"/O{output.parent}", f"/F{output.stem}",
        ROOT / "packaging/linkwise-modem.iss",
    )
    if thumbprint:
        sign_windows(output, thumbprint)


def sign_windows(path, thumbprint):
    run("signtool", "sign", "/sha1", thumbprint, "/fd", "SHA256",
        "/tr", "http://timestamp.digicert.com", "/td", "SHA256", path)
    run("signtool", "verify", "/pa", path)



def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("format", choices=("pkg", "exe", "deb", "rpm"))
    parser.add_argument("--output-dir", type=Path, default=ROOT / "dist")
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--iscc", default="ISCC.exe", help="path to the Inno Setup 6 compiler")
    parser.add_argument("--macos-installer-identity", help="Developer ID Installer certificate")
    parser.add_argument("--macos-application-identity", help="Developer ID Application certificate")
    parser.add_argument("--windows-signing-thumbprint", help="Authenticode certificate thumbprint")
    parser.add_argument("--macos-notary-profile", help="notarytool keychain profile")
    args = parser.parse_args()
    if args.macos_notary_profile and not (args.macos_application_identity and args.macos_installer_identity):
        parser.error("macOS notarization requires both Developer ID signing identities")
    system = platform.system()
    if args.format not in FORMATS.get(system, ()):
        parser.error(f"{args.format} packaging requires its native build platform")
    machine = platform.machine().lower()
    architecture = {"aarch64": "arm64", "amd64": "x86_64"}.get(machine, machine)
    if architecture not in ARCHITECTURES:
        parser.error(f"unsupported architecture: {machine}")

    target_suffix = {"Darwin": "apple-darwin", "Windows": "pc-windows-msvc",
                     "Linux": "unknown-linux-gnu"}[system]
    target_cpu = "aarch64" if architecture == "arm64" else "x86_64"
    target = f"{target_cpu}-{target_suffix}"
    cargo_manifest = ROOT / "rust/Cargo.toml"
    metadata = json.loads(run(args.cargo, "metadata", "--locked", "--no-deps", "--format-version=1",
                              "--manifest-path", cargo_manifest, capture=True))
    version = next(item["version"] for item in metadata["packages"] if item["name"] == "linkwise")
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        parser.error(f"expected a three-part numeric Cargo version, got {version}")
    third_party = notices(args.cargo, target)
    build_env = os.environ.copy()
    if system == "Windows":
        # Keep the installer independent of a separately installed VC runtime.
        encoded = build_env.get("CARGO_ENCODED_RUSTFLAGS")
        if encoded is None:
            build_env["RUSTFLAGS"] = (
                build_env.get("RUSTFLAGS", "") + " -C target-feature=+crt-static"
            ).strip()
        else:
            build_env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(
                flag for flag in (encoded, "-C", "target-feature=+crt-static") if flag
            )
    run(
        args.cargo, "build", "--locked", "--release", "--target", target, "--manifest-path", cargo_manifest,
        "--features", "audio-device", "--bin", "linkwise", "--bin", "linkwise-modemd",
        env=build_env,
    )
    binary_dir = Path(metadata["target_directory"]) / target / "release"
    suffix = ".exe" if system == "Windows" else ""
    reported = run(binary_dir / ("linkwise-modemd" + suffix), "--version", capture=True).strip()
    if reported != f"linkwise-modemd {version}":
        parser.error(f"built modem reports an unexpected version: {reported}")

    output_dir = args.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    deb_arch, rpm_arch, windows_arch = ARCHITECTURES[architecture]
    filenames = {
        "deb": f"linkwise-modem_{version}_{deb_arch}.deb",
        "rpm": f"linkwise-modem-{version}-1.{rpm_arch}.rpm",
        "pkg": f"linkwise-modem-{version}-macos-{architecture}.pkg",
        "exe": f"linkwise-modem-{version}-windows-{architecture}.exe",
    }
    filename = filenames[args.format]
    with tempfile.TemporaryDirectory(prefix=".linkwise-", dir=output_dir) as directory:
        work = Path(directory)
        output = work / filename
        if args.format == "deb":
            build_deb(work, binary_dir, version, deb_arch, output, third_party)
        elif args.format == "rpm":
            build_rpm(work, binary_dir, version, rpm_arch, output, third_party)
        elif args.format == "pkg":
            build_pkg(work, binary_dir, version, architecture, output, args.macos_installer_identity, args.macos_application_identity, third_party)
        else:
            build_exe(work, binary_dir, version, windows_arch, output, args.iscc, args.windows_signing_thumbprint, third_party)
        if args.format == "pkg" and args.macos_notary_profile:
            run("xcrun", "notarytool", "submit", output, "--keychain-profile", args.macos_notary_profile, "--wait")
            run("xcrun", "stapler", "staple", output)
            run("xcrun", "stapler", "validate", output)
        checksum = hashlib.sha256(output.read_bytes()).hexdigest()
        manifest = work / f"{filename}.sha256"
        manifest.write_text(f"{checksum}  {filename}\n", encoding="ascii")
        os.replace(output, output_dir / filename)
        os.replace(manifest, output_dir / manifest.name)
    print(output_dir / filename)


if __name__ == "__main__":
    main()
