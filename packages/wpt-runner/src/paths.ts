// Where the runner keeps things.

import { join } from "node:path";

export const REPO = new URL("../../../", import.meta.url).pathname;
export const RESULTS = join(REPO, "wpt/results.jsonl");
export const OUT = join(REPO, "out/wpt");

const safe = (url: string) => url.replace(/^\//, "").replace(/[?#]/g, "_");

/** A test's capture: snapshot.json, chrome.png, out.svg, svg.png (our SVG drawn), diff.png. */
export function testFolder(url: string): string {
  return join(OUT, safe(url));
}

/** The gate's screenshot of a reference. */
export function refPicture(url: string): string {
  return join(OUT, "refs", `${safe(url)}.png`);
}
