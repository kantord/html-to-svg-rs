---
title: Only the root's background colour covers the canvas, not its background image
status: open
regression-test: crates/html-to-svg-rs/tests/regressions.rs::canvas_background_image
---
CSS paints the root element's background (or else the body's) over the whole canvas: colour and
images alike, images positioned against the root element. `canvas_background`
(`crates/html-to-svg-rs/src/lower.rs`) moves only `background-color` to the canvas layer, and
`background_on_canvas` then drops every `background*` declaration from that element. So a
`background-image` or gradient on `<html>` (or a propagated `<body>`) is drawn nowhere.

Evidence: `background-attachment-margin-root-001.html` (a gradient on `html` with a margin:
Chrome tiles it over the whole viewport, our SVG has none; 370,000 pixels differ) and
`background-image-shared-stylesheet.html` (`:root { background-image: ... }`, 480,000). The 7
tests marked with this bug are the failing HTML tests whose `<html>` or `<body>` has a computed
`background-image` other than `none`.
