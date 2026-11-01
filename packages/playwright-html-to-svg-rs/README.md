# playwright-html-to-svg-rs

SVG snapshots of the page under test, in Playwright: Chrome lays the page out, and the
`html-to-svg` binary of this repository (`crates/html-to-svg-cli`) paints Chrome's boxes as SVG.

```ts
import { expect, test } from "playwright-html-to-svg-rs";

test("dashboard", async ({ page, svgSnapshot }) => {
  await page.goto("/dashboard");
  const { svg } = await svgSnapshot("dashboard");
  expect(svg).toContain('data-testid="revenue"');
});
```

`svgSnapshot(name, page?)` captures the current viewport, converts it, attaches `<name>.svg` to
the test report and returns `{ svg, report, dir }`. `report` is what the conversion found:
faces not found, warnings (e.g. an image that was not captured), elements cropped from Chrome's
screenshot (`<img>`, `<canvas>`, form controls). (`dir` is the capture under the test's output
directory: `snapshot.json`, `chrome.png`, `out.svg`, the fonts and images Chrome used).

## Shared assets

By default each SVG is self-contained: images are inlined as `data:` URLs. With many snapshots
showing the same images, set a shared folder instead:

```ts
export default defineConfig<HtmlToSvgOptions>({
  use: { htmlToSvgAssetsDir: "svg-assets" },
});
```

Each image is then written there once as `<sha256>.<ext>` and the SVGs link to it by a path
relative to the SVG. Downloaded images keep their bytes and their URL's extension, so the same
image is the same file in every snapshot and every run; `<img>`/`<canvas>` crops are `.png`. The
SVG attached to the test report then needs the folder next to it to show its images.

## Setup

- **Chromium or Chrome only:** the capture reads Chrome's layout over CDP.
- **The binary:** set `htmlToSvgBinary` in `use` (or `$HTML_TO_SVG_BIN`); the default is
  `html-to-svg` on `PATH`. Build it with `cargo build --release -p html-to-svg-cli`. The package
  does not ship the binary yet.
- **Fonts:** system fonts are found with `fc-list` on the machine that runs the conversion, which
  is the one running the test. Web fonts and images are saved from the test's own responses, which
  are recorded for the whole browser context from its creation, so navigating in a `beforeEach`
  is fine. A context the test creates itself is recorded from its first `svgSnapshot` on.
- The viewport is captured at scroll position 0; any device scale factor works.

## Tests

`pnpm test` builds the debug binary and runs `tests/` on Chromium, Chromium at 2x and Chrome
(`just test` at the repository root runs them in the test container), against the pages of
`packages/test-fixtures`, which Playwright builds and serves with Vite. They test the glue, not the
converter's accuracy (that is the Rust tests' job): `data-*` attributes reach the SVG; the web
font and images the page downloaded are saved and used; `<img>` and `<canvas>` are cropped from
the screenshot; and, as a coarse check, the SVG drawn back by Chrome differs from Chrome's picture
of the page in under 2% of pixels.
