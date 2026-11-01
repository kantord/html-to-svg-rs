---
title: XHTML documents lose their images, canvases and page background
status: open
---
In an XHTML document (`.xht`, `.xhtml`) Chrome reports tag names in lower case (`img`, `html`,
`body`); in an HTML document in upper case. The lowering only knows the upper-case names:

- `RASTER_TAGS` (`crates/html-to-svg-rs/src/lower.rs`) lists `IMG`, `CANVAS`, ... so an XHTML
  `<img>` or `<canvas>` is not cropped from Chrome's screenshot: it is drawn as an empty box.
- `canvas_background` looks for `HTML` and `BODY`, finds neither, and paints the canvas white:
  the page background of an XHTML page is lost, or painted twice.

Evidence: `background-size/background-size-cover.xht`, whose snapshot has node names `html`,
`body`, `img`; our SVG has no image where Chrome draws one. The 5 tests marked with this bug are
the failing XHTML tests with an `<img>` or `<canvas>` or a background on `<html>`/`<body>`.

Also affected, but they fail for another reason too and are left untriaged: `border-image-017` to
`020.xht` and `border-image-slice-001`, `002.xht` (`border-image` is not drawn at all).

Fix: compare tag names case-insensitively (or upper-case them when the snapshot is decoded,
`snapshot.rs`).
