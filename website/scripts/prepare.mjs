import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { locales, loadMessages, pages, pagePath } from "../../docs/i18n/locales.mjs";
import { buildGuides, buildManuals } from "../../docs/i18n/manuals.mjs";
import { catalog, translation } from "../../docs/i18n/catalog.mjs";
import { guideTrustMail } from "../../docs/i18n/guide-trust-mail.mjs";
import { visualGuide } from "../../docs/i18n/visual-guide.mjs";
import { varacGuide } from "../../docs/i18n/varac.mjs";
import { websiteLabels } from "../../docs/i18n/website-upgrade.mjs";
import { projectMedia } from "../src/project-content.mjs";

const root = new URL("../../", import.meta.url);
const publicDir = new URL("website/public/", root);
const base = "https://8r4n.github.io/linkwise/";
const commandLine = command => "- `" + command + "`";
await mkdir(new URL("install/", publicDir), { recursive: true });
for (const guide of ["windows", "macos", "ubuntu", "fedora", "winlink-quick-start"]) {
  const source = await readFile(new URL(`docs/install/${guide}.md`, root), "utf8");
  const markdown = source.replaceAll(
    "../../rust/", "https://github.com/8r4n/linkwise/blob/main/rust/",
  );
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
  const link = (label, url) => "- [" + label + "](" + url + ")";
  const reference = source => base + "reference/" + code + "/" + source + "/";
  await mkdir(target, { recursive: true });
  const overview = [
    "# Linkwise", "", "> " + project.overview, "", t.guide.lede, "",
    "## " + t.nav.downloads, "", t.downloads.verify, "",
    link(t.downloads.manifest, base + "releases.json"),
    link(t.nav.downloads, base + pagePath(code, "downloads").slice(1)),
    link(t.ui.manuals, reference("docs/man/linkwise.1")), "",
    "## " + t.nav.guide, "",
    link(t.guide.title, guide),
    link(t.guide.start, guide + "#start"),
    link(t.nav.winlink, base + pagePath(code, "winlink").slice(1)),
    ...["capabilities", "demos", "research"].map(page => link(project[page], base + pagePath(code, page).slice(1))),
    link(t.nav.docs, base + pagePath(code, "docs").slice(1)), "",
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
await writeFile(new URL("sitemap.xml", publicDir), `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${locales.flatMap(({ code }) => pages.map(page => pagePath(code, page).slice(1))).map(path => `  <url><loc>${base}${path}</loc></url>`).join("\n")}
</urlset>
`);
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
