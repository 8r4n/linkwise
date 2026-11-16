import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import { resolve, sep } from "node:path";
import { translation } from "./catalog.mjs";
import { loadMessages, locales } from "./locales.mjs";
import { operatorRuns, operatorSections } from "./operator.mjs";

const bidirectional = (text, dir) => operatorRuns(text, dir)
  .map(({ text, isolate }) => isolate ? "\u2066" + text + "\u2069" : text).join("");

export const manuals = ["linkwise.1", "linkwise-station.5", "linkwise-modemd.8"];
const escape = (value) => "\\&" + value.replaceAll("\\", "\\e");

export async function buildManuals(output, { manpath = false } = {}) {
  for (const { code, dir } of locales) {
    const t = loadMessages(code);
    for (const name of manuals) {
      const relative = "docs/man/" + name;
      const source = new URL("../man/" + name, import.meta.url);
      const destination = new URL(code + "/" + (manpath ? "man" + name.at(-1) + "/" : ""), output);
      await mkdir(destination, { recursive: true });
      const localized = translation(code, relative);
      const canonical = await readFile(localized ?? source, "utf8");
      if (localized || code === "en") {
        await writeFile(new URL(name, destination), canonical);
        continue;
      }
      const edition = name === "linkwise.1"
        ? [escape(t.guide.lede), ...operatorSections.flatMap(([key, commands]) => [
            '.SS "' + t.guide[key].replaceAll('"', '\\(dq') + '"',
            escape(bidirectional(t.guide[key + "Text"], dir)),
            ...(commands.length ? [".EX", ...commands.join("\n").split("\n").map(escape), ".EE"] : []),
          ])]
        : [escape(bidirectional(name === "linkwise-station.5" ? t.man.stationText : t.man.daemonText, dir))];
      const split = canonical.indexOf("\n.SH SYNOPSIS");
      if (split < 0) throw new Error("Missing SYNOPSIS in " + relative);
      await writeFile(new URL(name, destination), [
        '.\\" -*- coding: UTF-8 -*-', canonical.slice(0, split), '.SH "' + t.man.overview + '"', ...edition,
        '.SH "' + t.man.reference + '"', escape(t.ui.fallback), canonical.slice(split + 1),
      ].join("\n"));
    }
  }
}

export async function buildGuides(output) {
  await mkdir(output, { recursive: true });
  for (const { code, dir } of locales) {
    const t = loadMessages(code);
    const lines = ["# " + t.guide.title, "", t.guide.lede, ""];
    for (const [key, commands] of operatorSections) {
      lines.push("## " + t.guide[key], "", bidirectional(t.guide[key + "Text"], dir), "");
      if (commands.length) lines.push("~~~sh", ...commands, "~~~", "");
    }
    await writeFile(new URL(code + ".md", output), lines.join("\n"));
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const output = process.argv[2] ? pathToFileURL(resolve(process.argv[2]) + sep) : new URL("../build/i18n/man/", import.meta.url);
  await buildManuals(output, { manpath: true });
}
