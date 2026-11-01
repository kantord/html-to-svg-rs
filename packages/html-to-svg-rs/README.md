# html-to-svg-rs

Turns a page that Chrome laid out into SVG, in Node.js: the converter is Rust, loaded as a native
addon (napi-rs), so no binary has to be installed or spawned.

```js
const { convert } = require("html-to-svg-rs");

const { svg, png, report } = await convert("captures/home");
```

`convert(dir, options?)` takes a capture directory and returns a Promise (the work runs on Node's
thread pool):

- `svg`: the SVG; elements with `data-*` attributes keep them on the groups they painted;
- `png`: the same picture, rasterized by the converter's own renderer;
- `report`: what was drawn and what was not (`warnings`, `rasterFallbacks`, `dropped`), and
  `fonts`: which of the faces Chrome used were found;
- `assets`: with `assetsDir`, the image files the SVG links to.

Options: `assetsDir` writes embedded images to a folder shared by many SVGs, named by content
hash (`assetsHref` and `svgDir` decide how the SVG links them); `extraFonts` registers more font
files; `fitText: false` turns off the letter-spacing correction that makes each line exactly as
wide as Chrome made it.

`styles()` lists the computed styles a capture must ask Chrome for.

## Where captures come from

A capture directory holds Chrome's layout (`snapshot.json`, from `DOMSnapshot.captureSnapshot`),
its screenshot (`chrome.png`) and the fonts and images it used. The Playwright fixture
[`playwright-html-to-svg-rs`](../playwright-html-to-svg-rs) writes them during a test; the
`html-to-svg capture <url> <dir>` command of this repository writes them for any page.

## Platforms

Prebuilt for Linux (x64 and arm64, glibc and musl), macOS (x64 and arm64) and Windows (x64):
npm installs only the one for the machine (`html-to-svg-rs-<platform>`, optional dependencies).

## Building

```bash
pnpm build          # release build of crates/html-to-svg-node for this machine
pnpm build:debug
pnpm test
```

Releases are built and published by `.github/workflows/npm.yml` on a `v*` tag.
