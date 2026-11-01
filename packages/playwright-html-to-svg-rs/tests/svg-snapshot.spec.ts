// End-to-end tests of the glue between Playwright and the converter: what the fixture captures
// (snapshot, screenshot, fonts, images) and that `html-to-svg convert` can use all of it. How
// close the SVG is to Chrome is the Rust tests' job; the pixel check here only catches gross
// breakage (wrong scale, blank or shifted output).

import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";

import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

import { expect, test } from "../src/index.js";

/** Files the fixtures app serves as they are (`packages/test-fixtures/public`). */
const PUBLIC = new URL("../../test-fixtures/public/", import.meta.url);

// The card fixture (`packages/test-fixtures/pages/card.html`), served by Vite (playwright.config.ts):
// its web font and images are real network responses, which the capture has to save.
test.beforeEach(async ({ page }) => {
  await page.goto("/card.html");
});

test("the SVG keeps the page's data-* attributes", async ({ svgSnapshot }) => {
  const { svg } = await svgSnapshot("card");

  expect(svg.startsWith("<svg")).toBe(true);
  for (const id of ["summary", "badge", "regions", "row-eu", "dots", "web-font", "photo", "chart", "banner"]) {
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

  // Only the subset the page's text needs is downloaded (Vite names it with a content hash).
  const fonts = JSON.parse(await readFile(join(dir, "fonts.json"), "utf8"));
  expect(Object.values(fonts.web)).toEqual([
    expect.stringMatching(/\/assets\/pacifico-latin-400-normal-[\w-]+\.woff2$/),
  ]);
  const faces = Object.values(fonts.used).flat() as { family: string; custom: boolean }[];
  expect(faces).toContainEqual(expect.objectContaining({ family: "Pacifico", custom: true }));

  const images = JSON.parse(await readFile(join(dir, "images.json"), "utf8"));
  const paths = Object.keys(images.images).map((url) => new URL(url).pathname);
  expect(paths.sort()).toEqual(["/dots.svg", "/photo.png", "/stripes.png"]);
});

test("<img> and <canvas> are cropped from Chrome's screenshot", async ({ svgSnapshot }) => {
  const { svg, report } = await svgSnapshot("card");

  const fallbacks = report.report.raster_fallbacks.map((f) => f.split(" ")[0]);
  expect(fallbacks.sort()).toEqual(["<canvas>", "<img>"]);
  // The two crops, and the banner's downloaded background image.
  expect(svg.match(/<image\b/g)?.length).toBe(3);
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

test.describe("with htmlToSvgAssetsDir", () => {
  test.use({
    htmlToSvgAssetsDir: async ({}, use, testInfo) => use(testInfo.outputPath("assets")),
  });

  test("images are shared files named by content hash", async ({ svgSnapshot }, testInfo) => {
    const first = await svgSnapshot("first");
    const second = await svgSnapshot("second");
    const assetsDir = testInfo.outputPath("assets");

    // Nothing inlined; both SVGs link the same files, and each file exists once.
    for (const { svg } of [first, second]) expect(svg).not.toContain("data:image/png");
    expect(second.report.assets).toEqual(first.report.assets);
    expect((await readdir(assetsDir)).sort()).toEqual([...first.report.assets].sort());
    for (const name of first.report.assets) expect(name).toMatch(/^[0-9a-f]{64}\.png$/);

    // Every link resolves from the SVG's own folder to a file whose name is its content's hash.
    const hrefs = [...first.svg.matchAll(/href="([^"#][^"]*)"/g)].map((m) => m[1]);
    expect(hrefs.length).toBeGreaterThan(0);
    for (const href of hrefs) {
      const file = resolve(first.dir, href);
      expect(dirname(file)).toBe(assetsDir);
      const bytes = await readFile(file);
      expect(file).toContain(createHash("sha256").update(bytes).digest("hex"));
    }

    // The downloaded background is stored as the bytes the server sent, not re-encoded: the
    // same image on another page or in another test run is the same file.
    const stripes = await readFile(new URL("stripes.png", PUBLIC));
    const banner = `${createHash("sha256").update(stripes).digest("hex")}.png`;
    expect(first.report.assets).toContain(banner);
  });
});
