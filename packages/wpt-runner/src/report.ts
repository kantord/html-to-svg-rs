// out/wpt/report.html, from what the last run left: for every test with a capture (those that did
// not pass), Chrome's picture of the page, our SVG drawn by the same Chrome, where they differ
// (diff.png, made by `fidelity` in compare.ts), and the gate's screenshots of the references.

import { existsSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { join, relative } from "node:path";

import { type Bug, loadBugs } from "./bugs.ts";
import { readReftests } from "./manifest.ts";
import { refPicture, testFolder } from "./paths.ts";
import { load, scoreOf, type Entry } from "./results.ts";

const escape = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

export async function writeReport(repo: string, out: string): Promise<string> {
  const entries = await load(join(repo, "wpt/results.jsonl"));
  const manifest = "/wpt/MANIFEST.json";
  const refsOf = new Map(
    existsSync(manifest) ? (await readReftests(manifest)).map((t) => [t.url, t.refs.map((r) => r.url)]) : [],
  );

  const bugs = await loadBugs(repo);
  const rows: Entry[] = [...entries.values()].filter((e) => existsSync(join(testFolder(e.test), "diff.png")));
  // Grouped: not triaged yet, then each bug, then not planned; within a group, most differences
  // outside text first (boxes, borders, backgrounds, images in the wrong place).
  const group = (e: Entry) =>
    e.status === "fail" ? "0" : e.status === "bug" ? `1 ${e.bug}` : e.status === "not-planned" ? "2" : `3 ${e.status}`;
  rows.sort(
    (a, b) =>
      group(a).localeCompare(group(b)) ||
      (scoreOf(b)?.outside ?? 0) - (scoreOf(a)?.outside ?? 0) ||
      (scoreOf(b)?.text ?? 0) - (scoreOf(a)?.text ?? 0),
  );
  const heading = (e: Entry, bug: Bug | undefined) =>
    e.status === "fail"
      ? "Not triaged yet"
      : e.status === "bug"
        ? `Bug <a href="${escape(relative(out, join(repo, "wpt/bugs", `${e.bug}.md`)))}">${escape(e.bug)}</a>: ${escape(bug?.title ?? "(no file)")}`
        : e.status === "not-planned"
          ? "Not planned"
          : escape(e.status);
  const counts = new Map<string, number>();
  for (const e of entries.values()) counts.set(e.status, (counts.get(e.status) ?? 0) + 1);

  const img = (path: string, caption: string) =>
    `<figure><img src="${escape(relative(out, path))}" alt="${escape(caption)}" loading="lazy" width="400" height="300"><figcaption>${escape(caption)}</figcaption></figure>`;
  let lastGroup = "";
  const items = rows
    .map((e) => {
      const g = group(e);
      const title = g === lastGroup ? "" : `<h3 class="group">${heading(e, e.status === "bug" ? bugs.get(e.bug) : undefined)} (${rows.filter((r) => group(r) === g).length})</h3>`;
      lastGroup = g;
      const dir = testFolder(e.test);
      const refs = (refsOf.get(e.test) ?? []).map(refPicture).filter((p) => existsSync(p));
      return `${title}<section>
  <h2><a href="http://wpt.live${escape(e.test)}">${escape(e.test)}</a></h2>
  <p class="meta"><span class="status">${e.status}</span>
    misplaced outside text <b>${scoreOf(e)?.outside ?? "?"}</b> · inside text <b>${scoreOf(e)?.text ?? "?"}</b> · not exactly equal outside text ${scoreOf(e)?.exact ?? "?"}${e.lastPass ? ` · last passed ${escape(e.lastPass.date)} at ${escape(e.lastPass.version)}` : ""}${"note" in e ? ` · ${escape(e.note)}` : ""}</p>
  <div class="row">
    ${img(join(dir, "chrome.png"), "Chrome: the page")}
    ${img(join(dir, "svg.png"), "our SVG, drawn by the same Chrome")}
    ${img(join(dir, "diff.png"), "difference")}
    ${refs.map((p) => img(p, "Chrome: the reference (WPT's flags)")).join("")}
  </div>
</section>`;
    })
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
  .lede { color: var(--muted); margin: 0 0 8px; max-width: 90ch; }
  .key span { display: inline-block; width: 12px; height: 12px; vertical-align: -2px; margin: 0 4px 0 12px; border: 1px solid var(--line); }
  section { border-top: 1px solid var(--line); padding: 16px 0; }
  h3.group { font-size: 17px; margin: 32px 0 0; padding-top: 16px; border-top: 3px solid var(--fg); }
  h3.group a { color: inherit; }
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
<p class="lede">${[...counts].map(([k, v]) => `${v} ${k}`).join(" · ")}. Below, the ${rows.length} tests whose
comparison did not pass, most differences outside text first: Chrome's picture of the page, our SVG of it
drawn by the same Chrome, where they differ, and Chrome's screenshot of the reference; grouped by triage:
not triaged yet, each tracked bug, not planned. Click a picture to see
it at full size.</p>
<p class="lede key">Difference:<span style="background:#e60000"></span>misplaced, outside text
<span style="background:#ffaac8"></span>faint or edge only, outside text (tolerated)
<span style="background:#ff8c00"></span>misplaced, inside text
<span style="background:#ffdcaa"></span>glyph edges, inside text (tolerated)
<span style="background:#ccc"></span>identical</p>
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
