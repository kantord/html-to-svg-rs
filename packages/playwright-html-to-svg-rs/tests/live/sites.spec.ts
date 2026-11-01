// Proof of concept on live sites: each test opens a real page, takes an SVG snapshot, and keeps
// Chrome's screenshot (page.png), our SVG (page.svg), our SVG drawn by resvg (svg.png) and where
// they differ (diff.png) in out/live/<site>/, with the numbers in summary.json.
//
// Not part of `pnpm test`: the pages need the network and change every day. Run with
// `just live-snapshots` (in the test container; it also writes out/live/index.html). A test fails
// only when no SVG comes out; how close it is to Chrome is recorded, not asserted.

import { execFile } from "node:child_process";
import { copyFile, mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";

import { expect, test } from "../../src/index.js";

const run = promisify(execFile);
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
  test(site.name, async ({ page, svgSnapshot, htmlToSvgBinary }) => {
    test.setTimeout(120_000);
    const response = await page.goto(site.url, { waitUntil: "load", timeout: 60_000 });
    // Let late content (client rendering, lazy images, web fonts) arrive; a page that keeps
    // the network busy (ads, analytics) gets 10 s.
    await page.waitForLoadState("networkidle", { timeout: 10_000 }).catch(() => {});
    await page.waitForTimeout(1_000);

    const { svg, report, dir } = await svgSnapshot(site.name);
    expect(svg.startsWith("<svg")).toBe(true);

    // How far our SVG (drawn by resvg, an independent renderer) is from Chrome's screenshot.
    const { stdout } = await run(htmlToSvgBinary, ["diff", dir]);
    const line = stdout.split("\n").find((l) => l.trim().startsWith("svg vs chrome")) ?? "";
    const number = (label: string) => Number(new RegExp(`${label} ([\\d.]+)%`).exec(line)?.[1] ?? NaN);

    const out = join(OUT, site.name);
    await mkdir(out, { recursive: true });
    await copyFile(join(dir, "chrome.png"), join(out, "page.png"));
    await copyFile(join(dir, "out.svg"), join(out, "page.svg"));
    await copyFile(join(dir, "svg-via-resvg.png"), join(out, "svg.png"));
    await copyFile(join(dir, "diff-svg.png"), join(out, "diff.png"));
    const summary = {
      name: site.name,
      url: site.url,
      status: response?.status() ?? null,
      date: new Date().toISOString(),
      svgBytes: Buffer.byteLength(svg),
      visiblyDifferentPercent: number("visibly different pixels"),
      misplacedPercent: number("misplaced"),
      boxes: report.report.boxes,
      textLines: report.report.text_lines,
      fontsMissing: report.fonts.missing,
      rasterFallbacks: report.report.raster_fallbacks.length,
      dropped: Object.keys(report.report.dropped).length,
      warnings: report.report.warnings,
    };
    await writeFile(join(out, "summary.json"), JSON.stringify(summary, null, 2));
    test.info().annotations.push({
      type: "fidelity",
      description: `${summary.misplacedPercent}% misplaced, ${summary.visiblyDifferentPercent}% visibly different (HTTP ${summary.status})`,
    });
  });
}
