---
title: Only one subset of a web font split by unicode-range is used
status: fixed
regression-test: crates/html-to-svg-rs/tests/regressions.rs::font_subsets
---
A web font is often served as several files, one per `unicode-range` (latin, latin-ext,
cyrillic, ...), all with the same PostScript name. `fonts.rs` keys the files it found by
PostScript name and keeps one, so text needing another subset (`ő`) is drawn in a fallback font.

Found by the review of 2026-11-01 (correctness).

**Fixed:** `fonts.rs` keeps every file per PostScript name and registers each, under the alias and the family name. The regression test checks that both subset files of the fixture are registered (a pixel comparison cannot tell a wrong font from glyph-edge differences here).
