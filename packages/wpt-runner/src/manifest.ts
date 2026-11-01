// The reftests of a web-platform-tests MANIFEST.json (format version 9).
//
// `items.reftest` is a tree of folders whose leaves are files:
//   "name.html": [hash, [url | null, [[refUrl, "==" | "!="], ...], {fuzzy?, timeout?}], ...]
// One file can hold several tests (variants); a null url means the file's own path.

import { readFile } from "node:fs/promises";

export interface Fuzzy {
  /** The reference this allowance is for; null for every reference. */
  ref: string | null;
  /** How much one pixel's channel may differ, as an inclusive [min, max]. */
  maxDifference: [number, number];
  /** How many pixels may differ, as an inclusive [min, max]. */
  totalPixels: [number, number];
}

export interface Reftest {
  /** The test's URL path, e.g. `/css/css-borders/border-radius-001.html`. */
  url: string;
  /** References: the test must look the same as (`==`), or different from (`!=`), one of them. */
  refs: { url: string; relation: "==" | "!=" }[];
  fuzzy: Fuzzy[];
  /** `timeout: long` in the test's metadata. */
  long: boolean;
}

type RawFuzzy = [null | [string, string] | string, [[number, number], [number, number]]];
type RawTest = [string | null, [string, "==" | "!="][], { fuzzy?: RawFuzzy[]; timeout?: string }];
type RawTree = { [name: string]: RawTree | [string, ...RawTest[]] };

export async function readReftests(manifestPath: string): Promise<Reftest[]> {
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  if (manifest.version !== 9) {
    throw new Error(`unknown MANIFEST.json version ${manifest.version} (this runner reads 9)`);
  }
  const tests: Reftest[] = [];
  const walk = (node: RawTree, path: string) => {
    for (const [name, value] of Object.entries(node)) {
      if (!Array.isArray(value)) {
        walk(value, `${path}${name}/`);
        continue;
      }
      const [, ...variants] = value;
      for (const [url, refs, extras] of variants) {
        tests.push({
          // Variants (`?query`) come without the leading slash.
          url: url ? `/${url.replace(/^\/+/, "")}` : `/${path}${name}`,
          refs: refs.map(([ref, relation]) => ({ url: ref, relation })),
          fuzzy: (extras.fuzzy ?? []).map(([key, [diff, pixels]]) => ({
            ref: Array.isArray(key) ? key[1] : key,
            maxDifference: diff,
            totalPixels: pixels,
          })),
          long: extras.timeout === "long",
        });
      }
    }
  };
  walk(manifest.items.reftest ?? {}, "");
  return tests.sort((a, b) => a.url.localeCompare(b.url));
}
