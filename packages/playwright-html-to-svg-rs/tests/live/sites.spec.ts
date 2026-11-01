// Proof of concept on live sites: each test opens a real page, takes an SVG snapshot, and keeps
// Chrome's screenshot (page.png), our SVG (page.svg), our SVG drawn by the same Chrome (svg.png)
// and where they differ (diff.png) in out/live/<site>/, with the numbers in summary.json.
//
// Not part of `pnpm test`: the pages need the network and change every day. Run with
// `just live-snapshots` (in the test container; it also writes out/live/index.html). A test fails
// only when no SVG comes out; how close it is to Chrome is recorded, not asserted.

import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

import { expect, test } from "../../src/index.js";
const OUT = new URL("../../../../out/live/", import.meta.url).pathname;

const SITES: { name: string; url: string }[] = [
  { name: "hacker-news", url: "https://news.ycombinator.com/" },
  { name: "google", url: "https://www.google.com/" },
  { name: "nytimes", url: "https://www.nytimes.com/" },
  { name: "github-html-to-svg-rs", url: "https://github.com/kantord/html-to-svg-rs" },
  { name: "shadcn-ui", url: "https://ui.shadcn.com/" },
];

test.describe.configure({ mode: "parallel" });

for (const site of SITES) {
  test(site.name, async ({ page, svgSnapshot }) => {
    test.setTimeout(120_000);
    const response = await page.goto(site.url, { waitUntil: "load", timeout: 60_000 });
    // Let late content (client rendering, lazy images, web fonts) arrive; a page that keeps
    // the network busy (ads, analytics) gets 10 s.
    await page.waitForLoadState("networkidle", { timeout: 10_000 }).catch(() => {});
    await page.waitForTimeout(1_000);

    const { svg, report, fonts, dir } = await svgSnapshot(site.name);
    expect(svg.startsWith("<svg")).toBe(true);

    // How far our SVG, drawn by the same Chrome, is from Chrome's screenshot of the page.
    const chrome = PNG.sync.read(await readFile(join(dir, "chrome.png")));
    await page.setViewportSize({ width: chrome.width, height: chrome.height });
    await page.setContent(`<body style="margin:0">${svg}</body>`);
    const drawn = await page.screenshot({ scale: "css" });
    const diff = new PNG({ width: chrome.width, height: chrome.height });
    const differing = pixelmatch(chrome.data, PNG.sync.read(drawn).data, diff.data, chrome.width, chrome.height, {
      threshold: 0.2,
    });

    const out = join(OUT, site.name);
    await mkdir(out, { recursive: true });
    await copyFile(join(dir, "chrome.png"), join(out, "page.png"));
    await writeFile(join(out, "page.svg"), svg);
    await writeFile(join(out, "svg.png"), drawn);
    await writeFile(join(out, "diff.png"), PNG.sync.write(diff));
    const summary = {
      name: site.name,
      url: site.url,
      status: response?.status() ?? null,
      date: new Date().toISOString(),
      svgBytes: Buffer.byteLength(svg),
      differingPercent: Math.round((differing / (chrome.width * chrome.height)) * 10_000) / 100,
      boxes: report.boxes,
      textLines: report.textLines,
      fontsMissing: fonts.missing,
      rasterFallbacks: report.rasterFallbacks.length,
      dropped: Object.keys(report.dropped).length,
      warnings: report.warnings,
    };
    await writeFile(join(out, "summary.json"), JSON.stringify(summary, null, 2));
    test.info().annotations.push({
      type: "fidelity",
      description: `${summary.differingPercent}% of pixels differ (HTTP ${summary.status})`,
    });
  });
}
