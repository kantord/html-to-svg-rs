---
title: background-origin is never applied: background images are positioned from the padding box
status: fixed
regression-test: crates/html-to-svg-rs/tests/regressions.rs::background_origin_content_box
---
The capture does not ask Chrome for `background-origin` (`STYLES` in
`crates/html-to-svg-rs/src/snapshot.rs`) and the lowering does not pass it to Takumi
(`BOX_STYLES` in `lower.rs`), so background images are always positioned as for `padding-box`.
Takumi supports `background-origin`.

Evidence: about 25 failing WPT tests set `background-origin`.

**Fixed:** the capture asks for `background-origin`, the lowering passes it, and passes `padding` too, so Takumi knows the content box (see background-clip-not-forwarded).
