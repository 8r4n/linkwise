// Camera choreography is independent of RF timing. The scrubber seeks the actual replay.
export async function start(viewer, state, rayPoints, changeScenario) {
  const C = window.Cesium,
    el = (id) => document.getElementById(id);
  document.body.classList.add("orbital");
  const panel = document.createElement("section");
  panel.id = "flight";
  panel.innerHTML =
    '<div class="flightTop"><b>LINKWISE</b><span>RF NETWORK REPLAY</span></div><div class="flightBottom"><div id="flightRegion"></div><h1 id="flightTitle"></h1><p id="flightRead"></p></div><div class="flightClock"></div><div class="flightTec">Vertical electron content<div></div>0 <span>60+ TECU</span></div><div class="flightCredit">Winlink directory · 15 Nov 2026  |  Simulated links  |  Earth imagery · NASA / NOAA</div><div class="flightBar"><button id="fly">Play tour</button><input id="track" aria-label="Orbital tour position" type="range" min="0" max="1" step=".0001" value="0"><a href="./">Open inspector ↗</a></div>';
  document.body.append(panel);
  const segments = [
    {
      title: "Galicia after dark",
      region: "IBERIAN PENINSULA",
      from: [-2, 45, 22, -21, 3300000],
      to: [-7, 42, 40, -21, 2600000],
    },
    {
      title: "Relaying across Europe",
      region: "SIMULATED MESSAGE M1",
      from: [2, 50, 14, -58, 5600000],
      to: [3, 51, 22, -53, 5900000],
    },
    {
      title: "Through the F2 region",
      region: "F5ZFX  →  HB9T",
      from: [4.5, 45.8, 12, -14, 1700000],
      to: [4.5, 45.8, 20, -11, 1350000],
    },
    {
      title: "The global network",
      region: "PUBLIC VARA HF DIRECTORY",
      from: [-89, 33, 14, -66, 10000000],
      to: [-70, 37, 35, -54, 8500000],
    },
  ];
  const durations = [8, 11, 13, 12],
    total = 44;
  const tracks = viewer.scene.primitives.add(new C.PolylineCollection());
  const markers = viewer.scene.primitives.add(new C.PointPrimitiveCollection());
  const pulse = markers.add({
    pixelSize: 8,
    color: C.Color.WHITE,
    outlineColor: C.Color.CYAN,
    outlineWidth: 2,
    show: false,
  });
  let path = [],
    priorKey = "";
  function routeGraphics(i, slot, u, band) {
    const { replay, run } = state();
    const events = run.events.filter((e) => e[1] === 0 && e[4] === "transfer");
    const key = [i, slot, band].join("/");
    if (key !== priorKey) {
      priorKey = key;
      tracks.removeAll();
      path = [];
      if (i === 1) {
        for (const e of events.filter((e) => e[0] <= slot)) {
          const c = replay.contacts[e[2]],
            a = replay.stations[c[1]],
            b = replay.stations[c[2]];
          const g = new C.EllipsoidGeodesic(
            C.Cartographic.fromDegrees(a.lon, a.lat),
            C.Cartographic.fromDegrees(b.lon, b.lat),
          );
          const positions = Array.from({ length: 97 }, (_, j) => {
            const p = g.interpolateUsingFraction(j / 96);
            return C.Cartesian3.fromRadians(p.longitude, p.latitude, 8000);
          });
          tracks.add({
            positions,
            width: 3,
            material: C.Material.fromType("PolylineGlow", {
              color: C.Color.fromCssColorString("#64ffe1"),
              glowPower: 0.23,
            }),
          });
          if (e[0] === slot) path = positions;
        }
      }
    }
    if (i === 2 && !path.length) path = rayPoints();
    pulse.show = path.length > 1;
    if (pulse.show)
      pulse.position =
        path[
          Math.min(
            path.length - 1,
            Math.floor(((u * (i === 1 ? 7 : 4)) % 1) * path.length),
          )
        ];
  }
  let chapter = -1,
    previousSlot = -1,
    previousBand = -2,
    animation,
    playing = false,
    running = false;
  function select(id, val) {
    if (el(id).value !== String(val)) {
      el(id).value = val;
      el(id).dispatchEvent(new Event("change"));
    }
  }
  async function seek(progress) {
    const t = Math.min(total - 0.00001, progress * total);
    let i = 0,
      begin = 0;
    while (t >= begin + durations[i]) begin += durations[i++];
    const u = (t - begin) / durations[i],
      s = segments[i];
    document.body.dataset.shot = i;
    if (i !== chapter) {
      const name = i === 3 ? "vara-world" : "vara-europe";
      if (state().replay.scenario !== name) {
        await changeScenario(name);
      }
      select("physics", i === 2 ? "ray" : "schematic");
      select("layer", i === 3 ? "tec" : "off");
      el("flightRegion").textContent = s.region;
      el("flightTitle").textContent = s.title;
      previousSlot = -1;
      previousBand = -2;
      chapter = i;
    }
    const slot =
      i === 0
        ? 0
        : i === 1
          ? [0, 4, 5, 6, 7, 9, 10][Math.min(6, Math.floor(u * 7))]
          : i === 2
            ? 6
            : 90 + Math.floor(u * 6);
    if (slot !== previousSlot) {
      el("time").value = slot;
      el("time").dispatchEvent(new Event("input"));
      previousSlot = slot;
      if (i === 1) {
        const ids = state().run.events.filter(
          (e) => e[1] === 0 && e[4] === "transfer",
        );
        const e = ids.find((e) => e[0] >= slot) || ids.at(-1);
        select("circuit", e[2]);
      }
      if (i === 2)
        select(
          "circuit",
          state().replay.contacts.findIndex((c) => c[1] === 3 && c[2] === 4),
        );
    }
    const band =
      i === 2
        ? state().replay.frequency_hz.indexOf(u < 0.68 ? 3601500 : 14088500)
        : -1;
    if (previousBand !== band) {
      select("band", band);
      previousBand = band;
    }
    routeGraphics(i, slot, u, band);
    const pose = s.from.map((v, k) => v + (s.to[k] - v) * u);
    viewer.camera.lookAt(
      C.Cartesian3.fromDegrees(pose[0], pose[1]),
      new C.HeadingPitchRange(
        C.Math.toRadians(pose[2]),
        C.Math.toRadians(pose[3]),
        pose[4],
      ),
    );
    viewer.camera.lookAtTransform(C.Matrix4.IDENTITY);
    const { run, replay } = state();
    const count = run.events.filter(
      (e) => e[0] === slot && e[2] !== null,
    ).length;
    el("flightRead").textContent =
      i === 0
        ? "Night lights · NASA Black Marble 2016"
        : i === 1
          ? el("state").textContent + "   /   " + el("route").textContent
          : i === 2
            ? (replay.frequency_hz[band] / 1e6).toFixed(4) +
              " MHz" +
              "   /   " +
              el("model").textContent.split(". ")[0]
            : replay.stations.length +
              " gateways · " +
              count +
              " calls this slot   /   " +
              run.summary.delivered +
              " of " +
              replay.offers.length +
              " delivered by deadline";
    document.querySelector(".flightClock").textContent =
      el("clock").textContent;
    el("track").value = progress;
    viewer.scene.requestRender();
  }
  const track = el("track");
  function pause() {
    playing = false;
    cancelAnimationFrame(animation);
    el("fly").textContent = "Play tour";
  }
  let pendingSeek;
  async function step(value) {
    pendingSeek = value;
    if (running) return;
    running = true;
    try {
      while (pendingSeek !== undefined) {
        const next = pendingSeek;
        pendingSeek = undefined;
        await seek(next);
      }
    } finally {
      running = false;
    }
  }
  track.oninput = () => {
    pause();
    step(+track.value);
  };
  el("fly").onclick = () => {
    if (playing) {
      pause();
      return;
    }
    playing = true;
    el("fly").textContent = "Pause";
    const offset = +track.value >= 0.999 ? 0 : +track.value;
    const start = performance.now();
    async function tick() {
      if (!playing) return;
      const v = Math.min(
        1,
        offset + (performance.now() - start) / 1000 / total,
      );
      await step(v);
      if (v === 1) pause();
      else if (playing) animation = requestAnimationFrame(tick);
    }
    tick();
  };
  await seek(0);
  document.body.dataset.orbital = "ready";
  // Explicit frame seeking also permits deterministic off-screen recording.
  track.seekFrame = seek;
}
