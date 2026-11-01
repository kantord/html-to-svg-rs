// wpt/results.jsonl: one line per test, sorted by test. The runner sets `pass`, `fail`,
// `chrome-fails` and `error`; `bug` and `not-planned` are a person's triage, which the runner
// never undoes while the test fails: it still runs those tests, keeps their score current, and
// reports when one passes.
//
// The file is edited by hand (triage), so every line is checked when it is read (`parseEntry`):
// a mistyped status or a missing field is an error naming the line, not a silent wrong result.

import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";

/** A run: this repository's version, the date, the Chrome version, the WPT commit. */
export interface Run {
  version: string;
  date: string;
  chrome: string;
  wpt: string;
}

/** How far our SVG is from Chrome's picture (see `fidelity` in compare.ts), in pixels. */
export interface Score {
  /** Misplaced outside the text layer: decides the verdict with `text`. */
  outside: number;
  /** Misplaced in the text layer (`textMask` in compare.ts). */
  text: number;
  /** Not exactly equal outside the text layer: for information (faint shading differences). */
  exact: number;
}

interface Common {
  /** The test's URL path. */
  test: string;
  /** The run that last saw it pass. Kept when it starts failing: a `fail` with it regressed. */
  lastPass?: Run;
}

/** One test's line, by status: each status has the fields it needs, and only those. */
export type Entry =
  /** Chrome passes WPT's assertion, and our SVG matches Chrome's picture of the page. */
  | (Common & { status: "pass"; score: Score; lastPass: Run })
  /** Chrome passes, our SVG does not match, and nobody has looked at it yet. */
  | (Common & { status: "fail"; score: Score })
  /** Fails because of a tracked bug: `bug` names its file, wpt/bugs/<bug>.md. */
  | (Common & { status: "bug"; bug: string; score?: Score })
  /** Fails, and fixing it is not planned: `note` says why. */
  | (Common & { status: "not-planned"; note: string; score?: Score })
  /** Chrome fails WPT's own assertion (test against reference): says nothing about us. */
  | (Common & { status: "chrome-fails"; chromeFailed: Run })
  /** The test could not be run (timeout, conversion error): `note` has the message. */
  | (Common & { status: "error"; note: string });

export type Status = Entry["status"];

/** The latest measurement of a test, if it has one (not chrome-fails or error). */
export function scoreOf(entry: Entry): Score | undefined {
  return "score" in entry ? entry.score : undefined;
}

// ---- reading and writing ----

const STATUSES: readonly Status[] = ["pass", "fail", "bug", "not-planned", "chrome-fails", "error"];

type Json = Record<string, unknown>;
const isObject = (v: unknown): v is Json => typeof v === "object" && v !== null && !Array.isArray(v);

function string(o: Json, key: string): string {
  const v = o[key];
  if (typeof v !== "string") throw new Error(`"${key}" must be a string`);
  return v;
}

function run(v: unknown, key: string): Run {
  if (!isObject(v)) throw new Error(`"${key}" must be an object`);
  return { version: string(v, "version"), date: string(v, "date"), chrome: string(v, "chrome"), wpt: string(v, "wpt") };
}

function score(v: unknown): Score {
  if (!isObject(v)) throw new Error(`"score" must be an object`);
  const n = (key: string) => {
    const x = v[key];
    if (typeof x !== "number") throw new Error(`"score.${key}" must be a number`);
    return x;
  };
  return { outside: n("outside"), text: n("text"), exact: n("exact") };
}

/** One line of results.jsonl, checked. */
export function parseEntry(line: string): Entry {
  const o: unknown = JSON.parse(line);
  if (!isObject(o)) throw new Error("not a JSON object");
  const status = o["status"];
  if (typeof status !== "string" || !STATUSES.includes(status as Status)) {
    throw new Error(`"status" must be one of ${STATUSES.join(", ")}`);
  }
  // Fields in the order `record` writes them, so reading and saving changes no line.
  const test = string(o, "test");
  const last = o["lastPass"] === undefined ? {} : { lastPass: run(o["lastPass"], "lastPass") };
  const optionalScore = o["score"] === undefined ? {} : { score: score(o["score"]) };
  switch (status as Status) {
    case "pass":
      return { test, status: "pass", score: score(o["score"]), lastPass: run(o["lastPass"], "lastPass") };
    case "fail":
      return { test, status: "fail", score: score(o["score"]), ...last };
    case "bug":
      return { test, status: "bug", ...optionalScore, ...last, bug: string(o, "bug") };
    case "not-planned":
      return { test, status: "not-planned", ...optionalScore, ...last, note: string(o, "note") };
    case "chrome-fails":
      return { test, status: "chrome-fails", chromeFailed: run(o["chromeFailed"], "chromeFailed"), ...last };
    case "error":
      return { test, status: "error", note: string(o, "note"), ...last };
  }
}

export async function load(path: string): Promise<Map<string, Entry>> {
  const text = existsSync(path) ? await readFile(path, "utf8") : "";
  const entries = new Map<string, Entry>();
  for (const [i, line] of text.split("\n").entries()) {
    if (!line.trim()) continue;
    try {
      const entry = parseEntry(line);
      entries.set(entry.test, entry);
    } catch (e) {
      throw new Error(`${path}:${i + 1}: ${(e as Error).message}`);
    }
  }
  return entries;
}

export async function save(path: string, entries: Map<string, Entry>): Promise<void> {
  const lines = [...entries.values()].sort((a, b) => a.test.localeCompare(b.test)).map((e) => JSON.stringify(e));
  await writeFile(path, lines.join("\n") + (lines.length ? "\n" : ""));
}

// ---- recording a run ----

export type Outcome =
  | { kind: "pass"; score: Score }
  | { kind: "fail"; score: Score }
  | { kind: "chrome-fails" }
  | { kind: "error"; message: string };

/** What changed that a person should hear about. */
export type Change =
  /** Passed before, fails now: fails the run. */
  | { kind: "regressed"; test: string }
  /** Did not pass before, passes now; `bug` when it was marked with one. */
  | { kind: "fixed"; test: string; before: Status; bug?: string }
  /** Fails, never seen failing before, not triaged: to be triaged. */
  | { kind: "new-failure"; test: string; before?: Status }
  /** Still passes, with more than `WORSE` misplaced pixels more than last time. */
  | { kind: "worse"; test: string };

/** How many more misplaced pixels than last time make a passing test `worse`. */
const WORSE = 10;

/** Records one run's outcome; returns what changed that a person should hear about. */
export function record(entries: Map<string, Entry>, test: string, outcome: Outcome, run: Run): Change | undefined {
  const before = entries.get(test);
  const kept = before?.lastPass ? { lastPass: before.lastPass } : {};
  switch (outcome.kind) {
    case "pass": {
      entries.set(test, { test, status: "pass", score: outcome.score, lastPass: run });
      if (before && before.status !== "pass" && before.status !== "chrome-fails") {
        return { kind: "fixed", test, before: before.status, ...(before.status === "bug" ? { bug: before.bug } : {}) };
      }
      const was = before && scoreOf(before);
      const now = outcome.score;
      return was && now.outside + now.text > was.outside + was.text + WORSE ? { kind: "worse", test } : undefined;
    }
    case "fail":
      // A person's triage stays; only the measurement is brought up to date.
      if (before?.status === "bug" || before?.status === "not-planned") {
        entries.set(test, { ...before, score: outcome.score });
        return undefined;
      }
      entries.set(test, { test, status: "fail", score: outcome.score, ...kept });
      if (before?.status === "pass") return { kind: "regressed", test };
      if (before?.status === "fail") return undefined;
      return { kind: "new-failure", test, ...(before ? { before: before.status } : {}) };
    case "chrome-fails":
      entries.set(test, { test, status: "chrome-fails", chromeFailed: run, ...kept });
      return undefined;
    case "error":
      if (before?.status === "bug" || before?.status === "not-planned") return undefined;
      entries.set(test, { test, status: "error", note: outcome.message, ...kept });
      return before?.status === "pass" ? { kind: "regressed", test } : undefined;
  }
}
