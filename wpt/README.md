# web-platform-tests

The reftests of [web-platform-tests](https://github.com/web-platform-tests/wpt) (WPT) drive a
test run of html-to-svg; none of WPT is kept in this repository. What is kept:

| file | what it is |
|---|---|
| `commit` | the WPT commit every result is measured against |
| `paths` | the folders of WPT fetched (sparse checkout): its server, and the areas tested |
| `results.jsonl` | one line per test: its status, and when it last passed |
| `serve.sh` | in the test container: fetches the pinned checkout and serves it |

The runner is `packages/wpt-runner`.

## Running

```bash
just wpt                           # every reftest in the checkout (the areas in `paths`)
just wpt css/css-borders           # one area, or any path prefix
```

It runs in the test container (`container/Containerfile`). The checkout lives on a podman volume
(about 220 MB) and is fetched again only when `commit` or `paths` change.

For each reftest and each reference, Chromium opens the page at 800x600, as WPT's harness does
(after `load` and web fonts; a `reftest-wait` page is sent `TestRendered` and waited for). The page
is captured and converted to SVG. Then the test is judged twice, with WPT's rules for multiple
references and `fuzzy` allowances:

1. **Chrome's screenshots**, test against reference. If Chrome fails, the test is `chrome-fails`:
   it says nothing about the conversion.
2. **Our SVGs**, test against reference, both drawn at 800x600. Chrome passed, so a failure here
   is ours.

Captures of tests that do not pass stay in `out/wpt/<test path>/`, their references in
`out/wpt/refs/<path>/` (`snapshot.json`,
`chrome.png`, fonts, images, `out.svg`): the material for a Rust regression test.

## Statuses

| status | set by | meaning |
|---|---|---|
| `pass` | runner | Chrome passes, and so does our SVG |
| `todo` | runner, for a new failure; or you | our SVG fails; to be worked on |
| `not-planned` | you | our SVG fails; not going to be fixed (`note` says why) |
| `regression-test` | you | our SVG fails; a Rust test holds the case (`link`) |
| `chrome-fails` | runner | Chrome fails it too (`chrome` is the version) |
| `error` | runner | could not be run: timeout, conversion error (`note`) |

`lastPass` records the version of this repository, the date and the WPT commit of the last run
that passed. It stays when the test starts failing, so a `todo` with a `lastPass` is a
regression.

The runner never replaces `todo`, `not-planned` or `regression-test` with another failing
status. When such a test passes again it becomes `pass` (keeping its `link`), and the run lists
it under "Now passing".

**Triage** is editing the line: change `todo` to `not-planned` with a `note`, or write a
regression test and set

```json
{"test":"/css/...","status":"regression-test","link":"crates/html-to-svg-cli/tests/wpt.rs::border_radius_005"}
```

`link` is `<file>::<test name>`; every run (and `node packages/wpt-runner/src/main.ts check`)
fails when a link points to no such file or name.

A test that does not pass is tried once more on a fresh browser context; when the second try
passes, it counts as a pass and the run lists it under "Passed on retry (flaky)".

**A run fails** (exit code 1) when a test that passed before fails now, or a link points nowhere.

## Moving to a newer WPT

Put the new commit in `commit` and run `just wpt`. Each entry's `lastPass.wpt` says which WPT a
pass was measured against.
