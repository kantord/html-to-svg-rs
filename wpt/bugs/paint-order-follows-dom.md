---
title: z-index -1 paints a box over its parent instead of under its parent's background
status: open
regression-test: crates/html-to-svg-rs/tests/regressions.rs::negative_z_index
---
The lowering paints a node's children after the node and orders siblings by Chrome's
`paintOrders`. A `z-index: -1` box whose parent is no stacking context belongs to the nearest
stacking context's negative layer, painted *below* the parent's background: the common "frame
behind the button" (`::before` with `z-index: -1`). Nested under its DOM parent, it is painted
over the parent's background instead (a solid frame-coloured box over the button). The README's
known limits name the general case: paint order is per sibling list.

Fix: a design change of the lowering (build the stacking-context tree, and nest by it rather than
by the DOM). Found by the review of 2026-11-01 (correctness).
