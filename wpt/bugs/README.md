# Tracked bugs

One file per root cause that makes WPT tests fail: `<id>.md`, where `<id>` is what a failing
test's line in `../results.jsonl` names (`"status":"bug","bug":"<id>"`). Which tests a bug makes
fail is recorded there and only there; `just wpt` prints the count per open bug.

```markdown
---
title: One line: what goes wrong
status: open                      # or: fixed
regression-test: crates/html-to-svg-rs/tests/regressions.rs::xhtml_canvas   # <file>::<name>, comma separated
---
What goes wrong, why (the code), the evidence (tests, pictures); how it was fixed, once it is.
```

A run fails when a test names a bug with no file here, when a bug's `regression-test` does not
exist, or when a bug marked `fixed` still has failing tests. It lists open bugs none of whose tests
fail any more, to be marked fixed.

A test that fails for several reasons names the one to fix first; the others go in the bug's text.

## Regression tests

Every bug we can fix gets a Rust regression test in
`crates/html-to-svg-rs/tests/regressions.rs`, part of the ordinary `cargo test`: a page of our
own that isolates the bug (`tests/regressions/<case>/page.html`, or `.xhtml`), its capture by
Chrome committed next to it, and a test function naming the bug. No WPT file is copied.

1. Write `tests/regressions/<case>/page.html`: as small as reproduces the bug; no text unless the
   bug is about text (a web font then, with its licence next to it, so no installed font is
   needed).
2. `just regression-capture <case>`: Chrome captures it (`snapshot.json`, `chrome.png`, fonts,
   images), in the test container, served over HTTP.
3. Add `#[test] fn <case>() { case("<case>", "<bug>"); }` and the bug's `regression-test` line.
4. `cargo test -p html-to-svg-rs --test regressions -- --nocapture`, and look at
   `target/tmp/regressions/<case>/ours.png` against `chrome.png`: the difference must be the bug.

The test asserts what the bug's `status` says: `open`, that the bug still reproduces (more than 30
misplaced pixels, the WPT runner's rule); `fixed`, that the page matches Chrome. So a fix makes
the test fail until the bug is marked fixed, and a regression of a fixed bug fails it again.
