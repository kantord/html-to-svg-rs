# Tracked bugs

One file per root cause that makes WPT tests fail: `<id>.md`, where `<id>` is what a failing
test's line in `../results.jsonl` names (`"status":"bug","bug":"<id>"`). Which tests a bug makes
fail is recorded there and only there; `just wpt` prints the count per open bug.

```markdown
---
title: One line: what goes wrong
status: open                      # or: fixed
regression-test: crates/html-to-svg-rs/src/lower.rs::xhtml_tag_names   # optional, <file>::<name>
---
What goes wrong, why (the code), the evidence (tests, pictures); how it was fixed, once it is.
```

A run fails when a test names a bug with no file here, when a bug's `regression-test` does not
exist, or when a bug marked `fixed` still has failing tests. It lists open bugs none of whose tests
fail any more, to be marked fixed.

A test that fails for several reasons names the one to fix first; the others go in the bug's text.
