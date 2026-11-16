import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join, resolve, sep } from "node:path";

const root = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
const dirs = {
  operator: ["README.md", "rust/README.md", "website/README.md", "docs/install", "rust/docs"],
  manuals: ["docs/man"],
  architecture: ["docs/architecture"],
  testing: ["docs/testing", "lab-notes"],
  reference: ["docs/i18n/README.md", "docs/whitepaper", "docs/media", "packaging", "reference", "tools"],
};

let index;
export function catalog() {
  if (index) return index;
  const paths = execFileSync("git", ["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--",
    ...Object.values(dirs).flat()], { cwd: root, encoding: "utf8" })
    .split("\0").filter(path => /\.(md|[158]|tex|pdf|html)$/.test(path) && existsSync(join(root, path)));
  index = Object.entries(dirs).map(([group, roots]) => ({
    group, paths: [...new Set(paths.filter(path => roots.some(prefix => path === prefix || path.startsWith(prefix + "/"))))].sort(),
  }));
  return index;
}

export function translation(code, path) {
  if (code === "en") return null;
  const directory = join(root, "docs/i18n/translations", code);
  const candidate = join(directory, path);
  if (!resolve(candidate).startsWith(resolve(directory) + sep)) throw new Error("Invalid document path");
  return existsSync(candidate) ? candidate : null;
}
