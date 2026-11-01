# test-fixtures

The pages the Playwright tests of `playwright-html-to-svg-rs` snapshot, as a Vite app.

- **A fixture is a page:** `pages/<name>.html`, with its own CSS and TS next to it. Every
  `pages/*.html` is built; list new ones in `pages/index.html`. Tests open it as `/<name>.html`.
- **Imports are bundled like a real app's:** a web font from `@fontsource` becomes a hashed
  `.woff2` the page downloads, only for the subsets its text needs.
- **`public/`** is served as it is, for images whose exact bytes a test checks.

`pnpm dev` serves the pages on http://localhost:5199 with live reload, for writing a fixture.
The tests start `pnpm build && pnpm preview` themselves (`webServer` in the Playwright config),
on the same port, and reuse a server already running there outside CI.
