# html-to-svg-rs (prototype)

Chrome lays a page out, Takumi paints Chrome's boxes into SVG.

```
Chrome (CDP: DOMSnapshot.captureSnapshot + screenshot)
  -> snapshot.json                      crates/html-to-svg-rs/src/capture.rs
  -> decoded box tree                   crates/html-to-svg-rs/src/snapshot.rs
  -> Takumi node tree, no layout left   crates/html-to-svg-rs/src/lower.rs (+ geometry.rs)
  -> takumi::render_svg -> out.svg      (patched takumi-svg: annotations)
  -> data-* attributes back in          crates/html-to-svg-rs/src/annotate.rs
  -> resvg(out.svg) vs chrome.png       crates/html-to-svg-cli/src/diff.rs
```

Two crates:

- `crates/html-to-svg-rs`: the library. `capture::capture` drives Chrome and writes a capture
  directory; `convert` turns that directory into SVG without a browser.
- `crates/html-to-svg-cli`: the `html-to-svg` binary, plus the `diff` step, the test pages and the
  end-to-end test.

- `packages/playwright-html-to-svg-rs`: a Playwright fixture, `svgSnapshot(name)`, that captures
  the page under test over CDP and converts it with the binary (pnpm workspace; see its README).
- `crates/html-to-svg-node` and `packages/html-to-svg-rs`: the converter as a Node.js library,
  the npm package `html-to-svg-rs` (a napi-rs native addon, prebuilt per platform by
  `.github/workflows/npm.yml`; see its README).
- `packages/test-fixtures`: the pages those tests snapshot, a Vite app.
- `packages/wpt-runner` and `wpt/`: web-platform-tests reftests run through the converter, with a
  results database (`just wpt`; see `wpt/README.md`).

The patched takumi-svg lives at the workspace root (`vendor/`, wired in by `[patch.crates-io]` in
the root `Cargo.toml`).

## Tests

```bash
just test
```

runs every check (Rust fmt, clippy and tests; the Playwright tests on Chromium, Chromium at 2x and
Chrome) in one podman container (`container/Containerfile`): the same Chromium, Chrome, fonts and
fontconfig on every run, so a changed score means changed code. Inside it, a test that needs a
missing font fails instead of skipping. `just test-shell` opens a shell with the same mounts. The
first run builds the image and compiles everything (several minutes); caches live on podman volumes.

## CI and dependencies

- `.github/workflows/ci.yml`, on every push to main and pull request: Rust (fmt, clippy with
  warnings as errors, all tests, the end-to-end test required to find its fonts), `cargo deny
  check` (`deny.toml`: vulnerabilities and unmaintained crates, licences, crate sources), Node
  (`pnpm audit`, typecheck, the npm package's and the Playwright tests) and zizmor (the workflows'
  own security).
- `.github/workflows/npm.yml`: the npm package's native builds per platform, published on a `v*`
  tag. It uses no cache, so nothing a pull request could write reaches a published binary.
- Every action is pinned by commit, every job has only the permissions it needs, and checkouts
  keep no token.
- Every dependency is at least 3 days old: Renovate waits that long before proposing an update
  (security fixes do not wait, and digest updates have no release date to wait for); pnpm refuses
  younger packages, transitive ones too (`minimumReleaseAge` in `pnpm-workspace.yaml`); and
  `scripts/release-age.py` does the same for `Cargo.lock` in CI, as Cargo has no such setting.
  Exceptions are exact versions, with their reason (`release-age.toml`,
  `minimumReleaseAgeExclude`).
- `renovate.json` keeps dependencies current: Cargo dependencies are pinned to exact versions; takumi (and the patched takumi-svg in
  `vendor/`) moves only on approval, by hand; Playwright, napi-rs and the Rust toolchain each move
  as one group. Tool versions shared by the npm packages live once, in the pnpm catalog
  (`pnpm-workspace.yaml`).

## Run it

```bash
cargo build --release -p html-to-svg-cli
```

```bash
cd crates/html-to-svg-cli && ../../target/release/html-to-svg run pages/dashboard.html out/dashboard --size 1024x640
```

`run` is `capture` + `convert` + `diff`; each also runs alone (`html-to-svg capture <url|file> <dir>
[--settle SECONDS]`, `convert <dir> [--font F]...`, `diff <dir>`). The URL can be a file path,
`file://`, `https://` (through `$HTTPS_PROXY` when set) or `chrome://`. Output in `<dir>`:
`snapshot.json`, `chrome.png`, `fonts.json` + `fonts/` and `images.json` + `images/` (what Chrome
downloaded and drew with), `out.svg`, `takumi.png` (Takumi's own raster of the same tree),
`svg-via-resvg.png`, and `diff-svg.png` / `diff-takumi.png` (orange = differs, but a pixel within
1 px of the other picture matches it, so anti-aliasing or a sub-pixel shift; red = misplaced).
Chromium comes from `$CHROME`, else chromiumoxide's own search. Each capture runs Chrome with its
sandbox, in a temporary profile of its own (removed afterwards), and with greyscale unhinted glyphs
whatever the system's font settings. `--no-sandbox` (or `HTML_TO_SVG_NO_SANDBOX=1`, as the test
container sets) turns the sandbox off, only for where Chrome cannot have one.

`convert --assets DIR` writes the images the SVG would inline as `data:` URLs to `DIR` instead,
named `<sha256>.<ext>` (a downloaded image keeps its URL's extension; screenshot crops are `.png`),
so SVGs that show the same image share one file. The SVG links them by the path from its own
folder; `--assets-href PREFIX` sets another prefix, such as a CDN URL.

Fonts need no flags: capture asks Chrome which face drew each element's text and saves the web
fonts it downloaded; convert registers exactly those faces (system ones found through `fc-list`).
`--font` adds files on top.

`cargo test` runs the whole conversion offline on a committed capture of `pages/dashboard.html`
(`crates/html-to-svg-cli/tests/fixtures/dashboard/`) and checks the pixel score, the text widths and the annotations.

## Stress results (1280x800, 2026-10-31)

The live sites asked for (YouTube, ui.shadcn.com, Hacker News, Wikipedia) were refused by the
session's network policy, and building shadcn's site from its repository was refused by the
permission check (running outside code); these results are from what could run offline. "Ours"
pages are written for this prototype; the others are real pages made by other tools.

| page | what it is | boxes | text lines | visibly different | misplaced |
|---|---|---|---|---|---|
| `pages/dashboard.html` | ours: grid, table, inline marks, rotated badge, canvas | 60 | 46 | 0.19% | 0.03% |
| `pages/forms.html` | ours: native controls + shadcn-style restyled controls | 95 | 44 | 0.31% | 0.10% |
| `pages/stress.html` | ours: 16 hard features, one per tile | 116 | 73 | 1.68% | 0.52% |
| Go language spec | real: long document, code blocks (go1.25.1/doc) | 3998 | 8084 | 1.14% | 0.24% |
| rustdoc, struct page | real: rustdoc output of this crate | 854 | 1007 | 0.33% | 0.05% |
| rustdoc, crate index | real: rustdoc output of this crate | 134 | 104 | 0.72% | 0.11% |
| `chrome://settings` | real: web-component app, shadow DOM | 200 | 26 | 0.32% | 0.05% |
| `chrome://history` | real: list app, masked icons, image-set favicons | 285 | 32 | 1.51% | 0.13% |
| `chrome://version` | real: long table of text | 444 | 442 | 0.70% | 0.13% |
| `chrome://flags` | real: 6,396 boxes, many `<select>`s | 6396 | 4043 | 0.54% | 0.06% |

What "visibly different" minus "misplaced" is: glyph anti-aliasing. Chrome's text rasteriser adds
contrast and gamma, resvg and Takumi draw glyph edges slightly heavier; that belongs to whoever
renders the SVG, not to the conversion.

Bugs the stress set found and that are fixed: CSS background propagation looked for `<body>` under
the document box instead of under `<html>` (page background ended at the content); screen-reader
text hidden by the legacy `clip: rect(0 0 0 0)` was drawn; icons drawn as `mask-image` (and
`-webkit-mask-*`) were filled squares; `background-image`/`mask-image` URLs were not drawn (now the
images Chrome downloaded are inlined as `data:` URLs, `image-set()` resolved to the candidate Chrome
used); `clip-path` was not copied; text in shadow trees, for which Chrome reports no face, fell back
to a font Takumi lacked (faces are now also registered under their family name).

Still wrong on `pages/stress.html` (see Known limits): multi-column text, vertical writing modes,
`text-overflow: ellipsis`, an inline background over several lines, and conic gradients (seams).

## Results on `pages/dashboard.html` (1024x640, 2026-10-31)

The page has a CSS grid, a table, flex rows, wrapping paragraphs with inline `<mark>`/`<code>`, a
`::before` and list markers, rounded clips with `overflow: hidden`, shadows, gradients, opacity, a
rotated badge and a `<canvas>`.

| | mean channel diff | visibly different pixels (> 32/255) |
|---|---|---|
| out.svg rendered by resvg vs Chrome | 4.46/255 | 0.28% |
| Takumi's PNG of the same tree vs Chrome | 4.41/255 | 0.20% |

Text: with the same font files and font features, Takumi's line widths are within 0.05 px of
Chrome's on average (max 0.88 px over 46 lines); the letter-spacing fit corrects the 2 lines off
by more than 0.05 px to 0.00 px. What is left of the difference is anti-aliasing at glyph edges.
Every `data-testid` element of the page can be found in the SVG.

Steps that measurably mattered (each from the same capture):

| change | SVG visibly different |
|---|---|
| first run | 1.85% |
| keep spaces at the edges of a text fragment (`rose by 12.4%`, not `rose by12.4%`) | 1.06% |
| snap each line's top to a whole pixel (Chrome and Takumi's raster do; Takumi's SVG did not) | 0.37% |
| copy `font-variant-numeric` / `font-feature-settings` (tabular figures in the table) | 0.28% |

## How the lowering works

- **No layout left for Takumi.** Every box is `position: absolute; box-sizing: border-box` with
  Chrome's size, placed relative to its parent's padding box. The DOM nesting is kept because clips,
  opacity, transforms and filters only work as nesting. Display types are dropped: tables, floats,
  grid, inline formatting were all done by Chrome.
- **Paint order.** Siblings are sorted by Chrome's `paintOrders`.
- **Transforms.** Chrome reports the bounding box *after* transforms. The lowering keeps the map
  from untransformed to page space down the tree, finds each box's untransformed centre through
  its inverse, takes the untransformed size from `offsetRects` (elements) or by solving the
  rotated bounding box (text), and gives Takumi the box plus Chrome's `transform` and
  `transform-origin`.
- **Text.** One non-wrapping (`white-space: pre`) Takumi text node per Chrome text box (one per
  line), at Chrome's position, top snapped to a whole pixel when no rotation or scale applies.
  Each line is measured in Takumi (`takumi::measure`); if its width differs from Chrome's by more
  than 0.05 px, `letter-spacing` is corrected by the difference over the character count
  (`--no-fit` turns that off). A trailing space may or may not be in Chrome's box, so the fit uses
  whichever reading is closer.
- **Canvas background.** The root's or else the body's background covers the viewport (CSS
  background propagation); the body then does not paint it again.
- **Raster fallback.** `img`, `canvas`, `video`, `iframe`, inline `svg`, `object`, `embed` and form
  controls are cropped out of Chrome's screenshot and embedded as an `<image>`.
- **Annotations.** An element with `data-*` attributes gets a Takumi node id; the patched
  takumi-svg wraps *every paint chunk* of that node (background, content, outline, which can land
  far apart in paint order) in `<g data-takumi-node="…">`, and `annotate.rs` replaces that with
  the element's real attributes plus `data-html-to-svg-node` (the DOM index; a name no page uses, so
  a page's own `data-node` survives). `data-*` names XML does not allow are left out. Text lines carry the annotation
  of their nearest annotated ancestor; boxes carry only their own.

## The Takumi patch

`vendor/takumi-svg/` (workspace root) is takumi-svg 0.3.34 with `vendor/takumi-svg-annotations.patch` applied (about
20 lines: `SvgDocument::begin_annotation_group` and its call in `scene_emit.rs`), wired in with
`[patch.crates-io]`. To move to a newer Takumi: copy the new takumi-svg source over
`vendor/takumi-svg`, apply the patch again, bump `takumi` in `crates/html-to-svg-rs/Cargo.toml`.

## Upstream candidates (found by this prototype)

1. **takumi-svg drops raw RGBA images.** `ImageSourceInput::Rgba` falls through to `_ => None` in
   `image.rs` (`raster()`), so no `<image>` is written, though the raster backend draws it. The
   prototype passes PNG bytes (`ImageSourceInput::Buffer`) instead.
2. **takumi-svg does not snap text to the pixel grid** while Takumi's raster backend does; the same
   tree renders differently in the two backends (1.06% vs 0.30% visibly different pixels here).
   Whether SVG output should snap is a design question for upstream; the prototype snaps in the
   lowering.
3. **Node ids/attributes into the SVG.** The annotation patch above, as an option on `SvgOptions`.
4. **Conic gradients in SVG** are approximated by wedge paths (takumi-svg says so); rendered by
   resvg the wedges show seams and moiré (`pages/stress.html`, the clip-path tile).

## Known limits (not handled)

- Frames: only the first document of the snapshot is converted (`documents[1..]` ignored).
- Paint order is per sibling list: a positioned descendant that Chrome paints above a later
  sibling of its ancestor cannot be expressed as tree order.
- An inline element that wraps over several lines paints its background as one bounding box.
- Multi-column text: Chrome's `DOMSnapshot` reports the text boxes of every column at the first
  column's x (measured: both columns' lines start at x=974.5), so the columns are drawn on top of
  each other. A fix would infer the column from y going back up and shift by column width + gap.
- `writing-mode: vertical-*` text is drawn horizontally; `text-overflow: ellipsis` draws the
  clipped text without the `…`.
- `url(...)` images Chrome did not download (or `chrome://` ones it serves without the network
  domain) are dropped with a warning; 3D transforms are drawn flat; a rotated or scaled
  `img`/`canvas` is skipped; the raster fallback includes whatever Chrome drew over that element.
- Scroll position is assumed to be 0; scroll containers clip, scrollbars are not drawn.
- `transform-origin` must be in px (Chrome's computed value is).
- A face in a font collection (`.ttc`) other than the first is not registered (reported).
- The in-page (wasm) variant is not built: the lowering takes only `snapshot.json`, so a JS
  collector writing the same shape is the missing piece.
