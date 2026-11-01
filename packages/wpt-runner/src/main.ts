// Runs web-platform-tests reftests through html-to-svg and keeps wpt/results.jsonl.
//
//   node src/main.ts run [--binary B] [--jobs N] [--keep-passing] [path-prefix...]
//   node src/main.ts check
//
// `run` expects the pinned checkout served at http://web-platform.test:8000/ (wpt/serve.sh, in
// the test container: `just wpt`). For each reftest and each of its references it opens the page
// in Chromium at 800x600, captures it and converts it to SVG, then judges twice:
//   1. Chrome's screenshot of the test against Chrome's screenshot of the reference. If Chrome
//      fails, the test is `chrome-fails`: it says nothing about the conversion.
//   2. Our SVG of the test against our SVG of the reference, both drawn by Chrome at 800x600.
// Captures of tests that do not pass stay under out/wpt/<test path>/ (their references under
// out/wpt/refs/), ready to become a Rust regression test.
//
// `check` only verifies that every `regression-test` entry links to an existing Rust test.

import { execFile } from "node:child_process";
import { mkdir, readFile, rm } from "node:fs/promises";
import { cpus } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

import { type Browser, type BrowserContext, chromium, type Page } from "@playwright/test";
import { capture, convert, ResourceRecorder, styles } from "playwright-html-to-svg-rs";

import { difference, passes, type Difference } from "./compare.ts";
import { readReftests, type Reftest } from "./manifest.ts";
import { type Change, danglingLinks, load, type Outcome, record, save } from "./results.ts";

const REPO = new URL("../../../", import.meta.url).pathname;
const RESULTS = join(REPO, "wpt/results.jsonl");
const OUT = join(REPO, "out/wpt");
const ORIGIN = "http://web-platform.test:8000";
const VIEWPORT = { width: 800, height: 600 };
/**
 * The flags of WPT's own Chrome runs that change what is drawn (the pinned checkout's
 * tools/wptrunner/wptrunner/browsers/chrome.py): without them Chrome "fails" tests it passes
 * upstream, e.g. by anti-aliasing a test's glyphs differently from its reference's.
 */
const WPT_CHROME_ARGS = [
  "--enable-blink-features=NoFontAntialiasing",
  "--disable-features=ScrollbarAnimations",
  "--enable-experimental-web-platform-features",
];

interface Args {
  command: string;
  binary: string;
  jobs: number;
  keepPassing: boolean;
  prefixes: string[];
}

function parseArgs(argv: string[]): Args {
  const args: Args = {
    command: argv[0] ?? "",
    binary: process.env.HTML_TO_SVG_BIN ?? "html-to-svg",
    jobs: Math.max(1, Math.floor(cpus().length / 2)),
    keepPassing: false,
    prefixes: [],
  };
  for (let i = 1; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--binary") args.binary = argv[++i];
    else if (a === "--jobs") args.jobs = Number(argv[++i]);
    else if (a === "--keep-passing") args.keepPassing = true;
    else args.prefixes.push(a.replace(/^\/+/, ""));
  }
  return args;
}

const run = promisify(execFile);

async function git(...args: string[]): Promise<string> {
  return (await run("git", ["-C", REPO, ...args])).stdout.trim();
}

/** One page of a test or reference, as Chrome drew it and as our SVG draws it. */
interface Pictures {
  chrome: Buffer;
  svg: Buffer;
}

/** A browser context of its own per job: its responses are what the capture saves. */
class Job {
  readonly context: BrowserContext;
  readonly recorder: ResourceRecorder;
  /** Where tests and references are opened. */
  readonly page: Page;
  /** Where SVGs are drawn back, at the same size. */
  readonly canvas: Page;

  private constructor(context: BrowserContext, page: Page, canvas: Page) {
    this.context = context;
    this.recorder = new ResourceRecorder(context);
    this.page = page;
    this.canvas = canvas;
  }

  static async open(browser: Browser): Promise<Job> {
    const context = await browser.newContext({ viewport: VIEWPORT, deviceScaleFactor: 1 });
    return new Job(context, await context.newPage(), await context.newPage());
  }

  /**
   * Loads `url`, waits as WPT's harness does (docs/writing-tests/reftests.md, "Controlling When
   * Comparison Occurs"), captures it and converts it: after `load` and web fonts, a
   * `reftest-wait` page is sent `TestRendered` and given until it drops the class.
   */
  async pictures(url: string, dir: string, args: Args, long: boolean): Promise<Pictures> {
    const timeout = long ? 60_000 : 15_000;
    this.recorder.responses.splice(0);
    await this.page.goto(ORIGIN + url, { waitUntil: "load", timeout });
    await this.page.evaluate(async () => {
      await document.fonts.ready;
      // Pending paints: two frames.
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      const root = document.documentElement;
      if (root.classList.contains("reftest-wait")) {
        root.dispatchEvent(new Event("TestRendered", { bubbles: true }));
      }
    });
    await this.page.waitForFunction(
      () => !document.documentElement.classList.contains("reftest-wait"),
      undefined,
      { timeout },
    );
    await capture(this.page, dir, await styles(args.binary), this.recorder);
    const { svg } = await convert(args.binary, dir);
    const chrome = await readFile(join(dir, "chrome.png"));
    await this.canvas.setContent(`<body style="margin:0">${svg}</body>`);
    return { chrome, svg: await this.canvas.screenshot({ scale: "css" }) };
  }
}

/**
 * Where a page's capture goes. References get folders of their own: a test page can also be
 * another test's reference, and the two captures (and the clean-up of a passing test) must not
 * share files.
 */
function folder(url: string, role: "test" | "ref"): string {
  const path = url.replace(/^\//, "").replace(/[?#]/g, "_");
  return role === "test" ? join(OUT, path) : join(OUT, "refs", path);
}

async function judge(test: Reftest, job: Job, args: Args, refCache: Map<string, Promise<Pictures>>, chromeVersion: string): Promise<Outcome> {
  const testPictures = await job.pictures(test.url, folder(test.url, "test"), args, test.long);
  const chromeDiffs: { ref: string; relation: "==" | "!="; d: Difference }[] = [];
  const svgDiffs: { ref: string; relation: "==" | "!="; d: Difference }[] = [];
  for (const ref of test.refs) {
    let pictures = refCache.get(ref.url);
    if (!pictures) {
      pictures = job.pictures(ref.url, folder(ref.url, "ref"), args, test.long);
      refCache.set(ref.url, pictures);
    }
    const p = await pictures;
    chromeDiffs.push({ ref: ref.url, relation: ref.relation, d: difference(testPictures.chrome, p.chrome) });
    svgDiffs.push({ ref: ref.url, relation: ref.relation, d: difference(testPictures.svg, p.svg) });
  }
  if (!passes(test, chromeDiffs)) return { kind: "chrome-fails", chrome: chromeVersion };
  return passes(test, svgDiffs) ? { kind: "pass" } : { kind: "fail" };
}

/** The folders `run` tests when given no prefixes (wpt/areas). */
async function areas(): Promise<string[]> {
  return (await readFile(join(REPO, "wpt/areas"), "utf8"))
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#"))
    .map((l) => `${l.replace(/\/+$/, "")}/`);
}

async function runTests(args: Args): Promise<number> {
  const all = await readReftests("/wpt/MANIFEST.json");
  const prefixes = args.prefixes.length ? args.prefixes : await areas();
  const tests = all.filter((t) => prefixes.some((p) => t.url.slice(1).startsWith(p)));
  if (tests.length === 0) {
    console.error(`no reftests match ${prefixes.join(", ")}`);
    return 1;
  }
  const runInfo = {
    version: await git("describe", "--always", "--dirty"),
    date: new Date().toISOString().slice(0, 10),
    wpt: (await readFile(join(REPO, "wpt/commit"), "utf8")).trim().slice(0, 12),
  };
  const entries = await load(RESULTS);
  await mkdir(OUT, { recursive: true });

  const browser = await chromium.launch({ args: WPT_CHROME_ARGS });
  const chromeVersion = browser.version();
  console.log(`${tests.length} reftests, ${args.jobs} jobs, Chromium ${chromeVersion}, wpt ${runInfo.wpt}`);
  // References are shared by many tests: each is captured once per run.
  const refCache = new Map<string, Promise<Pictures>>();
  const changes: Change[] = [];
  const flaky: string[] = [];
  const counts = new Map<string, number>();
  let next = 0;
  let done = 0;

  const worker = async () => {
    let job = await Job.open(browser);
    while (next < tests.length) {
      const test = tests[next++];
      const attempt = async (): Promise<Outcome> => {
        try {
          return await judge(test, job, args, refCache, chromeVersion);
        } catch (e) {
          // A page that timed out may still be busy: go on with a fresh context.
          await job.context.close().catch(() => {});
          job = await Job.open(browser);
          return { kind: "error", message: String((e as Error).message ?? e).split("\n")[0] };
        }
      };
      let outcome = await attempt();
      if (outcome.kind !== "pass") {
        // Under the load of parallel jobs a page is now and then captured before it settles
        // (measured: a test that failed in a full run passed 5 of 5 times alone). One more try,
        // on a fresh context; a pass then is a pass, and listed as flaky.
        await job.context.close().catch(() => {});
        job = await Job.open(browser);
        const retry = await attempt();
        if (retry.kind === "pass") flaky.push(test.url);
        outcome = retry;
      }
      if (outcome.kind === "pass" && !args.keepPassing) {
        await rm(folder(test.url, "test"), { recursive: true, force: true });
      }
      const change = record(entries, test.url, outcome, runInfo);
      if (change) changes.push(change);
      counts.set(outcome.kind, (counts.get(outcome.kind) ?? 0) + 1);
      done++;
      if (done % 25 === 0 || done === tests.length) {
        console.log(`  ${done}/${tests.length}  ${[...counts].map(([k, v]) => `${k} ${v}`).join(", ")}`);
      }
    }
    await job.context.close();
  };
  await Promise.all(Array.from({ length: args.jobs }, worker));
  await browser.close();
  await save(RESULTS, entries);

  const regressed = changes.filter((c) => c.kind === "regressed");
  const fixed = changes.filter((c) => c.kind === "fixed");
  const fresh = changes.filter((c) => c.kind === "new-failure");
  const list = (title: string, items: Change[]) => {
    if (!items.length) return;
    console.log(`\n${title} (${items.length}):`);
    for (const c of items.slice(0, 50)) console.log(`  ${c.test}${c.before ? `  (was ${c.before})` : ""}`);
    if (items.length > 50) console.log(`  ... and ${items.length - 50} more`);
  };
  list("REGRESSED: passed before, fail now", regressed);
  list("Now passing", fixed);
  list("New failures, recorded as todo", fresh);
  list("Passed on retry (flaky)", flaky.map((test) => ({ test, kind: "fixed" as const })));
  const dangling = await danglingLinks(REPO, entries);
  list("Regression-test links to nothing", dangling.map((test) => ({ test, kind: "regressed" as const })));
  console.log(`\nresults: ${RESULTS}\nfailing captures: ${OUT}`);
  return regressed.length || dangling.length ? 1 : 0;
}

async function main(): Promise<number> {
  const args = parseArgs(process.argv.slice(2));
  if (args.command === "run") return runTests(args);
  if (args.command === "check") {
    const dangling = await danglingLinks(REPO, await load(RESULTS));
    for (const d of dangling) console.log(`regression-test link to nothing: ${d}`);
    return dangling.length ? 1 : 0;
  }
  console.error("usage: main.ts run [--binary B] [--jobs N] [--keep-passing] [prefix...] | check");
  return 2;
}

process.exitCode = await main();
