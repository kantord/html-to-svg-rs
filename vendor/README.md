# Vendored code

## takumi-svg

`takumi-svg/` is [takumi-svg](https://github.com/kane50613/takumi) 0.3.34 from crates.io,
Copyright (c) 2025 Kane Wang, licensed `MIT OR Apache-2.0` (`takumi-svg/LICENSE-MIT`,
`takumi-svg/LICENSE-APACHE`).

**Modified:** `takumi-svg-annotations.patch` is applied to it (about 20 lines in `src/lib.rs` and
`src/scene_emit.rs`, marked `PATCH(html-to-svg)`): each paint chunk of a node with an id is
wrapped in `<g data-takumi-node="...">`, so the SVG can be mapped back to the page's elements.

The root `Cargo.toml` uses it in place of the crates.io release (`[patch.crates-io]`). To move to
a newer takumi-svg: copy its source over `takumi-svg/`, apply the patch again, and bump `takumi`
in `crates/html-to-svg-rs/Cargo.toml`.
