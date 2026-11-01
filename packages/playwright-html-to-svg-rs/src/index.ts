// Playwright fixture: `svgSnapshot(name)` turns the page under test into an SVG.
//
// The capture runs in the test's own browser over CDP (Chromium and Chrome only); the conversion
// runs the `html-to-svg` binary of this repository (`crates/html-to-svg-cli`) on the capture.

import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";

import { test as base, type BrowserContext, type Page } from "@playwright/test";

import { capture, ResourceRecorder } from "./capture.js";

export { expect } from "@playwright/test";
export { capture, ResourceRecorder } from "./capture.js";

const run = promisify(execFile);

/** What `html-to-svg convert --json` reports about one conversion. */
export interface ConversionReport {
  fonts: {
    /** Faces Chrome used that were found and registered. */
    faces: number;
    /** Faces Chrome used that could not be found (the text falls back to another face). */
    missing: string[];
  };
  report: {
    boxes: number;
    text_lines: number;
    /** Elements whose pixels were cropped from Chrome's screenshot (`<img>`, `<canvas>`, ...). */
    raster_fallbacks: string[];
    /** CSS declarations the painter rejected, with how often each was dropped. */
    dropped: Record<string, number>;
    /** Things that were not drawn as Chrome drew them, e.g. an image that was not captured. */
    warnings: string[];
    /** |Chrome line width - SVG line width| per text line, in px, before and after fitting. */
    line_width_error: number[];
    line_width_error_fitted: number[];
  };
}

export interface SvgSnapshot {
  /** The SVG. Elements with `data-*` attributes keep them on the groups they painted. */
  svg: string;
  /** What the conversion found, dropped and warned about. */
  report: ConversionReport;
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

/** Converts a capture directory with the binary. */
export async function convert(
  binary: string,
  dir: string,
): Promise<{ svg: string; report: ConversionReport }> {
  const { stdout } = await run(binary, ["convert", dir, "--json"], {
    maxBuffer: 64 * 1024 * 1024,
  });
  const report = JSON.parse(stdout) as ConversionReport;
  return { svg: await readFile(join(dir, "out.svg"), "utf8"), report };
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

/** One recorder per browser context, started with the context (see the `context` fixture). */
const recorders = new WeakMap<BrowserContext, ResourceRecorder>();

function recorderFor(context: BrowserContext): ResourceRecorder {
  let recorder = recorders.get(context);
  if (!recorder) {
    recorder = new ResourceRecorder(context);
    recorders.set(context, recorder);
  }
  return recorder;
}

export const test = base.extend<HtmlToSvgFixtures & HtmlToSvgOptions>({
  htmlToSvgBinary: [process.env.HTML_TO_SVG_BIN ?? "html-to-svg", { option: true }],

  // Font and image responses are recorded from the moment the context exists, before any hook
  // or test navigates, so `svgSnapshot` can save their bodies whenever it is called. (A context
  // the test makes itself is recorded from its first `svgSnapshot` on.)
  context: async ({ context }, use) => {
    const recorder = recorderFor(context);
    await use(context);
    recorder.stop();
  },

  svgSnapshot: async ({ page, htmlToSvgBinary }, use, testInfo) => {
    if (testInfo.project.use.browserName && testInfo.project.use.browserName !== "chromium") {
      throw new Error("svgSnapshot needs Chromium or Chrome (it reads the layout over CDP)");
    }
    await use(async (name, target = page) => {
      const recorder = recorderFor(target.context());
      const dir = testInfo.outputPath("svg-snapshots", name);
      await capture(target, dir, await styles(htmlToSvgBinary), recorder);
      const { svg, report } = await convert(htmlToSvgBinary, dir);
      await testInfo.attach(`${name}.svg`, { body: svg, contentType: "image/svg+xml" });
      return { svg, report, dir };
    });
  },
});
