// wpt/results.jsonl: one line per test, sorted by test. The runner sets `pass`, `fail`,
// `chrome-fails` and `error`; `bug` and `not-planned` are a person's triage, which the runner
// never undoes while the test fails: it still runs those tests, keeps their score current, and
// reports when one passes.

import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

export type Status =
  /** Chrome passes WPT's assertion, and our SVG matches Chrome's picture of the page. */
  | "pass"
  /** Chrome passes, our SVG does not match, and nobody has looked at it yet. */
  | "fail"
  /** Fails because of a tracked bug: `bug` names its file, wpt/bugs/<bug>.md. */
  | "bug"
  /** Fails, and fixing it is not planned: `note` says why. */
  | "not-planned"
  /** Chrome fails WPT's own assertion (test against reference): says nothing about us. */
  | "chrome-fails"
  /** The test could not be run (timeout, conversion error): `note` has the message. */
  | "error";

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

export interface Entry {
  /** The test's URL path. */
  test: string;
  status: Status;
  /** The latest measurement (not for chrome-fails and error). */
  score?: Score;
  /** The run that last saw it pass. Kept when it starts failing: a `fail` with it regressed. */
  lastPass?: Run;
  /** For chrome-fails: the run that saw Chrome fail it. */
  chromeFailed?: Run;
  /** For bug: the bug's id, the name of its file in wpt/bugs/ without `.md`. */
  bug?: string;
  /** For not-planned: why; for error: the message. */
  note?: string;
}

/** Statuses a person sets: the runner keeps them while the test fails. */
const TRIAGED: Status[] = ["bug", "not-planned"];

export async function load(path: string): Promise<Map<string, Entry>> {
  const text = existsSync(path) ? await readFile(path, "utf8") : "";
  const entries = text
    .split("\n")
    .filter((line) => line.trim())
    .map((line) => JSON.parse(line) as Entry);
  return new Map(entries.map((e) => [e.test, e]));
}

export async function save(path: string, entries: Map<string, Entry>): Promise<void> {
  const lines = [...entries.values()]
    .sort((a, b) => a.test.localeCompare(b.test))
    .map((e) => JSON.stringify(e));
  await writeFile(path, lines.join("\n") + (lines.length ? "\n" : ""));
}

export type Outcome =
  | { kind: "pass"; score: Score }
  | { kind: "fail"; score: Score }
  | { kind: "chrome-fails" }
  | { kind: "error"; message: string };

export interface Change {
  test: string;
  /**
   * `regressed`: passed before, fails now (fails the run). `fixed`: a test that did not pass
   * passes now. `new-failure`: fails, never passed, not triaged: to be triaged. `worse`: still
   * passes, but with more than `WORSE` misplaced pixels more than the last run measured.
   */
  kind: "regressed" | "fixed" | "new-failure" | "worse";
  before?: Status;
  /** For a `fixed` test that was a `bug`: which. */
  bug?: string;
}

/** How many more misplaced pixels than last time make a passing test `worse` (see Change). */
const WORSE = 10;

/** Records one run's outcome; returns what changed that a person should hear about. */
export function record(entries: Map<string, Entry>, test: string, outcome: Outcome, run: Run): Change | undefined {
  const before = entries.get(test);
  const triaged = !!before && TRIAGED.includes(before.status);
  const lastPass = before?.lastPass;
  switch (outcome.kind) {
    case "pass": {
      entries.set(test, { test, status: "pass", score: outcome.score, lastPass: run });
      if (before && before.status !== "pass" && before.status !== "chrome-fails") {
        return { test, kind: "fixed", before: before.status, bug: before.bug };
      }
      const was = before?.score;
      const now = outcome.score;
      return was && now.outside + now.text > was.outside + was.text + WORSE
        ? { test, kind: "worse", before: "pass" }
        : undefined;
    }
    case "fail":
      if (triaged) {
        entries.set(test, { ...before, score: outcome.score });
        return undefined;
      }
      entries.set(test, { test, status: "fail", score: outcome.score, lastPass });
      if (before?.status === "pass") return { test, kind: "regressed", before: "pass" };
      return before?.status === "fail" ? undefined : { test, kind: "new-failure", before: before?.status };
    case "chrome-fails":
      entries.set(test, { test, status: "chrome-fails", chromeFailed: run, lastPass });
      return undefined;
    case "error":
      if (triaged) return undefined;
      entries.set(test, { test, status: "error", note: outcome.message, lastPass });
      return before?.status === "pass" ? { test, kind: "regressed", before: "pass" } : undefined;
  }
}
