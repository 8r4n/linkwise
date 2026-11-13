import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import { packagesFor, releaseManifest, repository } from "../src/releases.mjs";

const sha = "b".repeat(64);
const tag = "v0.1.0-preview";
const prefix = `${repository}/releases/download/${tag}/`;
const assets = packagesFor("0.1.0").flatMap(({ filename }) => [
  { name: filename, browser_download_url: prefix + filename, digest: "sha256:" + sha },
  { name: filename + ".sha256", browser_download_url: prefix + filename + ".sha256" },
]);
const mock = list => async () => Response.json(list);
const release = { tag_name: tag, prerelease: true, assets };
assert.equal((await releaseManifest(mock([release]))).installers.length, 8);
assert.equal((await releaseManifest(mock([{ ...release, assets: assets.slice(1) }]))).status, "pending");
assert.equal((await releaseManifest(mock([{ ...release, draft: true }]))).status, "pending");

const manifest = JSON.parse(await readFile("dist/releases.json", "utf8"));
assert.equal(manifest.installers.length, manifest.status === "available" ? 8 : 0);
for (const page of ["", "downloads/", "winlink/", "varac/"]) {
  const html = await readFile(`dist/${page}index.html`, "utf8");
  assert.ok(html.includes(`rel="canonical" href="https://8r4n.github.io/linkwise/${page}"`));
  assert.ok(html.includes("<h1>"));
  for (const [, path] of html.matchAll(/(?:href|src)="\/linkwise\/([^"#?]+)(?:[?#][^"]*)?"/g)) {
    await stat(`dist/${path.endsWith("/") ? path + "index.html" : path}`);
  }
  if (page === "downloads/") {
    for (const asset of manifest.installers) {
      assert.ok(html.includes(asset.url) && html.includes(asset.sha256));
      await stat(new URL(asset.install_guide).pathname.replace("/linkwise/", "dist/"));
    }
  }
}
console.log("Release selection, static HTML, canonical URLs, and local links passed.");
