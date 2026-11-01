// out/wpt/report.html: for every test with a capture under out/wpt/ (those that did not pass),
// Chrome's screenshot of the page, our SVG of it drawn by the same Chrome, and where they differ:
// red outside text, orange inside the text boxes Chrome reported (where glyph outlines and
// Chrome's font rasterizer are expected to differ at the edges), the rest faded.

import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { join, relative } from "node:path";

import { type Browser, chromium } from "@playwright/test";
import { PNG } from "pngjs";

import { readReftests } from "./manifest.ts";
import { load, type Entry } from "./results.ts";

interface Row {
  entry: Entry;
  dir: string;
  refDirs: string[];
  outsideText: number;
  insideText: number;
}

type Rect = [number, number, number, number];

/** Chrome's text boxes, grown by a pixel for anti-aliasing at their edges. */
function textBoxes(snapshot: { documents: { textBoxes?: { bounds?: number[][] } }[] }): Rect[] {
  return (snapshot.documents[0]?.textBoxes?.bounds ?? []).map(([x, y, w, h]) => [
    Math.floor(x) - 1,
    Math.floor(y) - 1,
    Math.ceil(x + w) + 1,
    Math.ceil(y + h) + 1,
  ]);
}

/** Draws our SVG in Chrome, writes svg.png and diff.png next to the capture; counts differences. */
async function compare(browser: Browser, dir: string) {
  const page = await browser.newPage({ viewport: { width: 800, height: 600 } });
  const svg = await readFile(join(dir, "out.svg"), "utf8");
  await page.setContent(`<body style="margin:0">${svg}</body>`);
  const drawnPng = await page.screenshot({ scale: "css" });
  await page.close();
  await writeFile(join(dir, "svg.png"), drawnPng);

  const chrome = PNG.sync.read(await readFile(join(dir, "chrome.png")));
  const drawn = PNG.sync.read(drawnPng);
  const boxes = textBoxes(JSON.parse(await readFile(join(dir, "snapshot.json"), "utf8")));
  const inText = (x: number, y: number) =>
    boxes.some(([l, t, r, b]) => x >= l && x < r && y >= t && y < b);
  const out = new PNG({ width: chrome.width, height: chrome.height });
  let outsideText = 0;
  let insideText = 0;
  for (let y = 0; y < chrome.height; y++) {
    for (let x = 0; x < chrome.width; x++) {
      const i = (y * chrome.width + x) * 4;
      const same =
        drawn.data[i] === chrome.data[i] &&
        drawn.data[i + 1] === chrome.data[i + 1] &&
        drawn.data[i + 2] === chrome.data[i + 2];
      let rgb: [number, number, number];
      if (same) {
        const grey = Math.round((chrome.data[i] + chrome.data[i + 1] + chrome.data[i + 2]) / 3 / 3 + 170);
        rgb = [grey, grey, grey];
      } else if (inText(x, y)) {
        insideText++;
        rgb = [255, 160, 0];
      } else {
        outsideText++;
        rgb = [230, 0, 0];
      }
      out.data.set([...rgb, 255], i);
    }
  }
  await writeFile(join(dir, "diff.png"), PNG.sync.write(out));
  return { outsideText, insideText };
}

const escape = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

export async function writeReport(repo: string, out: string, chromeArgs: string[]): Promise<string> {
  const entries = await load(join(repo, "wpt/results.jsonl"));
  const refsOf = new Map((await readReftests("/wpt/MANIFEST.json")).map((t) => [t.url, t.refs]));
  const folder = (url: string, role: "test" | "ref") => {
    const path = url.replace(/^\//, "").replace(/[?#]/g, "_");
    return role === "test" ? join(out, path) : join(out, "refs", path);
  };

  const browser = await chromium.launch({ args: chromeArgs });
  const rows: Row[] = [];
  for (const entry of entries.values()) {
    const dir = folder(entry.test, "test");
    if (!existsSync(join(dir, "out.svg"))) continue;
    const refDirs = (refsOf.get(entry.test) ?? [])
      .map((r) => folder(r.url, "ref"))
      .filter((d) => existsSync(join(d, "chrome.png")));
    rows.push({ entry, dir, refDirs, ...(await compare(browser, dir)) });
  }
  await browser.close();
  // Most differences outside text first: those are boxes, borders, images in the wrong place.
  rows.sort((a, b) => b.outsideText - a.outsideText || b.insideText - a.insideText);

  const img = (path: string, alt: string) =>
    `<figure><img src="${escape(relative(out, path))}" alt="${escape(alt)}" loading="lazy" width="400" height="300"><figcaption>${escape(alt)}</figcaption></figure>`;
  const total = 800 * 600;
  const pct = (n: number) => `${((n / total) * 100).toFixed(2)}%`;
  const items = rows
    .map(
      (r) => `<section id="${escape(r.entry.test)}">
  <h2><a href="http://wpt.live${escape(r.entry.test)}">${escape(r.entry.test)}</a></h2>
  <p class="meta"><span class="status ${r.entry.status}">${r.entry.status}</span>
    differing pixels outside text <b>${r.outsideText}</b> (${pct(r.outsideText)}),
    inside text boxes <b>${r.insideText}</b> (${pct(r.insideText)})${r.entry.note ? ` · ${escape(r.entry.note)}` : ""}</p>
  <div class="row">
    ${img(join(r.dir, "chrome.png"), "Chrome: the page")}
    ${img(join(r.dir, "svg.png"), "our SVG, drawn by Chrome")}
    ${img(join(r.dir, "diff.png"), "difference: red outside text, orange in text")}
    ${r.refDirs.map((d) => img(join(d, "chrome.png"), "Chrome: the reference")).join("")}
  </div>
</section>`,
    )
    .join("\n");

  const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>WPT report</title>
<style>
  :root { color-scheme: light dark; --bg: #fafafa; --fg: #1d1d1f; --muted: #6e6e73; --line: #e2e2e7; }
  @media (prefers-color-scheme: dark) { :root { --bg: #161617; --fg: #f2f2f4; --muted: #a1a1a6; --line: #333336; } }
  body { margin: 0 auto; max-width: 1720px; padding: 24px 16px 64px; background: var(--bg); color: var(--fg);
         font: 14px/1.45 system-ui, sans-serif; }
  h1 { font-size: 22px; margin: 0 0 4px; }
  .lede { color: var(--muted); margin: 0 0 24px; max-width: 80ch; }
  section { border-top: 1px solid var(--line); padding: 16px 0; }
  h2 { font-size: 14px; font-family: ui-monospace, monospace; margin: 0 0 4px; word-break: break-all; }
  h2 a { color: inherit; }
  .meta { color: var(--muted); margin: 0 0 10px; }
  .meta b { color: var(--fg); font-variant-numeric: tabular-nums; }
  .status { display: inline-block; padding: 1px 8px; border-radius: 99px; background: var(--line); color: var(--fg);
            font-size: 12px; margin-right: 6px; }
  .row { display: flex; gap: 12px; overflow-x: auto; }
  figure { margin: 0; flex: none; }
  img { display: block; width: 400px; height: 300px; border: 1px solid var(--line); background: white;
        image-rendering: pixelated; cursor: zoom-in; }
  img.zoom { width: 800px; height: 600px; cursor: zoom-out; }
  figcaption { color: var(--muted); font-size: 12px; margin-top: 4px; }
</style>
</head>
<body>
<h1>WPT report</h1>
<p class="lede">${rows.length} tests that did not pass, most differences outside text first. Each row:
Chrome's screenshot of the test page, our SVG of the same page drawn by the same Chrome, where the two
differ (red: outside text; orange: inside the text boxes Chrome reported, where glyph outlines are
expected to differ at the edges), and Chrome's screenshot of the reference. Click a picture to see it
at full size.</p>
${items}
<script>
  document.addEventListener("click", (e) => {
    if (e.target instanceof HTMLImageElement) e.target.classList.toggle("zoom");
  });
</script>
</body>
</html>
`;
  const path = join(out, "report.html");
  await writeFile(path, html);
  return path;
}
