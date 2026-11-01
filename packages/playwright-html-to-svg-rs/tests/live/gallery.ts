// out/live/index.html: every site of the last `just live-snapshots`, Chrome's screenshot next to
// our SVG (the SVG file itself, drawn by the browser viewing the page) and the difference.

import { readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

const OUT = new URL("../../../../out/live/", import.meta.url).pathname;

interface Summary {
  name: string;
  url: string;
  status: number | null;
  date: string;
  svgBytes: number;
  visiblyDifferentPercent: number;
  misplacedPercent: number;
  boxes: number;
  textLines: number;
  fontsMissing: string[];
  rasterFallbacks: number;
  dropped: number;
  warnings: string[];
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/"/g, "&quot;");

const sites: Summary[] = [];
for (const name of (await readdir(OUT, { withFileTypes: true })).filter((d) => d.isDirectory())) {
  try {
    sites.push(JSON.parse(await readFile(join(OUT, name.name, "summary.json"), "utf8")));
  } catch {
    // A site whose test failed has no summary.
  }
}
sites.sort((a, b) => a.name.localeCompare(b.name));

const figure = (src: string, caption: string) =>
  `<figure><a href="${esc(src)}"><img src="${esc(src)}" alt="${esc(caption)}" loading="lazy"></a><figcaption>${esc(caption)}</figcaption></figure>`;

const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Live snapshots</title>
<style>
  :root { color-scheme: light dark; --bg: #fafafa; --fg: #1d1d1f; --muted: #6e6e73; --line: #e2e2e7; }
  @media (prefers-color-scheme: dark) { :root { --bg: #161617; --fg: #f2f2f4; --muted: #a1a1a6; --line: #333336; } }
  body { margin: 0 auto; max-width: 1800px; padding: 24px 16px 64px; background: var(--bg); color: var(--fg);
         font: 14px/1.45 system-ui, sans-serif; }
  h1 { font-size: 22px; margin: 0 0 4px; }
  .lede { color: var(--muted); margin: 0 0 16px; max-width: 90ch; }
  section { border-top: 1px solid var(--line); padding: 16px 0; }
  h2 { font-size: 16px; margin: 0 0 4px; }
  h2 a { color: inherit; }
  .meta { color: var(--muted); margin: 0 0 10px; }
  .meta b { color: var(--fg); font-variant-numeric: tabular-nums; }
  .row { display: flex; gap: 12px; overflow-x: auto; }
  figure { margin: 0; flex: none; }
  img { display: block; width: 512px; height: 320px; object-fit: contain; object-position: top left;
        border: 1px solid var(--line); background: white; }
  figcaption { color: var(--muted); font-size: 12px; margin-top: 4px; }
  details { color: var(--muted); margin-top: 6px; }
</style>
</head>
<body>
<h1>Live snapshots</h1>
<p class="lede">Real pages at 1280x800, captured by Chrome and converted by html-to-svg. Per site: Chrome's screenshot,
our SVG file as your browser draws it, and where our SVG (drawn by resvg) differs from Chrome's screenshot
(orange: anti-aliasing or a sub-pixel shift; red: misplaced). Click a picture for the full size.</p>
${sites
  .map(
    (s) => `<section>
  <h2><a href="${esc(s.url)}">${esc(s.name)}</a></h2>
  <p class="meta">HTTP ${s.status ?? "?"} · misplaced <b>${s.misplacedPercent}%</b> · visibly different <b>${s.visiblyDifferentPercent}%</b>
    · ${s.boxes} boxes, ${s.textLines} text lines · ${s.rasterFallbacks} cropped from the screenshot
    · SVG ${(s.svgBytes / 1024).toFixed(0)} KB · ${esc(s.date.slice(0, 16).replace("T", " "))}</p>
  <div class="row">
    ${figure(`${s.name}/page.png`, "Chrome: page.png")}
    ${figure(`${s.name}/page.svg`, "ours: page.svg")}
    ${figure(`${s.name}/diff.png`, "difference")}
  </div>
  ${
    s.fontsMissing.length || s.warnings.length || s.dropped
      ? `<details><summary>${s.fontsMissing.length} fonts not found, ${s.warnings.length} warnings, ${s.dropped} kinds of declarations dropped</summary>
    <p>${[...s.fontsMissing.map((f) => `font not found: ${f}`), ...s.warnings].map(esc).join("<br>")}</p></details>`
      : ""
  }
</section>`,
  )
  .join("\n")}
</body>
</html>
`;
await writeFile(join(OUT, "index.html"), html);
console.log(`${sites.length} sites: ${join(OUT, "index.html")}`);
