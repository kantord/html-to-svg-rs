---
title: The body's background is propagated to the canvas even when containment forbids it
status: open
---
CSS propagates the body's background to the canvas only when neither `<html>` nor `<body>` has
paint, layout or size containment (`contain: paint`, `layout`, `size`, `content`, `strict`).
`canvas_background` (`crates/html-to-svg-rs/src/lower.rs`) checks only the colours, and the
capture does not ask Chrome for `contain` (`STYLES` in `snapshot.rs`), so it cannot tell.

Evidence: `background-color-body-propagation-008.html` (`html { contain: paint }`) and `-009`
(`body { contain: paint }`): Chrome paints green only behind the body's line of text, our SVG
paints the whole page green (464,000 pixels differ).
