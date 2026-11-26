import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFile, stat } from "node:fs/promises";
import { packagesFor, releaseManifest, repository } from "../src/releases.mjs";
import { locales, loadMessages, pages, pagePath } from "../../docs/i18n/locales.mjs";
import { catalog, translation } from "../../docs/i18n/catalog.mjs";
import { manuals } from "../../docs/i18n/manuals.mjs";
import { varacGuide } from "../../docs/i18n/varac.mjs";
import { websiteLabels } from "../../docs/i18n/website-upgrade.mjs";
import { audioSamples, deploymentBuilds, projectMedia, supportLinks } from "../src/project-content.mjs";

const sha = "b".repeat(64);
const dist = process.argv[2] ?? "dist";
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

const manifest = JSON.parse(await readFile(dist + "/releases.json", "utf8"));
assert.equal(manifest.installers.length, manifest.status === "available" ? 8 : 0);
for (const { id } of deploymentBuilds) {
  assert.ok(!manifest.installers.some(installer => installer.id === id), id);
}
const wslGuide = await readFile(dist + "/install/windows-wsl.md", "utf8");
assert.ok(wslGuide.includes(repository + "/blob/main/.github/workflows/rocky-wsl.yml"));
assert.ok(wslGuide.includes("(rocky-server.md#mail-access)"));
const rockyGuide = await readFile(dist + "/install/rocky-server.md", "utf8");
assert.ok(rockyGuide.includes(repository + "/blob/main/docs/architecture/station-integration-status.md"));
for (const [source, output] of projectMedia) {
  assert.ok((await stat(dist + "/" + output)).size > 0, output);
  assert.equal((await stat(dist + "/" + output)).size, (await stat("../" + source)).size, output);
}
const issueTemplate = await readFile("../.github/ISSUE_TEMPLATE/bug-report.yml", "utf8");
assert.ok(issueTemplate.includes("label: Steps to reproduce"));
const xml = await readFile(dist + "/sitemap.xml", "utf8");
const reference = catalog().flatMap(({ paths }) => paths);
const origin = "https://8r4n.github.io/linkwise";
const iconName = "github-invertocat-white.svg";
assert.deepEqual(await readFile(dist + "/" + iconName),
  await readFile(new URL("../../tools/hf-live/web/" + iconName, import.meta.url)));

const hfEntry = dist + "/hf-simulation/index.html";
const hfPage = await readFile(hfEntry, "utf8").catch(error => {
  if (error.code !== "ENOENT" || process.env.GITHUB_ACTIONS === "true") throw error;
});
if (hfPage) {
  assert.match(hfPage, /data-mode="browser"/);
  assert.ok(hfPage.includes(`href="${repository}" aria-label="Linkwise on GitHub"`));
  assert.match(hfPage, /<img src="\.\/github-invertocat-white\.svg" alt="" /);
  const wasm = await WebAssembly.compile(await readFile(dist + "/hf-simulation/bw500_browser.wasm"));
  const names = WebAssembly.Module.exports(wasm).map(({ name }) => name);
  for (const name of ["memory", "lw_encode_len", "lw_decode", "lw_propagate", "lw_set_mode", "lw_payload_capacity"]) {
    assert.ok(names.includes(name), name);
  }
  const { exports: modem } = await WebAssembly.instantiate(wasm);
  const payload = Uint8Array.of(1, 2, 3, 4, 5, 6, 7, 8, 9);
  for (const bandwidth of [500, 2300, 2750]) {
    assert.equal(modem.lw_set_mode(bandwidth), 1);
    new Uint8Array(modem.memory.buffer, modem.lw_payload_ptr(), payload.length).set(payload);
    const count = modem.lw_encode_len(payload.length);
    assert.equal(count, bandwidth === 500 ? 258048 : 210432);
    assert.equal(modem.lw_decode(count), 1);
    assert.deepEqual(new Uint8Array(modem.memory.buffer, modem.lw_payload_ptr(), payload.length), payload);
  }
  for (const asset of [
    "browser-worker.js", "data/path-inputs.mjs", "data/stations.mjs",
    "github-invertocat-white.svg", "vendor/Cesium.js", "vendor/sigplot.js",
  ]) {
    assert.ok((await stat(dist + "/hf-simulation/" + asset)).size > 0, asset);
  }
  const demos = await readFile(dist + "/demos/index.html", "utf8");
  assert.ok(demos.includes('href="/linkwise/hf-simulation/"'));
  execFileSync(process.execPath, ["--test", "../tools/hf-live/test_station_messaging.mjs",
    "../tools/hf-live/test_radio_policy.mjs"], { stdio: "inherit" });
}

async function checkHtml(path, code, dir) {
  const html = await readFile(dist + path + "index.html", "utf8");
  assert.ok(html.includes('<html lang="' + code + '" dir="' + dir + '">'), path);
  assert.ok(html.includes('rel="canonical" href="' + origin + path + '"'), path);
  assert.ok(html.includes("<h1>"), path);
  assert.ok(html.includes(`href="${repository}" lang="en" aria-label="Linkwise on GitHub"`), path);
  assert.ok(html.includes(`<img src="/linkwise/${iconName}" alt=""`), path);
  for (const { code: alternate } of locales) {
    assert.ok(html.includes('rel="alternate" hreflang="' + alternate + '"'), path + alternate);
  }
  for (const [, target] of html.matchAll(/(?:href|src|poster)="\/linkwise\/([^"#?]+)(?:[?#][^"]*)?"/g)) {
    await stat(dist + "/" + (target.endsWith("/") ? target + "index.html" : target));
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
  const project = websiteLabels(code);
  const varac = varacGuide(code);
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
    if (page === "" || page === "guide") {
      assert.ok(html.includes('href="/linkwise' + pagePath(code, "varac") + '"'), path);
    }
    if (page === "") {
      assert.ok(html.includes("<title>" + varac.siteTitle + " | Linkwise</title>"), path);
      assert.ok(html.includes(`data-release-status="${manifest.status}"`), path);
      assert.ok(html.includes('href="/linkwise' + pagePath(code, "demos") + '"'), path);
      assert.ok(html.includes('href="/linkwise' + pagePath(code, "capabilities") + '#station"'), path);
      if (manifest.status === "pending") {
        assert.ok(html.includes(project.pending) && html.includes(pagePath(code, "capabilities") + "#build"), path);
      } else assert.ok(html.includes(project.available), path);
    }
    if (page === "varac") {
      assert.ok(html.includes("<title>" + varac.title + "</title>"), path);
      assert.ok(html.includes('name="description" content="' + varac.lede + '"'), path);
      assert.ok(html.includes("/connect PEERCALL") && html.includes("/docs/testing/varac-chat.md/"), path);
    }
    if (page === "downloads") {
      assert.ok(html.includes(`data-release-status="${manifest.status}"`), path);
      assert.ok(html.includes(manifest.release_url ?? supportLinks.releases), path);
      for (const asset of manifest.installers) {
        assert.ok(html.includes(asset.url) && html.includes(asset.sha256));
        await stat(new URL(asset.install_guide).pathname.replace("/linkwise/", dist + "/"));
      }
      for (const { id, workflow } of deploymentBuilds) {
        const source = `docs/install/${id}.md`;
        const guide = translation(code, source) ? `/linkwise/reference/${code}/${source}/` : `/linkwise/install/${id}.md`;
        assert.ok(html.includes(`id="${id}"`) && html.includes(guide) && html.includes(workflow), path + id);
      }
      assert.ok(html.includes("Windows import and first launch remain open"), path);
    }
    if (["downloads", "guide", "varac", "winlink"].includes(page)) {
      assert.ok(html.includes(supportLinks.reportBug) && html.includes(supportLinks.knownIssues), path);
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
    if (page === "capabilities") {
      assert.ok(html.includes('id="build"') && html.includes("--features audio-device"), path);
      assert.ok(html.includes('id="station"') && html.includes("commissioned RMS/CMS exchange"), path);
      assert.ok(html.includes("/docs/architecture/station-integration-status.md/") && html.includes(repository + "/blob/main/docs/whitepaper/README.md"), path);
    }
    if (page === "demos") {
      for (const anchor of ["waveform", "winlink", "chat-and-mail", "rocky-live-usb", "briefing"]) {
        assert.ok(html.includes(`id="${anchor}"`), path + "#" + anchor);
      }
      for (const [name] of audioSamples) assert.ok(html.includes(`/linkwise/media/${name}`), path);
      assert.ok(html.includes("/linkwise/media/robustness-briefing-transcript.md"), path);
    }
    if (page === "research") {
      for (const anchor of ["acquisition", "interference", "beacons"]) assert.ok(html.includes(`id="${anchor}"`), path);
      for (const name of ["modem-robustness-test-plan.pdf", "lab-notes.pdf", "beacon-range-rings.png", "beacon-frame-fields.png"]) {
        assert.ok(html.includes(`/linkwise/media/${name}`), path);
      }
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
    const data = await readFile(dist + "/manuals/" + code + "/" + name, "utf8");
    const canonical = await readFile(new URL("../../docs/man/" + name, import.meta.url), "utf8");
    if (code === "en") assert.equal(data, canonical);
    else if (translation(code, "docs/man/" + name)) {
      assert.equal(data, await readFile(translation(code, "docs/man/" + name), "utf8"));
    } else {
      assert.ok(data.includes(t.man.overview));
      assert.ok(data.endsWith(canonical.slice(canonical.indexOf("\n.SH SYNOPSIS") + 1)));
    }
  }
  const guide = await readFile(dist + "/guides/" + code + ".md", "utf8");
  assert.ok(guide.includes(t.guide.title) && guide.includes("linkwise qsl card"));
  const agents = await readFile(dist + pagePath(code) + "llms.txt", "utf8");
  for (const page of ["capabilities", "demos", "research"]) {
    assert.ok(agents.includes(origin + pagePath(code, page)), code + " " + page);
  }
  assert.ok(agents.includes(origin + pagePath(code, "varac")), code + " VarAC");
  for (const anchor of ["chat-verification", "mail-client-tests", "performance"]) {
    assert.ok(agents.includes(origin + pagePath(code, "guide") + "#" + anchor), code + " " + anchor);
  }
  for (const [, href] of agents.matchAll(/\]\((https:\/\/[^)]+)\)/g)) {
    assert.ok(href.startsWith(origin + "/"), href);
    const { pathname, hash } = new URL(href);
    const target = dist + pathname.slice("/linkwise".length) + (pathname.endsWith("/") ? "index.html" : "");
    if (hash) assert.ok((await readFile(target, "utf8")).includes('id="' + hash.slice(1) + '"'), href);
    else await stat(target);
  }
}

console.log(locales.length + " locales, " + reference.length + " references per locale, manual downloads, canonical URLs, and internal links passed.");
