---
title: overflow clips every DOM descendant, also those whose containing block is outside it
status: open
regression-test: crates/html-to-svg-rs/tests/regressions.rs::overflow_popup
---
The lowering nests each box in its DOM parent's node, and a node with `overflow: hidden` clips
all of its children. In CSS, `overflow` clips a descendant only when the box is in that
descendant's containing-block chain: an absolutely positioned popup whose containing block is a
positioned ancestor *outside* the clipping box is not clipped (dropdowns, tooltips, menus in a
card with `overflow: hidden`). Same family as paint-order-follows-dom: the node tree follows the
DOM, where clipping and painting follow other trees.

Fix: a design change of the lowering (place out-of-flow boxes under the node of their containing
block, or give each box the clip of its containing-block chain). Found by the review of
2026-11-01 (correctness).
