"""Collect license texts for the locked modem package build."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TARGETS = (
    "aarch64-apple-darwin", "x86_64-apple-darwin",
    "aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc",
    "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu",
)
REVIEWED = {
    "(MIT OR Apache-2.0) AND Unicode-3.0",
    "Apache-2.0", "Apache-2.0 AND ISC", "Apache-2.0 OR ISC OR MIT", "Apache-2.0 OR MIT", "Apache-2.0/MIT",
    "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",

    "BSD-2-Clause OR MIT OR Apache-2.0", "BSD-3-Clause", "ISC",
    "MIT", "MIT OR Apache-2.0", "MIT/Apache-2.0", "Unlicense OR MIT",
}


def notices(cargo, target, features="audio-device"):
    metadata = json.loads(subprocess.check_output([
        cargo, "metadata", "--locked", "--format-version", "1",
        "--manifest-path", ROOT / "rust/Cargo.toml",
        "--filter-platform", target, "--features", features,
    ]))
    resolve = metadata["resolve"]
    nodes = {node["id"]: node for node in resolve["nodes"]}
    pending = [resolve["root"]]
    seen = set()
    while pending:
        package_id = pending.pop()
        if package_id in seen:
            continue
        seen.add(package_id)
        pending.extend(
            dep["pkg"] for dep in nodes[package_id]["deps"]
            if any(kind["kind"] != "dev" for kind in dep["dep_kinds"])
        )

    sections = ["Third-party notices for Linkwise modem\nTarget: " + target]
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["id"] not in seen or package["id"] == resolve["root"]:
            continue
        name = f'{package["name"]}-{package["version"]}'
        if package["license"] not in REVIEWED:
            raise ValueError(f'{target}: review license for {name}: {package["license"]}')
        directory = Path(package["manifest_path"]).parent
        files = {
            path for path in directory.iterdir() if path.is_file()
            and path.name.lower().startswith(("license", "notice", "copying", "copyright", "unlicense"))
        }
        license_directory = directory / "LICENSES"
        if license_directory.is_dir():
            files.update(path for path in license_directory.iterdir() if path.is_file())
        if package["license_file"]:
            files.add(directory / package["license_file"])
        if not files:
            fallback = ROOT / "packaging/licenses" / f"{name}.txt"
            if not fallback.is_file():
                raise ValueError(f"{target}: no license text for {name}")
            files.add(fallback)
        texts = "\n\n".join(path.read_text(encoding="utf-8").strip() for path in sorted(files))
        sections.append(f'{name} ({package["license"]})\n\n{texts}')
    if target.endswith("-windows-msvc"):
        inno = (ROOT / "packaging/licenses/Inno-Setup-6.7.3.txt").read_text(encoding="utf-8")
        sections.append("Inno Setup 6.7.3 installer engine\n\n" + inno.strip())
    return "\n\n".join(sections) + "\n"


if __name__ == "__main__":
    for triple in TARGETS:
        notices("cargo", triple)
        print(f"Reviewed dependency licenses: {triple}")
