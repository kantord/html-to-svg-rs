// Comparing two pictures the way wptrunner judges a reftest: identical, or different only within
// the test's `fuzzy` allowance.

import { PNG } from "pngjs";

import type { Fuzzy, Reftest } from "./manifest.ts";

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
    for (let c = 0; c < 4; c++) d = Math.max(d, Math.abs(pa.data[i + c] - pb.data[i + c]));
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
  /** Misplaced pixels outside text boxes (see `fidelity`). */
  outside: number;
  /** Misplaced pixels inside text boxes. */
  text: number;
  /** Pixels outside text boxes that are not exactly equal: faint shading differences included. */
  exact: number;
  /**
   * Chrome's picture faded: red where outside text is misplaced, pink where it differs only
   * faintly or at an edge, orange where text is misplaced, pale orange at glyph edges.
   */
  picture: Buffer;
}

/** Text boxes `[left, top, right, bottom)` in pixels. */
export type Box = [number, number, number, number];

/**
 * Compares Chrome's screenshot with our SVG drawn by the same Chrome. A pixel is *misplaced* when
 * a channel differs by more than 32 and no pixel within 1 px of the other picture explains it:
 * content that is missing, extra or in the wrong place, as opposed to edges anti-aliased
 * differently (a rounded corner drawn by Chrome as a rounded rectangle and by our SVG as a path:
 * measured up to about 30 pixels on WPT's border-radius tests) and faint shading differences
 * (gradients, blur). Glyph edges inside text boxes always differ (our SVG draws outlines, Chrome
 * draws from the font: about a third of the pixels in text boxes), so text and the rest are
 * counted apart.
 */
export function fidelity(chromePng: Buffer, svgPng: Buffer, boxes: Box[]): Fidelity {
  const chrome = PNG.sync.read(chromePng);
  const svg = PNG.sync.read(svgPng);
  const { width, height } = chrome;
  const sameSize = svg.width === width && svg.height === height;
  const inText = new Uint8Array(width * height);
  for (const [l, t, r, b] of boxes) {
    for (let y = Math.max(0, t); y < Math.min(height, b); y++) {
      inText.fill(1, y * width + Math.max(0, l), y * width + Math.min(width, r));
    }
  }
  const channel = (a: PNG, i: number, b: PNG, j: number) =>
    Math.max(
      Math.abs(a.data[i] - b.data[j]),
      Math.abs(a.data[i + 1] - b.data[j + 1]),
      Math.abs(a.data[i + 2] - b.data[j + 2]),
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
        const grey = Math.round((chrome.data[i] + chrome.data[i + 1] + chrome.data[i + 2]) / 9 + 170);
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
