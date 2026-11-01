// Runs web-platform-tests reftests through html-to-svg and keeps wpt/results.jsonl.
//
//   node src/main.ts run [--binary B] [--jobs N] [--keep-passing] [path-prefix...]
//   node src/main.ts check
//   node src/main.ts report
//
// `run` expects the pinned checkout served at http://web-platform.test:8000/ (wpt/serve.sh, in
// the test container: `just wpt`). Two Chromes, one job per CPU pair:
//   1. The gate, launched as WPT launches Chrome: screenshots of the test and its references,
//      judged by WPT's own rules. If Chrome fails, the test is `chrome-fails` and that is all.
//   2. The comparison Chrome, with unhinted greyscale glyphs (container/fonts-nohint.conf): the
//      test page is captured and converted, our SVG is drawn by the same Chrome, and the two
//      pictures are compared (`fidelity` in compare.ts): misplaced pixels, outside text and
//      inside text boxes apart. Within the thresholds below, the test passes.
// Captures of tests that do not pass stay under out/wpt/<test path>/, with svg.png (our SVG,
// drawn) and diff.png; references' screenshots under out/wpt/refs/.
//
// `check` verifies the results against the bug files (wpt/bugs/, see bugs.ts).
// `report` writes out/wpt/report.html from what a run left (see report.ts).

import { execFile } from "node:child_process";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { cpus } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

import { type Browser, type BrowserContext, chromium, type Page } from "@playwright/test";
import { capture, convert, ResourceRecorder, styles } from "playwright-html-to-svg-rs";

import { type Box, difference, fidelity, passes } from "./compare.ts";
import { readReftests, type Reftest } from "./manifest.ts";
import { OUT, refPicture, REPO, RESULTS, testFolder } from "./paths.ts";
import { writeReport } from "./report.ts";
import { bugProblems, closable, failingCounts, loadBugs } from "./bugs.ts";
import { type Change, load, type Outcome, record, type Run, save, type Score } from "./results.ts";

const ORIGIN = "http://web-platform.test:8000";
const VIEWPORT = { width: 800, height: 600 };

/**
 * The flags of WPT's own Chrome runs that change what is drawn (the pinned checkout's
 * tools/wptrunner/wptrunner/browsers/chrome.py): without them Chrome "fails" tests it passes
 * upstream, e.g. by anti-aliasing a test's glyphs differently from its reference's.
 */
const GATE_CHROME = {
  args: [
    "--enable-blink-features=NoFontAntialiasing",
    "--disable-features=ScrollbarAnimations",
    "--enable-experimental-web-platform-features",
  ],
};

/**
 * The Chrome our SVG is compared against: unhinted, greyscale glyphs, closest to the glyph
 * outlines our SVG draws (measured on 20 pages with text: line widths within 0.1 px of ours, where
 * WPT's flags put them 9 px apart). Not WPT's flags: those change text layout.
 */
const COMPARISON_CHROME = {
  args: ["--font-render-hinting=none", "--disable-lcd-text"],
  env: { FONTCONFIG_FILE: "/etc/html-to-svg/fonts-nohint.conf" },
};

/**
 * A test passes when our SVG has at most this many misplaced pixels outside text boxes and inside
 * them (`fidelity` in compare.ts). Calibrated on all 909 compared tests of the two areas: rounded
 * edges anti-aliased differently reach about 30; past 30 the number of passing tests barely
 * moves (490 at 30, 506 at 100), the rest differing by thousands of pixels.
 */
const THRESHOLD = { outside: 30, text: 30 };

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

/**
 * Loads `url` and waits as WPT's harness does (docs/writing-tests/reftests.md, "Controlling When
 * Comparison Occurs"): after `load` and web fonts, a `reftest-wait` page is sent `TestRendered`
 * and given until it drops the class.
 */
async function open(page: Page, url: string, long: boolean): Promise<void> {
  const timeout = long ? 60_000 : 15_000;
  await page.goto(ORIGIN + url, { waitUntil: "load", timeout });
  await page.evaluate(async () => {
    await document.fonts.ready;
    // Pending paints: two frames.
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const root = document.documentElement;
    if (root.classList.contains("reftest-wait")) {
      root.dispatchEvent(new Event("TestRendered", { bubbles: true }));
    }
  });
  await page.waitForFunction(() => !document.documentElement.classList.contains("reftest-wait"), undefined, {
    timeout,
  });
}

/** One page in each Chrome, in contexts of their own. */
class Job {
  readonly gate: Page;
  readonly context: BrowserContext;
  readonly recorder: ResourceRecorder;
  /** Where test pages are captured for the comparison. */
  readonly page: Page;
  /** Where our SVGs are drawn, in the same Chrome. */
  readonly canvas: Page;

  private constructor(gate: Page, context: BrowserContext, page: Page, canvas: Page) {
    this.gate = gate;
    this.context = context;
    this.recorder = new ResourceRecorder(context);
    this.page = page;
    this.canvas = canvas;
  }

  static async open(gate: Browser, comparison: Browser): Promise<Job> {
    const options = { viewport: VIEWPORT, deviceScaleFactor: 1 };
    const gatePage = await (await gate.newContext(options)).newPage();
    const context = await comparison.newContext(options);
    return new Job(gatePage, context, await context.newPage(), await context.newPage());
  }

  async close(): Promise<void> {
    await this.gate.context().close().catch(() => {});
    await this.context.close().catch(() => {});
  }

  /** The gate's screenshot of a page. */
  async screenshot(url: string, long: boolean): Promise<Buffer> {
    await open(this.gate, url, long);
    return this.gate.screenshot({ scale: "css" });
  }

  /** Captures and converts the test page, draws our SVG, compares (see `fidelity`). */
  async compare(test: Reftest, args: Args): Promise<Score> {
    const dir = testFolder(test.url);
    this.recorder.responses.splice(0);
    await open(this.page, test.url, test.long);
    await capture(this.page, dir, await styles(args.binary), this.recorder);
    const { svg } = await convert(args.binary, dir);
    await this.canvas.setContent(`<body style="margin:0">${svg}</body>`);
    const drawn = await this.canvas.screenshot({ scale: "css" });
    const snapshot = JSON.parse(await readFile(join(dir, "snapshot.json"), "utf8"));
    // Chrome's text boxes, grown by a pixel for the anti-aliasing at their edges.
    const boxes: Box[] = (snapshot.documents[0]?.textBoxes?.bounds ?? []).map(
      ([x, y, w, h]: number[]) => [Math.floor(x) - 1, Math.floor(y) - 1, Math.ceil(x + w) + 1, Math.ceil(y + h) + 1],
    );
    const result = fidelity(await readFile(join(dir, "chrome.png")), drawn, boxes);
    await writeFile(join(dir, "svg.png"), drawn);
    await writeFile(join(dir, "diff.png"), result.picture);
    return { outside: result.outside, text: result.text, exact: result.exact };
  }
}

async function judge(test: Reftest, job: Job, args: Args, refs: Map<string, Promise<Buffer>>): Promise<Outcome> {
  const testShot = await job.screenshot(test.url, test.long);
  const pictures = [];
  for (const ref of test.refs) {
    let shot = refs.get(ref.url);
    if (!shot) {
      shot = job.screenshot(ref.url, test.long);
      refs.set(ref.url, shot);
      const path = refPicture(ref.url);
      shot.then(async (png) => {
        await mkdir(join(path, ".."), { recursive: true });
        await writeFile(path, png);
      }).catch(() => {});
    }
    pictures.push({ ref: ref.url, relation: ref.relation, d: difference(testShot, await shot) });
  }
  if (!passes(test, pictures)) return { kind: "chrome-fails" };
  const score = await job.compare(test, args);
  const ok = score.outside <= THRESHOLD.outside && score.text <= THRESHOLD.text;
  return { kind: ok ? "pass" : "fail", score };
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
  const entries = await load(RESULTS);
  await mkdir(OUT, { recursive: true });

  const gate = await chromium.launch(GATE_CHROME);
  const comparison = await chromium.launch({ ...COMPARISON_CHROME, env: { ...process.env, ...COMPARISON_CHROME.env } });
  const runInfo: Run = {
    version: await git("describe", "--always", "--dirty"),
    date: new Date().toISOString().slice(0, 10),
    chrome: gate.version(),
    wpt: (await readFile(join(REPO, "wpt/commit"), "utf8")).trim().slice(0, 12),
  };
  console.log(`${tests.length} reftests, ${args.jobs} jobs, Chromium ${runInfo.chrome}, wpt ${runInfo.wpt}`);
  // References are shared by many tests: each is screenshotted once per run.
  const refs = new Map<string, Promise<Buffer>>();
  const changes: Change[] = [];
  const flaky: string[] = [];
  const counts = new Map<string, number>();
  let next = 0;
  let done = 0;

  const worker = async () => {
    let job = await Job.open(gate, comparison);
    const attempt = async (test: Reftest): Promise<Outcome> => {
      try {
        return await judge(test, job, args, refs);
      } catch (e) {
        // A page that timed out may still be busy: go on with fresh contexts.
        await job.close();
        job = await Job.open(gate, comparison);
        return { kind: "error", message: String((e as Error).message ?? e).split("\n")[0] };
      }
    };
    while (next < tests.length) {
      const test = tests[next++];
      let outcome = await attempt(test);
      if (outcome.kind !== "pass") {
        // Under the load of parallel jobs a page is now and then captured before it settles
        // (measured: a test that failed in a full run passed 5 of 5 times alone). One more try,
        // on fresh contexts; a pass then is a pass, and listed as flaky.
        await job.close();
        job = await Job.open(gate, comparison);
        const retry = await attempt(test);
        if (retry.kind === "pass") flaky.push(test.url);
        outcome = retry;
      }
      if (outcome.kind === "pass" && !args.keepPassing) {
        await rm(testFolder(test.url), { recursive: true, force: true });
      }
      const change = record(entries, test.url, outcome, runInfo);
      if (change) changes.push(change);
      const status = entries.get(test.url)?.status ?? outcome.kind;
      counts.set(status, (counts.get(status) ?? 0) + 1);
      done++;
      if (done % 50 === 0 || done === tests.length) {
        console.log(`  ${done}/${tests.length}  ${[...counts].map(([k, v]) => `${k} ${v}`).join(", ")}`);
      }
    }
    await job.close();
  };
  await Promise.all(Array.from({ length: args.jobs }, worker));
  await gate.close();
  await comparison.close();
  await save(RESULTS, entries);

  const list = (title: string, items: { test: string; before?: string; bug?: string }[]) => {
    if (!items.length) return;
    console.log(`\n${title} (${items.length}):`);
    for (const c of items.slice(0, 50)) {
      console.log(`  ${c.test}${c.before ? `  (was ${c.before}${c.bug ? ` ${c.bug}` : ""})` : ""}`);
    }
    if (items.length > 50) console.log(`  ... and ${items.length - 50} more`);
  };
  const regressed = changes.filter((c) => c.kind === "regressed");
  list("REGRESSED: passed before, fails now", regressed);
  list("Now passing", changes.filter((c) => c.kind === "fixed"));
  list("New failures, to triage", changes.filter((c) => c.kind === "new-failure"));
  list("Got worse, still passing", changes.filter((c) => c.kind === "worse"));
  list("Passed on retry (flaky)", flaky.map((test) => ({ test })));
  const bugs = await loadBugs(REPO);
  const perBug = failingCounts(entries);
  const open = [...bugs.values()].filter((b) => b.status === "open");
  if (open.length) {
    console.log(`\nOpen bugs (${open.length}):`);
    for (const b of open) console.log(`  ${b.id}: ${perBug.get(b.id) ?? 0} failing tests  ${b.title}`);
  }
  list("Open bugs none of whose tests fail any more: mark them fixed?", closable(entries, bugs).map((b) => ({ test: `wpt/bugs/${b.id}.md` })));
  const problems = await bugProblems(REPO, entries, bugs);
  list("PROBLEMS with bugs", problems.map((test) => ({ test })));
  const untriaged = [...entries.values()].filter((e) => e.status === "fail").length;
  console.log(`\n${untriaged} failures not triaged yet (status "fail").`);
  console.log(`results: ${RESULTS}\ncaptures of what did not pass: ${OUT} (just wpt-report)`);
  return regressed.length || problems.length ? 1 : 0;
}

async function main(): Promise<number> {
  const args = parseArgs(process.argv.slice(2));
  if (args.command === "run") return runTests(args);
  if (args.command === "report") {
    console.log(`report: ${await writeReport(REPO, OUT)}`);
    return 0;
  }
  if (args.command === "check") {
    const problems = await bugProblems(REPO, await load(RESULTS), await loadBugs(REPO));
    for (const p of problems) console.log(p);
    return problems.length ? 1 : 0;
  }
  console.error("usage: main.ts run [--binary B] [--jobs N] [--keep-passing] [prefix...] | check | report");
  return 2;
}

process.exitCode = await main();
