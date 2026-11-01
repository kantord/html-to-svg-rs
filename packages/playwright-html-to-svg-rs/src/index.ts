// html-to-svg for Playwright: `snapshot(page)` turns a page into an SVG, and the test fixture
// `svgSnapshot(name)` does it for the page under test and attaches the SVG to the report.
//
// The capture runs in the page's own browser over CDP (Chromium and Chrome only); the conversion
// runs in this Node process, in the native addon of the generic npm package `html-to-svg-rs`.

import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { test as base, type BrowserContext, type Page } from "@playwright/test";
import { type Conversion, convert, styles } from "html-to-svg-rs";

import { capture, ResourceRecorder } from "./capture.js";

export { expect } from "@playwright/test";
export { type Conversion, convert, styles } from "html-to-svg-rs";
export { capture, ResourceRecorder } from "./capture.js";

export interface SnapshotOptions {
  /**
   * Keep the capture here: snapshot.json, chrome.png (the page as Chrome drew it), the fonts and
   * images it used, and out.svg. Without it, the capture goes to a temporary folder that is
   * removed afterwards.
   */
  dir?: string | undefined;
  /** Write images to this folder, shared by many snapshots, instead of inlining them. */
  assetsDir?: string | undefined;
  /**
   * The page's fonts and images as they were downloaded, recorded from before the page loaded
   * (`new ResourceRecorder(context)`). Without it, they are read from what Chrome keeps for the
   * page, which is the same except for images reached through a redirect.
   */
  recorder?: ResourceRecorder | undefined;
}

export interface SvgSnapshot extends Conversion {
  /** Where the capture was kept (`dir`), if it was. */
  dir?: string;
}

/**
 * Turns the current viewport of `page` (Chromium or Chrome) into an SVG: Chrome's layout is
 * captured, then converted. Scroll position, cookies and the page itself are left as they were.
 *
 *     const { svg } = await snapshot(page);
 */
export async function snapshot(page: Page, options: SnapshotOptions = {}): Promise<SvgSnapshot> {
  const dir = options.dir ?? (await mkdtemp(join(tmpdir(), "html-to-svg-")));
  try {
    await capture(page, dir, styles(), options.recorder);
    const conversion = await convert(dir, options.assetsDir === undefined ? {} : { assetsDir: options.assetsDir });
    if (!options.dir) return conversion;
    await writeFile(join(dir, "out.svg"), conversion.svg);
    return { ...conversion, dir };
  } finally {
    if (!options.dir) await rm(dir, { recursive: true, force: true });
  }
}

export interface HtmlToSvgOptions {
  /**
   * A folder shared by all snapshots: images are written there once, named by content hash
   * (`<sha256>.<ext>`), and the SVGs link to them by relative path instead of inlining them.
   * Unset (the default), every SVG is self-contained.
   */
  htmlToSvgAssetsDir: string | undefined;
}

export interface HtmlToSvgFixtures {
  /**
   * Captures the page's current viewport and converts it to SVG. Saves the capture under the
   * test's output directory and attaches the SVG to the test report as `<name>.svg`.
   */
  svgSnapshot: (name: string, page?: Page) => Promise<SvgSnapshot & { dir: string }>;
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
  htmlToSvgAssetsDir: [undefined, { option: true }],

  // Font and image responses are recorded from the moment the context exists, before any hook
  // or test navigates, so `svgSnapshot` can save their bodies whenever it is called. (A context
  // the test makes itself is recorded from its first `svgSnapshot` on.)
  context: async ({ context }, use) => {
    const recorder = recorderFor(context);
    await use(context);
    recorder.stop();
  },

  svgSnapshot: async ({ page, htmlToSvgAssetsDir }, use, testInfo) => {
    if (testInfo.project.use.browserName && testInfo.project.use.browserName !== "chromium") {
      throw new Error("svgSnapshot needs Chromium or Chrome (it reads the layout over CDP)");
    }
    await use(async (name, target = page) => {
      const dir = testInfo.outputPath("svg-snapshots", name);
      const result = await snapshot(target, {
        dir,
        assetsDir: htmlToSvgAssetsDir,
        recorder: recorderFor(target.context()),
      });
      await testInfo.attach(`${name}.svg`, { body: result.svg, contentType: "image/svg+xml" });
      return { ...result, dir };
    });
  },
});
