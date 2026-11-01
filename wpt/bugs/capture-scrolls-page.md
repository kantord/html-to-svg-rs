---
title: svgSnapshot leaves the page under test scrolled to the top
status: fixed
regression-test: packages/playwright-html-to-svg-rs/tests/svg-snapshot.spec.ts::the snapshot leaves the page scrolled where it was
---
The conversion assumes scroll position 0, so `capture.ts` scrolls the page to the top before
reading its layout, and left it there: a test that scrolled, took a snapshot and went on worked
on a page scrolled somewhere else.

Found by the review of 2026-11-01 (correctness).

**Fixed:** `capture.ts` remembers the scroll position and restores it after the capture.
