// wpt/results.jsonl: one line per test, sorted by test. The runner writes `pass`, `chrome-fails`
// and `error`, and `todo` for a failure it has not seen before; `todo`, `not-planned` and
// `regression-test` are decisions a person makes and the runner never undoes.

import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

export type Status =
  /** Chrome passes it, and so does our SVG. */
  | "pass"
  /** Our SVG fails where Chrome passes: to be worked on. */
  | "todo"
  /** Our SVG fails, and fixing it is not planned (`note` says why). */
  | "not-planned"
  /** Our SVG fails, and a Rust test holds the case (`link`). */
  | "regression-test"
  /** Chrome itself fails it: says nothing about the conversion. */
  | "chrome-fails"
  /** The test could not be run (timeout, conversion error): `note` has the message. */
  | "error";

export interface Entry {
  /** The test's URL path. */
  test: string;
  status: Status;
  /** When it last passed: this repository's version, the date, the WPT commit. */
  lastPass?: { version: string; date: string; wpt: string };
  /** Why: for not-planned, todo and error. */
  note?: string;
  /** For regression-test: `<file>::<test name>`, the Rust test holding the case. */
  link?: string;
  /** For chrome-fails: the Chrome version that failed it. */
  chrome?: string;
}

/** Statuses a person sets; a new failure never replaces them. */
const DECIDED: Status[] = ["todo", "not-planned", "regression-test"];

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
  | { kind: "pass" }
  | { kind: "fail" }
  | { kind: "chrome-fails"; chrome: string }
  | { kind: "error"; message: string };

export interface Change {
  test: string;
  /** `regressed`: passed before, fails now. `fixed`: did not pass before, passes now. */
  kind: "regressed" | "fixed" | "new-failure";
  before?: Status;
}

/** Records one run's outcome; returns what changed that a person should hear about. */
export function record(
  entries: Map<string, Entry>,
  test: string,
  outcome: Outcome,
  run: { version: string; date: string; wpt: string },
): Change | undefined {
  const before = entries.get(test);
  const keep = { lastPass: before?.lastPass, note: before?.note, link: before?.link };
  switch (outcome.kind) {
    case "pass":
      entries.set(test, { test, status: "pass", lastPass: run, link: keep.link });
      return before && before.status !== "pass" && before.status !== "chrome-fails"
        ? { test, kind: "fixed", before: before.status }
        : undefined;
    case "fail":
      if (before && DECIDED.includes(before.status)) return undefined;
      entries.set(test, { test, status: "todo", ...keep });
      return before?.status === "pass"
        ? { test, kind: "regressed", before: "pass" }
        : { test, kind: "new-failure", before: before?.status };
    case "chrome-fails":
      entries.set(test, { test, status: "chrome-fails", chrome: outcome.chrome, lastPass: keep.lastPass });
      return undefined;
    case "error":
      if (before && DECIDED.includes(before.status)) return undefined;
      entries.set(test, { test, status: "error", note: outcome.message, lastPass: keep.lastPass });
      return before?.status === "pass" ? { test, kind: "regressed", before: "pass" } : undefined;
  }
}

/** `regression-test` entries whose link names no file, or a file without that test name. */
export async function danglingLinks(repo: string, entries: Map<string, Entry>): Promise<string[]> {
  const dangling: string[] = [];
  for (const e of entries.values()) {
    if (e.status !== "regression-test") continue;
    const [file, name] = (e.link ?? "").split("::");
    const path = join(repo, file ?? "");
    const ok =
      !!file && !!name && existsSync(path) && (await readFile(path, "utf8")).includes(name);
    if (!ok) dangling.push(`${e.test} -> ${e.link ?? "(no link)"}`);
  }
  return dangling;
}
