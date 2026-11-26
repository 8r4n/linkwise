import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { locales, loadMessages, pages, pagePath } from "../../docs/i18n/locales.mjs";
import { buildGuides, buildManuals } from "../../docs/i18n/manuals.mjs";
import { catalog, translation } from "../../docs/i18n/catalog.mjs";
import { guideTrustMail } from "../../docs/i18n/guide-trust-mail.mjs";
import { visualGuide } from "../../docs/i18n/visual-guide.mjs";
import { varacGuide } from "../../docs/i18n/varac.mjs";
import { websiteLabels } from "../../docs/i18n/website-upgrade.mjs";
import { deploymentBuilds, projectMedia, supportLinks } from "../src/project-content.mjs";
import { repository } from "../src/releases.mjs";

const root = new URL("../../", import.meta.url);
const publicDir = new URL("website/public/", root);
const base = "https://8r4n.github.io/linkwise/";
const repositorySources = repository + "/blob/main/";
const commandLine = command => "- `" + command + "`";
await mkdir(new URL("install/", publicDir), { recursive: true });
await copyFile(new URL("tools/hf-live/web/github-invertocat-white.svg", root),
  new URL("github-invertocat-white.svg", publicDir));
for (const guide of ["windows", "macos", "ubuntu", "fedora", "winlink-quick-start", ...deploymentBuilds.map(({ id }) => id)]) {
  const source = await readFile(new URL(`docs/install/${guide}.md`, root), "utf8");
  const markdown = source
    .replaceAll("../../rust/", repositorySources + "rust/")
    .replaceAll("../../.github/", repositorySources + ".github/")
    .replaceAll("../architecture/", repositorySources + "docs/architecture/");
  await writeFile(new URL(`install/${guide}.md`, publicDir), markdown);
}
for (const [source, name] of projectMedia) {
  const destination = new URL(name, publicDir);
  await mkdir(new URL(".", destination), { recursive: true });
  await copyFile(new URL(source, root), destination);
}
for (const { code } of locales) {
  const t = loadMessages(code);
  const project = websiteLabels(code);
  const trust = guideTrustMail(code);
  const visual = visualGuide(code);
  const varac = varacGuide(code);
  const target = new URL(pagePath(code).slice(1), publicDir);
  const guide = base + pagePath(code, "guide").slice(1);
  const capabilities = base + pagePath(code, "capabilities").slice(1);
  const link = (label, url) => "- [" + label + "](" + url + ")";
  const reference = source => base + "reference/" + code + "/" + source + "/";
  await mkdir(target, { recursive: true });
  const overview = [
    "# Linkwise", "", "> " + project.overview, "", t.guide.lede, "",
    "## Start here", "",
    link("Project source on GitHub", repository),
    link("First run from the project README", repository + "#first-run"),
    link("What works today", repository + "#what-works-today"),
    link("HF propagation and antenna simulator", base + "hf-simulation/"),
    link("Matched 20 m antenna gain study", base + "research/antenna-gain/"),
    link("Local MCP guide", repositorySources + "rust/docs/mcp-usage.md#simulated-radio-mcp-quickstart"), "",
    "## " + t.nav.downloads, "", t.downloads.verify, "",
    "If installers are pending, use the browser simulator or build from source.", "",
    link(t.downloads.manifest, base + "releases.json"),
    link(t.nav.downloads, base + pagePath(code, "downloads").slice(1)),
    link(project.releaseNotes, supportLinks.releases),
    link(project.build, capabilities + "#build"),
    ...deploymentBuilds.map(({ id, title }) => link(title, base + `install/${id}.md`)),
    link(t.ui.manuals, reference("docs/man/linkwise.1")), "",
    "## " + t.nav.guide, "",
    link(t.guide.title, guide),
    link(t.guide.start, guide + "#start"),
    link(t.nav.winlink, base + pagePath(code, "winlink").slice(1)),
    ...["capabilities", "demos", "learn", "research"].map(page => link(project[page], base + pagePath(code, page).slice(1))),
    link(t.nav.docs, base + pagePath(code, "docs").slice(1)), "",
    "## Station qualification", "",
    link("Station integration and qualification", capabilities + "#station"),
    link("Station implementation and test evidence", repositorySources + "docs/architecture/station-integration-status.md"), "",
    "## " + varac.title, "", varac.lede, "", varac.setup, "", varac.bench, varac.scope, "",
    link(varac.title, base + pagePath(code, "varac").slice(1)),
    link(visual.bench, reference("docs/testing/varac-chat.md")), "",
    "## " + trust.verify, "", t.guide.chatText, "", trust.trust, trust.proof, "",
    link(trust.verify, guide + "#chat-verification"),
    link(t.guide.pki, reference("rust/docs/pki.md")),
    link(t.guide.files, guide + "#files"),
    link(t.guide.qsl, guide + "#qsl"),
    commandLine("linkwise pki check station-pki.json"),
    commandLine("linkwise chat --pki station-pki.json --enroll YOURCALL"),
    commandLine("/cert offer [--encrypt]"),
    commandLine("/cert inspect"),
    commandLine("/cert accept SIGNING [ENCRYPTION]"),
    commandLine("/proof message-proof.json"),
    commandLine("linkwise pki verify-chat station-pki.json message-proof.json"), "",
    "## " + trust.clients, "", trust.flow, "", trust.tls, trust.radio, "",
    link(trust.clients, guide + "#mail-example"),
    link(trust.setup, reference("rust/docs/mobile-development.md")),
    link(trust.tested, guide + "#mail-client-tests"),
    link(t.guide.mail, guide + "#mail"),
    commandLine("linkwise mail smtp MAILBOX CALLSIGN 2525 --starttls cert.pem key.pem"),
    commandLine("linkwise mail imap MAILBOX 1143 --tls cert.pem key.pem"), "",
    "## " + visual.performance, "", visual.payloadNote, visual.turnNote, "", visual.limits, t.guide.securityText, "",
    link(visual.performance, guide + "#performance"),
    link(visual.bench, reference("docs/testing/vara-interoperability.md")),
  ];
  await writeFile(new URL("llms.txt", target), overview.join("\n") + "\n");
}
const documents = catalog().flatMap(({ paths }) => paths);
for (const { code } of locales) {
  for (const source of documents) {
    if (source.startsWith("docs/man/")) continue;
    const local = translation(code, source);
    if (!local) continue;
    const destination = new URL("editions/" + code + "/" + source, publicDir);
    await mkdir(new URL(".", destination), { recursive: true });
    if (source.endsWith(".md")) {
      const markdown = (await readFile(local, "utf8")).replace(
        /\]\((?:\.\.\/){6}(?=[a-zA-Z])/g, "](https://github.com/8r4n/linkwise/blob/main/",
      );
      await writeFile(destination, markdown);
    } else {
      await copyFile(local, destination);
    }
  }
}
await buildManuals(new URL("manuals/", publicDir));
await buildGuides(new URL("guides/", publicDir));
await writeFile(new URL(".nojekyll", publicDir), "");

if (process.env.GITHUB_ACTIONS === "true") {
  const forecast = join(process.env.RUNNER_TEMP || tmpdir(), "linkwise-hf-forecast.json");
  const env = { ...process.env, LINKWISE_HF_REFRESH_WEATHER: "1" };
  execFileSync("rustup", ["target", "add", "wasm32-unknown-unknown", "--toolchain", "stable"], { stdio: "inherit" });
  try {
    execFileSync("bash", [fileURLToPath(new URL("forecast-hf.sh", import.meta.url)), forecast],
      { stdio: "inherit", timeout: 180_000 });
    env.LINKWISE_HF_FORECAST = forecast;
  } catch {
    console.warn("P.533 forecast preparation failed; the HF page will use its scenario fallback.");
  }
  execFileSync(process.execPath, [fileURLToPath(new URL("prepare-hf.mjs", import.meta.url))],
    { stdio: "inherit", env });
}

const sitemapPaths = new Set(locales.flatMap(({ code }) => pages.map(page => pagePath(code, page).slice(1))));
sitemapPaths.add("research/antenna-gain/");
if (["index.html", "bw500_browser.wasm"].every(file => existsSync(new URL(`hf-simulation/${file}`, publicDir)))) {
  sitemapPaths.add("hf-simulation/");
}
await writeFile(new URL("sitemap.xml", publicDir), `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${Array.from(sitemapPaths, path => `  <url><loc>${base}${path}</loc></url>`).join("\n")}
</urlset>
`);
