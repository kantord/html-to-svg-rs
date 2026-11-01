---
title: An image reached through a redirect is captured under its final URL only
status: fixed
regression-test: packages/playwright-html-to-svg-rs/tests/svg-snapshot.spec.ts::an image reached through a redirect is captured
---
Both captures (`capture.ts` and `capture.rs`) recorded a downloaded image under the URL of
its response, which after a redirect is the final one; the page's CSS names the first, so the
conversion found no image for it and left it out ("url() was not captured").

Found by the review of 2026-11-01 (correctness).

**Fixed:** an image is recorded under every URL of its redirect chain: Playwright's
`Request.redirectedFrom()` in `capture.ts`, `Network.requestWillBeSent` events with a
`redirectResponse` in `capture.rs` (checked by hand with a redirecting server; the Rust capture
has no automated test, as it needs Chrome).
