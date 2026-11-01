// Comparing two pictures the way wptrunner judges a reftest: identical, or different only within
// the test's `fuzzy` allowance.

import { PNG } from "pngjs";

import type { Fuzzy, Reftest } from "./manifest.ts";

/** Byte `i` of pixel data; out of range is a bug in the caller, not a black pixel. */
function at(data: Uint8Array, i: number): number {
  const v = data[i];
  if (v === undefined) throw new RangeError(`pixel byte ${i} of ${data.length}`);
  return v;
}

export interface Difference {
  /** The largest difference of any channel of any pixel, 0..255. */
  maxDifference: number;
  /** How many pixels differ at all. */
  differingPixels: number;
}

export function difference(a: Buffer, b: Buffer): Difference {
  const pa = PNG.sync.read(a);
  const pb = PNG.sync.read(b);
  if (pa.width !== pb.width || pa.height !== pb.height) {
    return { maxDifference: 255, differingPixels: Math.max(pa.width * pa.height, pb.width * pb.height) };
  }
  let maxDifference = 0;
  let differingPixels = 0;
  for (let i = 0; i < pa.data.length; i += 4) {
    let d = 0;
    for (let c = 0; c < 4; c++) d = Math.max(d, Math.abs(at(pa.data, i + c) - at(pb.data, i + c)));
    if (d > 0) {
      differingPixels++;
      maxDifference = Math.max(maxDifference, d);
    }
  }
  return { maxDifference, differingPixels };
}

/** The allowance for one reference: its own, else the test-wide one, else none. */
function allowance(test: Reftest, ref: string): Fuzzy | undefined {
  return test.fuzzy.find((f) => f.ref === ref) ?? test.fuzzy.find((f) => f.ref === null);
}

/** Whether two pictures count as "the same" for `==` / "different" for `!=`. */
export function looksSame(test: Reftest, ref: string, d: Difference): boolean {
  if (d.differingPixels === 0) return true;
  const fuzzy = allowance(test, ref);
  if (!fuzzy) return false;
  const within = (v: number, [min, max]: [number, number]) => v >= min && v <= max;
  return within(d.maxDifference, fuzzy.maxDifference) && within(d.differingPixels, fuzzy.totalPixels);
}

/**
 * WPT's rule (docs/writing-tests/reftests.md, "Multiple References"): if there are `==`
 * references, at least one must look the same; if there are `!=` references, all must look
 * different. (Chains, references that have references of their own, are not followed.)
 */
export function passes(test: Reftest, pictures: { ref: string; relation: "==" | "!="; d: Difference }[]) {
  const matches = pictures.filter((p) => p.relation === "==");
  const mismatches = pictures.filter((p) => p.relation === "!=");
  return (
    (matches.length === 0 || matches.some((p) => looksSame(test, p.ref, p.d))) &&
    mismatches.every((p) => !looksSame(test, p.ref, p.d))
  );
}

/** How our SVG differs from Chrome's picture of the same page (see `fidelity`). */
export interface Fidelity {
  /** Misplaced pixels outside the text layer (see `fidelity`, `textMask`). */
  outside: number;
  /** Misplaced pixels in the text layer. */
  text: number;
  /** Pixels outside the text layer that are not exactly equal: faint shading differences included. */
  exact: number;
  /**
   * Chrome's picture faded: red where outside text is misplaced, pink where it differs only
   * faintly or at an edge, orange where text is misplaced, pale orange at glyph edges.
   */
  picture: Buffer;
}

/**
 * The text layer: the pixels where Chrome's picture of the page differs from Chrome's picture of
 * the same page with its text made invisible (`HIDE_TEXT` in main.ts, which changes no layout),
 * grown by a pixel for the anti-aliasing at glyph edges. Glyphs, underlines, text shadows and
 * glyphs reaching past their line box are in it; the background between and under the glyphs is
 * not.
 */
export function textMask(chromePng: Buffer, textlessPng: Buffer): Uint8Array {
  const a = PNG.sync.read(chromePng);
  const b = PNG.sync.read(textlessPng);
  const { width, height } = a;
  const mask = new Uint8Array(width * height);
  if (b.width !== width || b.height !== height) return mask;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      if (a.data[i] === b.data[i] && a.data[i + 1] === b.data[i + 1] && a.data[i + 2] === b.data[i + 2]) continue;
      for (let dy = -1; dy <= 1; dy++) {
        const ny = y + dy;
        if (ny < 0 || ny >= height) continue;
        mask.fill(1, ny * width + Math.max(0, x - 1), ny * width + Math.min(width, x + 2));
      }
    }
  }
  return mask;
}

/**
 * Compares Chrome's screenshot with our SVG drawn by the same Chrome. A pixel is *misplaced* when
 * a channel differs by more than 32 and no pixel within 1 px of the other picture explains it:
 * content that is missing, extra or in the wrong place, as opposed to edges anti-aliased
 * differently (a rounded corner drawn by Chrome as a rounded rectangle and by our SVG as a path:
 * measured up to about 30 pixels on WPT's border-radius tests) and faint shading differences
 * (gradients, blur). Glyph edges always differ (our SVG draws outlines, Chrome draws from the
 * font: about a third of the pixels inside text boxes), so the text layer (`textMask`) and the
 * rest are counted apart.
 */
export function fidelity(chromePng: Buffer, svgPng: Buffer, inText: Uint8Array): Fidelity {
  const chrome = PNG.sync.read(chromePng);
  const svg = PNG.sync.read(svgPng);
  const { width, height } = chrome;
  const sameSize = svg.width === width && svg.height === height;
  const channel = (a: PNG, i: number, b: PNG, j: number) =>
    Math.max(
      Math.abs(at(a.data, i) - at(b.data, j)),
      Math.abs(at(a.data, i + 1) - at(b.data, j + 1)),
      Math.abs(at(a.data, i + 2) - at(b.data, j + 2)),
    );
  // Whether pixel (x, y) of `a` matches some pixel of `b` within 1 px.
  const near = (a: PNG, b: PNG, x: number, y: number) => {
    const i = (y * width + x) * 4;
    for (let dy = -1; dy <= 1; dy++) {
      for (let dx = -1; dx <= 1; dx++) {
        const nx = x + dx;
        const ny = y + dy;
        if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue;
        if (channel(a, i, b, (ny * width + nx) * 4) <= 32) return true;
      }
    }
    return false;
  };
  const out = new PNG({ width, height });
  let outside = 0;
  let text = 0;
  let exact = 0;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const p = y * width + x;
      const i = p * 4;
      const d = sameSize ? channel(chrome, i, svg, i) : 255;
      let rgb: [number, number, number];
      if (d === 0) {
        const grey = Math.round((at(chrome.data, i) + at(chrome.data, i + 1) + at(chrome.data, i + 2)) / 9 + 170);
        rgb = [grey, grey, grey];
      } else {
        const misplaced = !sameSize || (d > 32 && !near(chrome, svg, x, y) && !near(svg, chrome, x, y));
        if (inText[p]) {
          if (misplaced) text++;
          rgb = misplaced ? [255, 140, 0] : [255, 220, 170];
        } else {
          exact++;
          if (misplaced) outside++;
          rgb = misplaced ? [230, 0, 0] : [255, 170, 200];
        }
      }
      out.data.set([...rgb, 255], i);
    }
  }
  return { outside, text, exact, picture: PNG.sync.write(out) };
}
