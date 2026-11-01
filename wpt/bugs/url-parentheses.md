---
title: A url() with a parenthesis inside (a data URI with rgb()) is cut short
status: fixed
regression-test: crates/html-to-svg-rs/tests/regressions.rs::data_url_parentheses
---
`inline_urls` and `resolve_image_sets` (`resources.rs`) end a `url(` at the first `)`. Chrome
writes computed URLs quoted (`url("...")`), and a data URI may hold `rgb(...)` or other
parentheses: the URL is cut there and the image is lost.

Found by the review of 2026-11-01 (round 1 of its skeptic pass).

**Fixed:** `read_url` (`resources.rs`) reads a quoted URL to its closing quote, escapes included; `closing_paren` skips quoted strings when finding the end of an `image-set(`.
