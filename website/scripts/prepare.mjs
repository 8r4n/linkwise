import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { locales, loadMessages, pages, pagePath } from "../../docs/i18n/locales.mjs";
import { buildGuides, buildManuals } from "../../docs/i18n/manuals.mjs";
import { catalog, translation } from "../../docs/i18n/catalog.mjs";

const root = new URL("../../", import.meta.url);
const publicDir = new URL("website/public/", root);
const base = "https://8r4n.github.io/linkwise/";
await mkdir(new URL("install/", publicDir), { recursive: true });
for (const guide of ["windows", "macos", "ubuntu", "fedora", "winlink-quick-start"]) {
  const source = await readFile(new URL(`docs/install/${guide}.md`, root), "utf8");
  const markdown = source.replaceAll(
    "../../rust/", "https://github.com/8r4n/linkwise/blob/main/rust/",
  );
  await writeFile(new URL(`install/${guide}.md`, publicDir), markdown);
}
for (const [source, name] of [["modem-demo.png", "modem-demo.png"], ["winlink-prbs9/winlink-prbs9.mp4", "winlink-prbs9.mp4"]]) {
  await copyFile(new URL("docs/media/modem/" + source, root), new URL(name, publicDir));
}
for (const { code } of locales) {
  const t = loadMessages(code);
  const target = new URL(pagePath(code).slice(1), publicDir);
  await mkdir(target, { recursive: true });
  const overview = [
    "# Linkwise", "", "> " + t.home.lede, "", t.downloads.verify, t.downloads.pending + " GitHub Releases.", "",
    "- [" + t.downloads.manifest + "](" + base + "releases.json)",
    ...Object.entries(t.nav).map(([page, label]) => "- [" + label + "](" + base + pagePath(code, page).slice(1) + ")"),
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
