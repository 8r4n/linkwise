# Linkwise website

The Astro site uses the layout and shadcn component conventions from the
[AREA44 Astro shadcn/ui template](https://github.com/area44/astro-shadcn-ui-template/tree/3020318a9bd28b77d62d983453f316a59d7954c4).
The template license is in TEMPLATE-LICENSE.

Use Node.js 22.19 or later.

```sh
cd website
npm ci
npm run build
npm run check
npm run dev
```

Open the development server at `/linkwise/`. The production URL is
`https://8r4n.github.io/linkwise/`.

In the repository's Pages settings, select **GitHub Actions** as the source.
The website workflow checks pull requests and deploys from main. A manual run
or the scheduled rebuild also refreshes the public release snapshot.

## Browser HF simulation

GitHub Pages serves `/linkwise/hf-simulation/` as static assets. Each visitor's
Web Worker runs the 48-station scheduler, the Linkwise modem built for
WebAssembly, and the modeled HF channel. The globe and SigPlot stay on the
main browser thread. The three choices use BW500 level-1 and BW2300/BW2750
level-4 DATA waveforms. The station console encrypts chat and email between
ephemeral station keys with ECDH P-256 and AES-256-GCM. Authenticated messages
appear in the simulated inbox after their modem frames arrive. The simulated
station chooses a frequency and transmit power, listens for a clear channel,
and backs off or retunes when delivery fails. A waiting message can hold the
next clear turn on one band while current traffic finishes. Marginal paths
and interference can exhaust the retry budget. This is a frame-level
simulation with browser-local mail delivery.

The site’s **Learn** page links to four guided missions. Training uses preset
simulated links and messages. Mission progress and the HF Operator badge are
stored in the browser.

**Antennas and link budget** adds two matched antenna exercises to Learn.
The Antenna Lab uses bundled forecast and weather inputs and saves progress
in the browser. The simulator's **Antenna-enabled simulation** link opens the
interactive station ports and rotator for free play.

The Pages build prepares the assets automatically. For local development,
install the Rust WASM target and prepare the page before running Astro.

```sh
rustup target add wasm32-unknown-unknown --toolchain stable
npm run build:hf
npm run build
npm run check
node --test ../tools/hf-live/test_station_messaging.mjs
```

Set `LINKWISE_HF_FORECAST` to a monthly cache exported by
`tools/hf-live/path_forecast.py` to include a P.533/P.372 path baseline.
`LINKWISE_HF_REFRESH_WEATHER=1` stages a timestamped NOAA snapshot.
GitHub Actions tries both during the scheduled Pages build, and the browser
can refresh NOAA directly and fall back to its staged inputs or scenario.
Visitors run independent simulations while their pages are open.

Create a tag such as `v0.1.0-preview` on the reviewed release commit. The
installer workflow checks all eight packages and checksum sidecars before
publishing a GitHub prerelease. The website build uses only complete public
release sets and obtains the SHA-256 from GitHub asset metadata. The same
release snapshot feeds the static download table and releases.json.

The homepage uses the same release status to show either downloads or source
build and demo links. The downloads and setup pages link directly to GitHub's
release notes, bug report form, and open issue list. Support requests stay with
the repository.

The capabilities, demos, Learn, and research pages use the shared locale navigation.
Detailed technical content is provided in English. Update the evidence and
media inventory in `src/project-content.mjs` and the linked research sources
when refreshing the site. Render the two beacon thumbnails from the current
A4 map and frame diagram PDF. Run this from the repository root.

```sh
pdftoppm -r 106 -png lab-notes/figures/kiwisdr-beacon-map-dsp.pdf docs/testing/figures/kiwisdr-beacon-preview
```

After deployment, submit `https://8r4n.github.io/linkwise/sitemap.xml` in a
verified Search Console URL-prefix property for `https://8r4n.github.io/linkwise/`.
Inspect the home, VarAC, and operator guide URLs there to check discovery and
indexing. A hostname-wide crawler policy belongs at
`https://8r4n.github.io/robots.txt`, separately from the project deployment.

## Translations

English keeps the unprefixed URL. For example, Arabic starts at
'/linkwise/ar/' and Simplified Chinese at '/linkwise/zh-Hans/'.
Each page links to its language equivalents. The operator guide also
builds as downloadable Markdown. The documentation catalog links to each
source and identifies coverage for localized reference editions.

Edit the shared message bundles under **docs/i18n/messages/**. Run the site
check after changes. See the [localization guide](../docs/i18n/README.md)
for full document overlays, man page generation, and fallback behavior.
