#!/usr/bin/env python3
"""Build a native Linkwise modem preview installer."""
import argparse
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


def stage(binaries, docs, binary_dir, suffix, guide):
    binaries.mkdir(parents=True, exist_ok=True)
    docs.mkdir(parents=True, exist_ok=True)
    for program in PROGRAMS:
        name = program + suffix
        shutil.copyfile(binary_dir / name, binaries / name)
        (binaries / name).chmod(0o755)
    for source, name in (
        (ROOT / "docs/install" / f"{guide}.md", "Install.md"),
        (ROOT / "rust/docs/modem-setup.md", "Station-setup.md"),
    ):
        shutil.copyfile(source, docs / name)
        (docs / name).chmod(0o644)


def build_deb(work, binary_dir, version, architecture, output):
    debian = work / "debian"
    payload = debian / "linkwise-modem"
    stage(payload / "usr/bin", payload / "usr/share/doc/linkwise-modem", binary_dir, "", "ubuntu")
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
        f"Depends: {dependencies}\n"
        "Homepage: https://github.com/8r4n/linkwise\n"
        "Description: Linkwise modem preview\n"
        " Station diagnostics and local modem simulations for amateur radio.\n",
        encoding="utf-8",
    )
    run("dpkg-deb", "--root-owner-group", "--build", payload, output)


def build_rpm(work, binary_dir, version, architecture, output):
    payload = work / "payload"
    stage(payload / "usr/bin", payload / "usr/share/doc/linkwise-modem", binary_dir, "", "fedora")
    run(
        "rpmbuild", "-bb", "--define", f"_topdir {work}", "--define",
        f"linkwise_version {version}", "--target", architecture,
        ROOT / "packaging/linkwise-modem.spec",
    )
    shutil.copyfile(next((work / "RPMS" / architecture).glob("*.rpm")), output)


def build_pkg(work, binary_dir, version, architecture, output, identity):
    payload = work / "payload"
    stage(
        payload / "usr/local/bin", payload / "usr/local/share/doc/linkwise-modem",
        binary_dir, "", "macos",
    )
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


def build_exe(work, binary_dir, version, architecture, output, compiler):
    stage(work, work, binary_dir, ".exe", "windows")
    run(
        compiler, f"/DAppVersion={version}", f"/DAppArchitecture={architecture}",
        f"/DStageDir={work}", f"/O{output.parent}", f"/F{output.stem}",
        ROOT / "packaging/linkwise-modem.iss",
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("format", choices=("pkg", "exe", "deb", "rpm"))
    parser.add_argument("--output-dir", type=Path, default=ROOT / "dist")
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--iscc", default="ISCC.exe", help="path to the Inno Setup 6 compiler")
    parser.add_argument("--macos-installer-identity", help="Developer ID Installer certificate")
    args = parser.parse_args()
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
            build_deb(work, binary_dir, version, deb_arch, output)
        elif args.format == "rpm":
            build_rpm(work, binary_dir, version, rpm_arch, output)
        elif args.format == "pkg":
            build_pkg(work, binary_dir, version, architecture, output, args.macos_installer_identity)
        else:
            build_exe(work, binary_dir, version, windows_arch, output, args.iscc)
        checksum = hashlib.sha256(output.read_bytes()).hexdigest()
        manifest = work / f"{filename}.sha256"
        manifest.write_text(f"{checksum}  {filename}\n", encoding="ascii")
        os.replace(output, output_dir / filename)
        os.replace(manifest, output_dir / manifest.name)
    print(output_dir / filename)


if __name__ == "__main__":
    main()
