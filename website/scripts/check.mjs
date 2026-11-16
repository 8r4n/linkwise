import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import { packagesFor, releaseManifest, repository } from "../src/releases.mjs";
import { locales, loadMessages, pages, pagePath } from "../../docs/i18n/locales.mjs";
import { catalog, translation } from "../../docs/i18n/catalog.mjs";
import { manuals } from "../../docs/i18n/manuals.mjs";

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
const xml = await readFile("dist/sitemap.xml", "utf8");
const reference = catalog().flatMap(({ paths }) => paths);
const origin = "https://8r4n.github.io/linkwise";

async function checkHtml(path, code, dir) {
  const html = await readFile("dist" + path + "index.html", "utf8");
  assert.ok(html.includes('<html lang="' + code + '" dir="' + dir + '">'), path);
  assert.ok(html.includes('rel="canonical" href="' + origin + path + '"'), path);
  assert.ok(html.includes("<h1>"), path);
  for (const { code: alternate } of locales) {
    assert.ok(html.includes('rel="alternate" hreflang="' + alternate + '"'), path + alternate);
  }
  for (const [, target] of html.matchAll(/(?:href|src)="\/linkwise\/([^"#?]+)(?:[?#][^"]*)?"/g)) {
    await stat("dist/" + (target.endsWith("/") ? target + "index.html" : target));
  }
  return html;
}

const completeReferences = [
  "docs/man/linkwise-modemd.8", "docs/man/linkwise-station.5", "docs/install/macos.md",
  "docs/install/ubuntu.md", "docs/install/fedora.md", "docs/install/winlink-quick-start.md",
];
const examples = (text, source) => Array.from(
  text.matchAll(source.endsWith(".md") ? /^[ \t]*`{3}sh\n([\s\S]*?)^\s*`{3}$/gm : /^\.EX\n([\s\S]*?)^\.EE$/gm),
  ([, body]) => body.trimEnd(),
);

for (const { code, dir = "ltr" } of locales) {
  const t = loadMessages(code);
  for (const source of completeReferences) {
    if (code === "en") break;
    const local = translation(code, source);
    assert.ok(local, "Missing full edition: " + code + " " + source);
    const [english, edition] = await Promise.all([
      readFile(new URL("../../" + source, import.meta.url), "utf8"), readFile(local, "utf8"),
    ]);
    assert.deepEqual(examples(edition, source), examples(english, source), code + " " + source);
    if (source.includes("winlink-quick-start")) {
      for (const term of ["18310", "8300", "127.0.0.1", "500", "Apply and start service", "Automatically launch TNC when session is opened", "tools/fetch-modem-profile.py"]) {
        assert.ok(edition.includes(term), code + " " + term);
      }
    }
  }
  for (const page of pages) {
    const path = pagePath(code, page);
    const html = await checkHtml(path, code, dir);
    assert.ok(xml.includes("<loc>" + origin + path + "</loc>"));
    if (page === "downloads") {
      for (const asset of manifest.installers) {
        assert.ok(html.includes(asset.url) && html.includes(asset.sha256));
        await stat(new URL(asset.install_guide).pathname.replace("/linkwise/", "dist/"));
      }
    }
    if (page === "guide") {
      assert.ok(html.includes('<pre dir="ltr">') && html.includes("linkwise qsl receive"));
      for (const anchor of ["workflow-examples", "chat-example", "chat-verification", "mail-example", "mail-client-tests", "performance", "payload-chart", "turn-chart"]) {
        assert.ok(html.includes('id="' + anchor + '"'), path + "#" + anchor);
      }
      assert.ok(html.includes("linkwise mail listen ./mailbox BB2BB AA1AA"), path);
      assert.ok(html.includes("linkwise pki verify-chat") && html.includes("2525 · STARTTLS / SSL/TLS"), path);
      assert.ok(html.includes("VARA HF → Linkwise") && html.includes("Linkwise → VARA HF"), path);
    }
  }
  for (const source of reference) {
    const path = "/reference/" + code + "/" + source + "/";
    const html = await checkHtml(path, code, dir);
    assert.ok(html.includes(repository + "/blob/main/" + source), source);
    const local = translation(code, source);
    if (code !== "en" && !local) {
      assert.ok(html.includes(t.ui.fallback), path);
    }
    if (local && !source.startsWith("docs/man/")) {
      assert.ok(html.includes("/linkwise/editions/" + code + "/" + source), path);
    }
  }
  for (const name of manuals) {
    const data = await readFile("dist/manuals/" + code + "/" + name, "utf8");
    const canonical = await readFile(new URL("../../docs/man/" + name, import.meta.url), "utf8");
    if (code === "en") assert.equal(data, canonical);
    else if (translation(code, "docs/man/" + name)) {
      assert.equal(data, await readFile(translation(code, "docs/man/" + name), "utf8"));
    } else {
      assert.ok(data.includes(t.man.overview));
      assert.ok(data.endsWith(canonical.slice(canonical.indexOf("\n.SH SYNOPSIS") + 1)));
    }
  }
  const guide = await readFile("dist/guides/" + code + ".md", "utf8");
  assert.ok(guide.includes(t.guide.title) && guide.includes("linkwise qsl card"));
  const agents = await readFile("dist" + pagePath(code) + "llms.txt", "utf8");
  for (const anchor of ["chat-verification", "mail-client-tests", "performance"]) {
    assert.ok(agents.includes(origin + pagePath(code, "guide") + "#" + anchor), code + " " + anchor);
  }
  for (const [, href] of agents.matchAll(/\]\((https:\/\/[^)]+)\)/g)) {
    assert.ok(href.startsWith(origin + "/"), href);
    const { pathname, hash } = new URL(href);
    const target = "dist" + pathname.slice("/linkwise".length) + (pathname.endsWith("/") ? "index.html" : "");
    if (hash) assert.ok((await readFile(target, "utf8")).includes('id="' + hash.slice(1) + '"'), href);
    else await stat(target);
  }
}

console.log(locales.length + " locales, " + reference.length + " references per locale, manual downloads, canonical URLs, and internal links passed.");
