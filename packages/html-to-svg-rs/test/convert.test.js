// The Node.js bindings, on captures committed for the Rust regression tests (no fonts needed).
const assert = require("node:assert/strict");
const { existsSync, mkdtempSync, readdirSync, rmSync } = require("node:fs");
const { tmpdir } = require("node:os");
const { join } = require("node:path");
const test = require("node:test");

const { convert, styles } = require("..");

const FIXTURES = join(__dirname, "../../../crates/html-to-svg-rs/tests/regressions");

test("styles() lists the computed styles a capture asks for", () => {
  const list = styles();
  assert.ok(list.length > 50);
  for (const name of ["display", "background-clip", "contain", "corner-top-left-shape"]) {
    assert.ok(list.includes(name), name);
  }
});

test("convert() turns a capture into an SVG, a PNG and a report", async () => {
  const result = convert(join(FIXTURES, "xhtml-canvas"));
  assert.ok(result instanceof Promise, "runs off the main thread: returns a Promise");
  const { svg, png, report, fonts, assets } = await result;

  assert.ok(svg.startsWith("<svg"));
  assert.deepEqual([...png.subarray(0, 4)], [0x89, 0x50, 0x4e, 0x47], "PNG signature");
  assert.ok(report.boxes > 0);
  assert.deepEqual(report.rasterFallbacks.map((f) => f.split(" ")[0]), ["<canvas>"]);
  assert.deepEqual(report.warnings, []);
  assert.deepEqual(fonts.missing, []);
  assert.deepEqual(assets, []);
});

test("convert() keeps the page's data-* attributes", async () => {
  const { svg } = await convert(join(FIXTURES, "duplicate-data-node"));
  assert.match(svg, /data-testid="card"/);
  assert.match(svg, /data-node="from-the-page"/);
  assert.match(svg, /data-html-to-svg-node="\d+"/);
});

test("convert() writes images to a shared assets folder when asked", async () => {
  const dir = mkdtempSync(join(tmpdir(), "html-to-svg-node-"));
  try {
    const assetsDir = join(dir, "assets");
    const { svg, assets } = await convert(join(FIXTURES, "xhtml-canvas"), { assetsDir, svgDir: dir });
    assert.equal(assets.length, 1);
    assert.match(assets[0], /^[0-9a-f]{64}\.png$/);
    assert.deepEqual(readdirSync(assetsDir), assets);
    assert.ok(svg.includes(`href="assets/${assets[0]}"`), "linked by the path from svgDir");
    assert.ok(!svg.includes("data:image/png"));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("convert() rejects with the reason when the capture is missing", async () => {
  const missing = join(tmpdir(), "html-to-svg-no-such-capture");
  assert.ok(!existsSync(missing));
  await assert.rejects(convert(missing), /snapshot\.json/);
});
