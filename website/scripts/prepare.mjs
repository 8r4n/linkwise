import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";

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
await writeFile(new URL("llms.txt", publicDir), `# Linkwise

> Open-source HF software modem preview for amateur radio.

Native Rust peer-to-peer mail forwarding is in development. Virtual BW500 attachment delivery has been verified on the bench. Winlink Express is a P2P interoperability workflow.

Read the manifest before selecting an installer. A pending manifest means there is no complete public installer release. Match the target OS, distro and architecture, verify the SHA-256, and use the linked installation and service verification instructions.

- [Release manifest](${base}releases.json)
- [Platform downloads](${base}downloads/)
- [Winlink Express VARA HF P2P](${base}winlink/)
- [Native Rust mail forwarding status](${base}mail/)
`);
await writeFile(new URL("sitemap.xml", publicDir), `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${["", "downloads/", "winlink/", "mail/"].map(path => `  <url><loc>${base}${path}</loc></url>`).join("\n")}
</urlset>
`);
await writeFile(new URL(".nojekyll", publicDir), "");
