import { build } from "esbuild";
import { copyFileSync } from "node:fs";
await build({
  entryPoints: ["src/sigplot-entry.js"],
  bundle: true,
  minify: true,
  platform: "browser",
  target: ["es2020"],
  outfile: "public/vendor/sigplot-3.1.7.js",
  legalComments: "linked",
});
copyFileSync("node_modules/sigplot/LICENSE", "public/vendor/LICENSE.sigplot");
