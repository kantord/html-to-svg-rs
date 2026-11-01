---
title: An element cropped from the screenshot and reaching past the viewport is stretched
status: fixed
regression-test: crates/html-to-svg-rs/tests/regressions.rs::offscreen_raster
---
`raster` (`lower.rs`) crops an `img`, `canvas`, form control ... out of Chrome's screenshot and
draws the crop over the element's whole box. When the box reaches past the viewport, only the
visible part is in the screenshot, and that part is stretched over the whole box.

Found by the review of 2026-11-01 (correctness).

**Fixed:** `raster` crops only the part inside the viewport and places it at its own size and place in the element's box, and reports the element as partly outside. (A first version also moved the crop by the element's border width; that broke 6 WPT tests of bordered `img`, `video`, `iframe`, `svg` and form controls, because the element's node gets no border: the crop already holds Chrome's border.)
