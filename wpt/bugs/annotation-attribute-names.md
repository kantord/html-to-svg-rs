---
title: A page's own data-node, or an attribute name XML forbids, makes the SVG invalid
status: fixed
regression-test: crates/html-to-svg-rs/tests/regressions.rs::duplicate_data_node
---
`annotate.rs` writes `data-node` (the DOM index) on every annotated group and then copies the
element's `data-*` attributes. A page whose element has its own `data-node` gets the attribute
twice, which is not XML: browsers refuse to open the SVG. The names are also copied as they are,
but HTML allows `data-*` names XML does not (`setAttribute("data-a·b;", ...)`).

Found by the review of 2026-11-01 (correctness).

**Fixed:** our own attribute is now `data-html-to-svg-node`, a name no page uses, so the page's `data-node` is kept as it is; `data-*` names XML does not allow are left out (`xml_name` in `annotate.rs`).
