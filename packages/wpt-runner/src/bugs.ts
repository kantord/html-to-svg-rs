// wpt/bugs/<id>.md: one file per root cause that makes WPT tests fail. Front matter:
//
//   ---
//   title: XHTML documents lose their images and page background
//   status: open                      # or: fixed
//   regression-test: crates/html-to-svg-rs/tests/regressions.rs::xhtml_canvas   # optional, comma separated
//   ---
//   What goes wrong, why, the evidence; how it was fixed, once it is.
//
// Which tests a bug makes fail is recorded in wpt/results.jsonl (`"status":"bug","bug":"<id>"`)
// and nowhere else, so the two cannot disagree.

import { existsSync } from "node:fs";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";

import type { Entry } from "./results.ts";

export interface Bug {
  id: string;
  title: string;
  status: "open" | "fixed";
  /** `<file>::<test name>`, comma separated: the Rust tests that hold the case. */
  regressionTests: string[];
}

export async function loadBugs(repo: string): Promise<Map<string, Bug>> {
  const dir = join(repo, "wpt/bugs");
  const bugs = new Map<string, Bug>();
  if (!existsSync(dir)) return bugs;
  for (const name of (await readdir(dir)).filter((n) => n.endsWith(".md") && n !== "README.md")) {
    const id = name.slice(0, -3);
    const text = await readFile(join(dir, name), "utf8");
    const front = /^---\n([\s\S]*?)\n---/.exec(text)?.[1] ?? "";
    const field = (key: string) => new RegExp(`^${key}:\\s*(.+?)\\s*$`, "m").exec(front)?.[1];
    bugs.set(id, {
      id,
      title: field("title") ?? id,
      status: field("status") === "fixed" ? "fixed" : "open",
      regressionTests: (field("regression-test") ?? "").split(",").map((s) => s.trim()).filter(Boolean),
    });
  }
  return bugs;
}

/**
 * What is inconsistent between the results and the bug files, as sentences: a test naming a bug
 * that has no file, a regression test that does not exist, a bug marked fixed whose tests still
 * fail. Any of these fails a run (and `just test`).
 */
export async function bugProblems(repo: string, entries: Map<string, Entry>, bugs: Map<string, Bug>): Promise<string[]> {
  const problems: string[] = [];
  for (const e of entries.values()) {
    if (e.status === "bug" && (!e.bug || !bugs.has(e.bug))) {
      problems.push(`${e.test}: bug "${e.bug ?? ""}" has no file wpt/bugs/${e.bug ?? "?"}.md`);
    }
    if (e.status === "bug" && e.bug && bugs.get(e.bug)?.status === "fixed") {
      problems.push(`${e.test}: still fails, but bug ${e.bug} is marked fixed`);
    }
  }
  for (const bug of bugs.values()) {
    for (const test of bug.regressionTests) {
      const [file, name] = test.split("::");
      const path = join(repo, file ?? "");
      // A Rust test is `fn <name>(`; a Playwright test is named by its title in quotes.
      const marker = file?.endsWith(".rs") ? `fn ${name}(` : `"${name}"`;
      const ok = !!file && !!name && existsSync(path) && (await readFile(path, "utf8")).includes(marker);
      if (!ok) problems.push(`bug ${bug.id}: regression test ${test} does not exist`);
    }
  }
  return problems;
}

/**
 * Open bugs some of whose tests passed in this run (`passedNow`) and none of whose tests fail any
 * more: candidates to mark fixed. A bug no WPT test is marked with (one found outside WPT, held
 * only by its Rust regression test) is never listed.
 */
export function closable(entries: Map<string, Entry>, bugs: Map<string, Bug>, passedNow: Set<string>): Bug[] {
  const failing = new Set([...entries.values()].filter((e) => e.status === "bug").map((e) => e.bug));
  return [...bugs.values()].filter((b) => b.status === "open" && passedNow.has(b.id) && !failing.has(b.id));
}

/** How many tests each bug makes fail now. */
export function failingCounts(entries: Map<string, Entry>): Map<string, number> {
  const counts = new Map<string, number>();
  for (const e of entries.values()) {
    if (e.status === "bug" && e.bug) counts.set(e.bug, (counts.get(e.bug) ?? 0) + 1);
  }
  return counts;
}
