---
title: corner-shape is never applied: shaped corners are drawn round
status: fixed
regression-test: crates/html-to-svg-rs/tests/regressions.rs::corner_shape_bevel
---
The capture does not ask Chrome for `corner-shape` (`STYLES` in
`crates/html-to-svg-rs/src/snapshot.rs`; Chrome may expose it as the four longhands
`corner-top-left-shape` and so on) and the lowering does not pass it to Takumi (`BOX_STYLES` in
`lower.rs`), so `bevel`, `notch`, `scoop`, `squircle` and `superellipse()` corners come out
round. Takumi supports `corner-shape` (`takumi-core` `layout/corner_shape.rs`).

Evidence: about 35 failing WPT tests in `css/css-borders/corner-shape/` (Chrome itself fails 25
more of that folder).

**Fixed:** the capture asks for the four longhands (`corner-top-left-shape`, ...) and the lowering passes them to Takumi, which draws them.
