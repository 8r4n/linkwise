export const repository = "https://github.com/8r4n/linkwise";
export const targets = [
  { id: "windows", label: "Windows", requirement: "Windows 11", format: "exe", guide: "windows", names: ["linkwise-modem-{v}-windows-x86_64.exe", "linkwise-modem-{v}-windows-arm64.exe"] },
  { id: "macos", label: "macOS", requirement: "macOS 13+", format: "pkg", guide: "macos", names: ["linkwise-modem-{v}-macos-x86_64.pkg", "linkwise-modem-{v}-macos-arm64.pkg"] },
  { id: "ubuntu", label: "Ubuntu", requirement: "Ubuntu 24.04", format: "deb", guide: "ubuntu", names: ["linkwise-modem_{v}_amd64.deb", "linkwise-modem_{v}_arm64.deb"] },
  { id: "fedora", label: "Fedora", requirement: "Fedora 45", format: "rpm", guide: "fedora", names: ["linkwise-modem-{v}-1.x86_64.rpm", "linkwise-modem-{v}-1.aarch64.rpm"] },
];

export function packagesFor(version) {
  return targets.flatMap(({ names, ...target }) => names.map((name, index) => ({
    ...target,
    architecture: index === 0 ? "x64" : "arm64",
    filename: name.replace("{v}", version),
  })));
}

export async function releaseManifest(fetcher = fetch) {
  const response = await fetcher("https://api.github.com/repos/8r4n/linkwise/releases?per_page=30");
  if (!response.ok) throw new Error(`GitHub release lookup failed: ${response.status}`);
  const releases = await response.json();
  for (const release of releases) {
    const match = /^v(\d+\.\d+\.\d+)(?:-[\w.-]+)?$/.exec(release.tag_name);
    if (release.draft || !match) continue;
    const assets = new Map(release.assets.map(asset => [asset.name, asset]));
    const installers = packagesFor(match[1]).map(pkg => {
      const asset = assets.get(pkg.filename);
      const checksum = assets.get(pkg.filename + ".sha256");
      const prefix = `${repository}/releases/download/${release.tag_name}/`;
      if (asset?.browser_download_url !== prefix + pkg.filename
          || checksum?.browser_download_url !== prefix + pkg.filename + ".sha256"
          || !/^sha256:[a-f0-9]{64}$/.test(asset.digest ?? "")) return null;
      return {
        ...pkg,
        url: asset.browser_download_url,
        checksum_url: checksum.browser_download_url,
        sha256: asset.digest.slice(7),
        install_guide: `https://8r4n.github.io/linkwise/install/${pkg.guide}.md`,
      };
    });
    if (installers.every(Boolean)) return {
      schema_version: 1, status: "available", version: match[1], tag: release.tag_name,
      preview: release.prerelease, release_url: `${repository}/releases/tag/${release.tag_name}`, installers,
    };
  }
  return { schema_version: 1, status: "pending", installers: [] };
}

let snapshot;
export function publishedRelease() {
  return snapshot ??= releaseManifest();
}
