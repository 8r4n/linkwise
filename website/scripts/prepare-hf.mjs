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
const title = "HF Propagation and Antenna Simulator";
const description = "Explore HF propagation and antenna gain in a browser simulator. Run 48 stations, compare antenna patterns, and inspect transmitted and received HF modem signals.";
const introduction = "Run a 48-station HF network in your browser. Explore shortwave propagation, compare antenna gain and orientation, and inspect transmitted and received modem signals.";
const canonical = "https://8r4n.github.io/linkwise/hf-simulation/";

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
await copyFile(join(source, "antenna-profiles.json"), join(page, "antenna-profiles.json"));
await copyFile(join(target, "wasm32-unknown-unknown/release/linkwise_hf_browser.wasm"), join(page, "bw500_browser.wasm"));

const data = [join(source, "site-inputs/build.py"), "--output", join(page, "data")];
if (process.env.LINKWISE_HF_FORECAST) data.push("--forecast", process.env.LINKWISE_HF_FORECAST);
if (process.env.LINKWISE_HF_REFRESH_WEATHER === "1") data.push("--refresh-weather");
run(python, data);

let html = await readFile(join(web, "index.html"), "utf8");
for (const [source, replacement] of [
  ['<html lang="en">', '<html lang="en" data-mode="browser">'],
  ["<title>Linkwise | Live HF simulation</title>", `<title>${title} | Linkwise</title>
    <meta name="description" content="${description}">
    <link rel="canonical" href="${canonical}">
    <link rel="describedby" type="text/plain" href="/linkwise/llms.txt">`],
  ["<h1>Live HF simulation</h1>", `<h1>${title}</h1>
          <p>${introduction}</p>`],
]) {
  if (!html.includes(source)) throw new Error(`HF dashboard HTML is missing ${source}`);
  html = html.replace(source, replacement);
}
await writeFile(join(page, "index.html"), html);
