---
title: The Rust capture's screenshot depended on the system's font settings
status: fixed
regression-test: crates/html-to-svg-cli/tests/end_to_end.rs::dashboard_converts_close_to_chrome_with_annotations
---
Chrome takes subpixel (LCD) text rendering and hinting from fontconfig. On a system set to
`rgba rgb` (the test container's default), `chrome.png` had colour-fringed glyphs and the
dashboard fixture differed from our SVG in 3% of its pixels instead of 0.3%: the reference
picture depended on the machine, not on the page. `--disable-lcd-text` alone did not change it.

Found while recapturing the dashboard fixture (2026-11-01).

**Fixed:** `capture.rs` writes a fontconfig file into each capture's temporary profile (the
system's configuration, plus greyscale unhinted glyphs) and starts Chrome with `FONTCONFIG_FILE`
pointing to it, the same settings as the WPT runner's comparison Chrome.
