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
the test report and returns `{ svg, dir }` (`dir` is the capture under the test's output
directory: `snapshot.json`, `chrome.png`, `out.svg`, the fonts and images Chrome used).

## Setup

- **Chromium or Chrome only:** the capture reads Chrome's layout over CDP.
- **The binary:** set `htmlToSvgBinary` in `use` (or `$HTML_TO_SVG_BIN`); the default is
  `html-to-svg` on `PATH`. Build it with `cargo build --release -p html-to-svg-cli`. The package
  does not ship the binary yet.
- **Fonts:** system fonts are found with `fc-list` on the machine that runs the conversion, which
  is the one running the test. Web fonts and images are saved from the test's own responses, which
  are recorded from the moment the test asks for `svgSnapshot`.
- The viewport is captured at scroll position 0; any device scale factor works.

## Tests

`pnpm test` builds the debug binary and runs `tests/` on Chromium, Chromium at 2x and Chrome: the
SVG keeps the page's `data-*` attributes, and the SVG drawn back by Chrome differs from Chrome's
picture of the page in under 2% of pixels.
