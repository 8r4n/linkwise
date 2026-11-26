import { execFileSync } from "node:child_process";
import { cp, copyFile, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const source = join(root, "tools/hf-live");
const crate = join(root, "rust/hf-browser");
const target = join(crate, "target");
const page = join(root, "website/public/hf-simulation");
const vendor = join(page, "vendor");
const sigplot = join(root, "rust/demo-ui/public/vendor");
const python = process.env.LINKWISE_HF_PYTHON || "python3";

function run(program, args, env = process.env) {
  execFileSync(program, args, { cwd: root, env, stdio: "inherit" });
}

run("cargo", ["+stable", "build", "--manifest-path", join(crate, "Cargo.toml"), "--locked",
  "--target", "wasm32-unknown-unknown", "--release"], { ...process.env, CARGO_TARGET_DIR: target });
run(python, [join(root, "globe/prepare.py")]);
await mkdir(vendor, { recursive: true });
await cp(join(root, "globe/vendor"), vendor, { recursive: true });
await copyFile(join(root, "globe/imagery/blue-marble-clear.jpg"), join(vendor, "blue-marble-clear.jpg"));
for (const [from, to] of [
  ["sigplot-3.1.7.js", "sigplot.js"],
  ["sigplot-3.1.7.js.LEGAL.txt", "sigplot-3.1.7.js.LEGAL.txt"],
  ["LICENSE.sigplot", "LICENSE.sigplot"],
  ["NOTICE.sigplot", "NOTICE.sigplot"],
]) {
  await copyFile(join(sigplot, from), join(vendor, to));
}

const web = join(source, "web");
for (const filename of await readdir(web)) {
  if (/\.(?:m?js|css|svg)$/.test(filename)) await copyFile(join(web, filename), join(page, filename));
}
await cp(join(web, "training-data"), join(page, "training-data"), { recursive: true });
const html = await readFile(join(web, "index.html"), "utf8");
const start = '<html lang="en">';
if (!html.includes(start)) throw new Error("HF dashboard HTML is missing its browser mode entry point");
await writeFile(join(page, "index.html"), html.replace(start, '<html lang="en" data-mode="browser">')
  .replace("<title>Linkwise | Live HF simulation</title>", "<title>Linkwise | Browser HF simulation</title>"));
await copyFile(join(target, "wasm32-unknown-unknown/release/linkwise_hf_browser.wasm"), join(page, "bw500_browser.wasm"));

const data = [join(source, "site-inputs/build.py"), "--output", join(page, "data")];
if (process.env.LINKWISE_HF_FORECAST) data.push("--forecast", process.env.LINKWISE_HF_FORECAST);
if (process.env.LINKWISE_HF_REFRESH_WEATHER === "1") data.push("--refresh-weather");
run(python, data);
