// Playwright fixture: `svgSnapshot(name)` turns the page under test into an SVG.
//
// The capture runs in the test's own browser over CDP (Chromium and Chrome only); the conversion
// runs the `html-to-svg` binary of this repository (`crates/html-to-svg-cli`) on the capture.

import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";

import { test as base, type Page } from "@playwright/test";

import { capture, ResourceRecorder } from "./capture.js";

export { expect } from "@playwright/test";
export { capture, ResourceRecorder } from "./capture.js";

const run = promisify(execFile);

export interface SvgSnapshot {
  /** The SVG. Elements with `data-*` attributes keep them on the groups they painted. */
  svg: string;
  /** The capture directory: snapshot.json, chrome.png (the page as Chrome drew it), out.svg. */
  dir: string;
}

export interface HtmlToSvgOptions {
  /**
   * Path of the `html-to-svg` binary. Defaults to `$HTML_TO_SVG_BIN`, else `html-to-svg` on PATH.
   */
  htmlToSvgBinary: string;
}

export interface HtmlToSvgFixtures {
  /**
   * Captures the page's current viewport and converts it to SVG. Saves the capture under the
   * test's output directory and attaches the SVG to the test report as `<name>.svg`.
   */
  svgSnapshot: (name: string, page?: Page) => Promise<SvgSnapshot>;
}

/** Converts a capture directory with the binary; returns the SVG. */
export async function convert(binary: string, dir: string): Promise<string> {
  await run(binary, ["convert", dir], { maxBuffer: 64 * 1024 * 1024 });
  return readFile(join(dir, "out.svg"), "utf8");
}

const stylesByBinary = new Map<string, Promise<string[]>>();

/** The computed styles a capture asks Chrome for, as the binary's conversion expects them. */
export function styles(binary: string): Promise<string[]> {
  let found = stylesByBinary.get(binary);
  if (!found) {
    found = run(binary, ["styles"]).then(({ stdout }) => JSON.parse(stdout) as string[]);
    stylesByBinary.set(binary, found);
  }
  return found;
}

export const test = base.extend<HtmlToSvgFixtures & HtmlToSvgOptions>({
  htmlToSvgBinary: [process.env.HTML_TO_SVG_BIN ?? "html-to-svg", { option: true }],

  svgSnapshot: async ({ page, htmlToSvgBinary }, use, testInfo) => {
    if (testInfo.project.use.browserName && testInfo.project.use.browserName !== "chromium") {
      throw new Error("svgSnapshot needs Chromium or Chrome (it reads the layout over CDP)");
    }
    // Font and image responses are recorded from the start of the test, before navigation,
    // so their bodies can be saved with the capture.
    const recorders = new Map<Page, ResourceRecorder>([[page, new ResourceRecorder(page)]]);
    await use(async (name, target = page) => {
      const recorder = recorders.get(target) ?? new ResourceRecorder(target);
      recorders.set(target, recorder);
      const dir = testInfo.outputPath("svg-snapshots", name);
      await capture(target, dir, await styles(htmlToSvgBinary), recorder);
      const svg = await convert(htmlToSvgBinary, dir);
      await testInfo.attach(`${name}.svg`, { body: svg, contentType: "image/svg+xml" });
      return { svg, dir };
    });
    for (const recorder of recorders.values()) recorder.stop();
  },
});
