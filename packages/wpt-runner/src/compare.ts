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
