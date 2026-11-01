// End-to-end tests of the glue between Playwright and the converter: what the fixture captures
// (snapshot, screenshot, fonts, images) and that `html-to-svg convert` can use all of it. How
// close the SVG is to Chrome is the Rust tests' job; the pixel check here only catches gross
// breakage (wrong scale, blank or shifted output).

import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";

import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

import { expect, test } from "../src/index.js";

const ORIGIN = "https://html-to-svg.test";
const PAGES = new URL("./pages/", import.meta.url);
const WEB_FONT = createRequire(import.meta.url).resolve(
  "@fontsource/pacifico/files/pacifico-latin-400-normal.woff2",
);

/** A 160x90 gradient: an image the page downloads, drawn by an `<img>`. */
function photo(): Buffer {
  const png = new PNG({ width: 160, height: 90 });
  for (let y = 0; y < 90; y++) {
    for (let x = 0; x < 160; x++) {
      const i = (y * 160 + x) * 4;
      png.data.set([x + 60, y * 2 + 40, 200 - x, 255], i);
    }
  }
  return PNG.sync.write(png);
}

// Serve tests/pages from a made-up origin: the font and images are then real network responses,
// which the capture has to save for the conversion to use them.
test.beforeEach(async ({ page }) => {
  await page.route(`${ORIGIN}/**`, async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/fonts/pacifico.woff2") {
      return route.fulfill({ body: await readFile(WEB_FONT), contentType: "font/woff2" });
    }
    if (path === "/photo.png") return route.fulfill({ body: photo(), contentType: "image/png" });
    const body = await readFile(new URL(path.slice(1), PAGES));
    const contentType = path.endsWith(".svg") ? "image/svg+xml" : "text/html";
    return route.fulfill({ body, contentType });
  });
  await page.goto(`${ORIGIN}/card.html`);
});

test("the SVG keeps the page's data-* attributes", async ({ svgSnapshot }) => {
  const { svg } = await svgSnapshot("card");

  expect(svg.startsWith("<svg")).toBe(true);
  for (const id of ["summary", "badge", "regions", "row-eu", "dots", "web-font", "photo", "chart"]) {
    expect(svg, `data-testid="${id}"`).toContain(`data-testid="${id}"`);
  }
  expect(svg).not.toContain("data-takumi-node");
});

test("the conversion finds every font and image the page used", async ({ svgSnapshot }) => {
  const { report, dir } = await svgSnapshot("card");

  // Every face Chrome drew with was found again: system faces through fc-list, the web font
  // from the saved response.
  expect(report.fonts.missing).toEqual([]);
  expect(report.fonts.faces).toBeGreaterThanOrEqual(2);
  // Nothing was left undrawn (a background url() that was not captured would warn).
  expect(report.report.warnings).toEqual([]);

  const fonts = JSON.parse(await readFile(join(dir, "fonts.json"), "utf8"));
  expect(Object.values(fonts.web)).toContain(`${ORIGIN}/fonts/pacifico.woff2`);
  const faces = Object.values(fonts.used).flat() as { family: string; custom: boolean }[];
  expect(faces).toContainEqual(expect.objectContaining({ family: "Pacifico", custom: true }));

  const images = JSON.parse(await readFile(join(dir, "images.json"), "utf8"));
  expect(Object.keys(images.images).sort()).toEqual([`${ORIGIN}/dots.svg`, `${ORIGIN}/photo.png`]);
});

test("<img> and <canvas> are cropped from Chrome's screenshot", async ({ svgSnapshot }) => {
  const { svg, report } = await svgSnapshot("card");

  const fallbacks = report.report.raster_fallbacks.map((f) => f.split(" ")[0]);
  expect(fallbacks.sort()).toEqual(["<canvas>", "<img>"]);
  expect(svg.match(/<image\b/g)?.length).toBe(2);
});

test("the SVG, drawn by Chrome, looks like the page", async ({ page, svgSnapshot }) => {
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
  expect(share, `${(share * 100).toFixed(2)}% of pixels differ`).toBeLessThan(0.02);
});
