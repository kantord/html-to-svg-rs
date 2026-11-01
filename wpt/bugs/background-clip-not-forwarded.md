---
title: background-clip is never applied: backgrounds always reach the border box
status: fixed
regression-test: crates/html-to-svg-rs/tests/regressions.rs::background_clip_content_box, crates/html-to-svg-rs/tests/regressions.rs::background_clip_text
---
The capture does not ask Chrome for `background-clip` (`STYLES` in
`crates/html-to-svg-rs/src/snapshot.rs`) and the lowering does not pass it to Takumi
(`BOX_STYLES` in `lower.rs`), so every background is drawn under the border too. Takumi supports
`background-clip` (`border-box`, `padding-box`, `content-box`, `text`, `border-area`; its SVG
backend draws `text` in `takumi-svg/src/render.rs`), so forwarding it should be most of the fix.

Unknown: `background-clip: text` clips to the box's glyphs, and our lowering draws each line of
text as a node of its own; whether Takumi finds those glyphs is to be seen.

Evidence: about 59 failing WPT tests use `background-clip` (20 with box values, 39 with `text` or
`border-area`); the regression tests reproduce both kinds.

**Fixed:** the capture asks for `background-clip`, the lowering passes it, and passes `padding` too (it changes no size or place: boxes are border-box and children absolute from the padding box), so Takumi knows the content box. `background-clip: text` works with our one-node-per-line text: Takumi finds the glyphs.
