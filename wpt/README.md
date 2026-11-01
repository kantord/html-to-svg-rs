# web-platform-tests

The reftests of [web-platform-tests](https://github.com/web-platform-tests/wpt) (WPT) drive a
test run of html-to-svg; none of WPT is kept in this repository. What is kept:

| file | what it is |
|---|---|
| `commit` | the WPT commit every result is measured against |
| `areas` | the folders whose reftests are run |
| `paths` | the other folders of WPT fetched: its server, and files the tests share |
| `results.jsonl` | one line per test: its status, its latest score, when it last passed |
| `serve.sh` | in the test container: fetches the pinned checkout and serves it |

The runner is `packages/wpt-runner`.

## Running

```bash
just wpt                           # every reftest in wpt/areas
just wpt css/css-borders           # one area, or any path prefix
just wpt-report                    # out/wpt/report.html: what did not pass, with pictures
```

It runs in the test container (`container/Containerfile`). The checkout lives on a podman volume
(about 260 MB) and is fetched again only when `commit`, `areas` or `paths` change. A full run of
the two areas takes about two minutes.

## What a test checks

WPT's own assertion (a test looks like its reference) is Chrome's to pass, not ours. Ours is that
**our SVG of the page looks like Chrome's picture of the page**. Two Chromes:

1. **The gate**, launched with WPT's own flags. It screenshots the test and its references, and
   judges them by WPT's rules (several references, `fuzzy` allowances, `reftest-wait`). If Chrome
   fails, the test is `chrome-fails`, with the run that saw it, and is not compared.
2. **The comparison Chrome**, with unhinted greyscale glyphs (`container/fonts-nohint.conf`,
   `--font-render-hinting=none --disable-lcd-text`). It captures the test page, the converter
   turns that into SVG, and the same Chrome draws the SVG. The two pictures are compared by
   **misplaced pixels**: a pixel counts when a channel differs by more than 32 and no pixel within
   1 px of the other picture explains it. That is content missing, extra or in the wrong place,
   and not:
   - **edges anti-aliased differently**: Chrome draws a rounded rectangle, our SVG the same shape
     as a path (measured up to about 30 pixels on WPT's border-radius tests);
   - **glyph edges**: our SVG draws glyphs as outlines, Chrome draws them from the font, and their
     edges always differ (measured: about a third of the pixels inside text boxes, whatever
     Chrome's font settings);
   - **faint shading differences**: gradients, blur.

   **The text layer** is where Chrome's picture changes when the page's text is made invisible:
   after the capture, the runner adds CSS that hides glyphs and nothing else
   (`-webkit-text-fill-color`, `text-shadow`, `text-decoration-color` and `text-emphasis-color`
   transparent, `HIDE_TEXT` in main.ts; not `color`, which would hide `currentColor` borders too),
   takes a second screenshot (`textless.png`), and the differing pixels, grown by one for glyph
   edges, are the text layer. Glyphs, underlines, shadows and glyphs reaching past their line box
   are in it; the background between and under the glyphs is not, and is held to the same rule
   as every other box.

   The `score` counts misplaced pixels in the text layer and outside it apart, and, for
   information, the pixels outside it that are not exactly equal (`exact`). A test passes with at
   most 30 of each (`THRESHOLD` in `packages/wpt-runner/src/main.ts`): calibrated on all 909
   compared tests, where past 30 the number of passing tests barely moves (490 at 30, 506 at 100)
   and the rest differ by thousands of pixels. A passing test whose misplaced count grows by more
   than 10 is listed as "got worse" without failing the run.

Why not WPT's flags for the comparison too: they change text layout (measured on 20 pages: line
widths 9 px from what the converter measures for the same font, against 0.1 px without them).

A test that does not pass is tried once more on fresh browser contexts; a pass then counts, and
the run lists it under "Passed on retry (flaky)". Captures of tests that do not pass stay in
`out/wpt/<test path>/` (`snapshot.json`, `chrome.png`, fonts, images, `out.svg`, `svg.png`: our SVG
drawn, `diff.png`): the material for a Rust regression test.

## Statuses

| status | set by | meaning |
|---|---|---|
| `pass` | runner | Chrome passes; our SVG matches Chrome's picture |
| `fail` | runner | Chrome passes; our SVG does not match; not triaged yet |
| `bug` | you | fails because of a tracked bug: `bug` names its file, `bugs/<bug>.md` |
| `not-planned` | you | fails, and fixing it is not planned: `note` says why |
| `chrome-fails` | runner | Chrome fails WPT's assertion (`chromeFailed`: version, date, WPT commit) |
| `error` | runner | could not be run: timeout, conversion error (`note`) |

`lastPass` is the run that last saw the test pass: this repository's version, the date, Chrome's
version and the WPT commit. It stays when the test starts failing, so a `fail` with a `lastPass` is
a regression.

**Triage** is editing a `fail` line: to a tracked bug, when its root cause is known (one file per
root cause in `bugs/`, see `bugs/README.md`), or to not planned:

```json
{"test":"/css/...","status":"bug","bug":"xhtml-tag-case", ...}
{"test":"/css/...","status":"not-planned","note":"vertical writing modes", ...}
```

The runner still runs triaged tests and keeps their `score` current, but never changes their
status while they fail. When one passes it becomes `pass`, and the run lists it under "Now
passing"; an open bug none of whose tests fail any more is listed, to be marked fixed. Each run
prints every open bug with how many tests it makes fail.

**A run fails** (exit code 1) when a test that passed before fails now, a test names a bug with no
file, a bug's regression test does not exist, or a bug marked fixed still has failing tests. `just
test` runs the same consistency check. New failures are listed for triage but do not fail the run.

## Moving to a newer WPT or Chrome

Put the new commit in `commit` (or update the test image) and run `just wpt`. Each `lastPass` and
`chromeFailed` says which WPT and Chrome it was measured with.
