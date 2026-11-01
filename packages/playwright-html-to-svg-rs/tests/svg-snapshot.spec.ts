import { readFile } from "node:fs/promises";
import { join } from "node:path";

import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

import { expect, test } from "../src/index.js";

const ORIGIN = "https://html-to-svg.test";
const PAGES = new URL("./pages/", import.meta.url);

// Serve tests/pages from a made-up origin: the image is then a real network response, which the
// capture has to save for the conversion to draw it.
test.beforeEach(async ({ page }) => {
  await page.route(`${ORIGIN}/**`, async (route) => {
    const name = new URL(route.request().url()).pathname.slice(1);
    const body = await readFile(new URL(name, PAGES));
    const contentType = name.endsWith(".svg") ? "image/svg+xml" : "text/html";
    await route.fulfill({ body, contentType });
  });
});

test("the SVG keeps the page's data-* attributes", async ({ page, svgSnapshot }) => {
  await page.goto(`${ORIGIN}/card.html`);
  const { svg } = await svgSnapshot("card");

  expect(svg.startsWith("<svg")).toBe(true);
  for (const id of ["summary", "badge", "regions", "row-eu", "dots"]) {
    expect(svg, `data-testid="${id}"`).toContain(`data-testid="${id}"`);
  }
  expect(svg).not.toContain("data-takumi-node");
});

test("the SVG, drawn by Chrome, looks like the page", async ({ page, svgSnapshot }) => {
  await page.goto(`${ORIGIN}/card.html`);
  const { svg, dir } = await svgSnapshot("card");
  const original = PNG.sync.read(await readFile(join(dir, "chrome.png")));

  // Draw the SVG at the same CSS size and compare it with Chrome's own picture of the page.
  await page.setViewportSize({ width: original.width, height: original.height });
  await page.setContent(`<body style="margin:0">${svg}</body>`);
  const drawn = PNG.sync.read(await page.screenshot({ scale: "css" }));

  const diff = new PNG({ width: original.width, height: original.height });
  const differing = pixelmatch(
    original.data,
    drawn.data,
    diff.data,
    original.width,
    original.height,
    { threshold: 0.2 },
  );
  await test.info().attach("diff.png", { body: PNG.sync.write(diff), contentType: "image/png" });
  const share = differing / (original.width * original.height);
  // Glyph anti-aliasing differs a little; misplaced or missing content shows up far above this.
  expect(share, `${(share * 100).toFixed(2)}% of pixels differ`).toBeLessThan(0.02);
});
