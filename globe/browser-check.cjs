const { chromium } = require("playwright-core");
const assert = require("node:assert/strict");
(async () => {
  const browser = await chromium.launch({
    executablePath: process.env.CHROME_BIN,
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
  });
  try {
    const p = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    const url = process.env.GLOBE_URL || "http://127.0.0.1:8743";
    const errors = [],
      external = [];
    p.on("pageerror", (e) => errors.push(e.message));
    p.on("request", (r) => {
      if (
        /^https?:/.test(r.url()) &&
        new URL(r.url()).origin !== new URL(url).origin
      )
        external.push(r.url());
    });
    await p.goto(url);
    await p.waitForFunction(
      () => document.querySelector("#state").textContent === "Queued at F5ZFX",
    );
    assert(await p.locator("#gate").isDisabled());
    await p.selectOption("#scenario", "six");
    await p.waitForFunction(
      () => document.querySelector("#state").textContent === "Queued at D",
    );
    const set = (n) =>
      p.locator("#time").evaluate((e, n) => {
        e.value = n;
        e.dispatchEvent(new Event("input"));
      }, n);
    await p.waitForFunction(() =>
      document.querySelector("#weather").textContent.includes("NOAA"),
    );
    await p.selectOption("#physics", "ray");
    assert.match(await p.locator("#model").textContent(), /^Connecting ray/);
    await p.selectOption("#band", "2");
    assert.match(await p.locator("#metrics").textContent(), /14.10 MHz/);
    assert.match(
      await p.locator("#model").textContent(),
      /^No connecting modeled ray/,
    );
    await p.selectOption("#band", "0");
    await p.click("#tilt");
    await p.waitForTimeout(1000);
    await set(10);
    assert.equal(await p.locator("#state").textContent(), "Delivered at F");
    await set(6);
    assert.equal(await p.locator("#state").textContent(), "Queued at D");
    await p.check("#gate");
    await set(20);
    assert.equal(await p.locator("#state").textContent(), "Expired at A");
    await p.uncheck("#gate");
    await p.selectOption("#policy", "0");
    assert.equal(await p.locator("#state").textContent(), "Expired at C");
    await p.selectOption("#layer", "quality");
    await p.selectOption("#layer", "height");
    await p.selectOption("#physics", "schematic");
    await p.selectOption("#scenario", "global-1000");
    await p.waitForFunction(
      () => document.querySelector("#message").options.length === 1000,
    );
    await p.selectOption("#policy", "1");
    await set(0);
    assert.match(await p.locator("#weather").textContent(), /gap/);
    await set(90);
    await p.waitForFunction(() =>
      document.querySelector("#weather").textContent.includes("NOAA"),
    );
    await p.check("#potential");
    await p.selectOption("#message", "50");
    await p.click("#home");
    const perf = await p.evaluate(async () => {
      const samples = [];
      let last = performance.now(),
        start = last,
        idx = 89;
      return new Promise((resolve) => {
        function step(t) {
          samples.push(t - last);
          last = t;
          if (t - start > 3500) {
            samples.shift();
            samples.sort((a, b) => a - b);
            resolve({
              frames: samples.length,
              ms: t - start,
              p95_ms: samples[Math.floor(0.95 * samples.length)],
            });
            return;
          }
          if (samples.length % 4 === 0) {
            const e = document.querySelector("#time");
            e.value = ++idx;
            e.dispatchEvent(new Event("input"));
          }
          requestAnimationFrame(step);
        }
        requestAnimationFrame(step);
      });
    });
    console.log(
      "Headless SwiftShader, 1280x800, 1000 nodes, slot advances",
      perf,
    );
    assert.deepEqual(errors, []);
    assert.deepEqual(external, []);
    assert.equal(await p.locator(".cesium-widget-errorPanel").count(), 0);
    console.log("All UI checks passed. Zero external requests.");
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
