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

Create a tag such as `v0.1.0-preview` on the reviewed release commit. The
installer workflow checks all eight packages and checksum sidecars before
publishing a GitHub prerelease. The website build uses only complete public
release sets and obtains the SHA-256 from GitHub asset metadata. The same
release snapshot feeds the static download table and releases.json.

After deployment, verify the root crawler policy at
`https://8r4n.github.io/robots.txt` and submit the project sitemap in Search
Console. The root policy is scoped to the GitHub Pages hostname.
