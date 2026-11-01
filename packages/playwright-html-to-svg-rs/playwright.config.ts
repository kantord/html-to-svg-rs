import { defineConfig, devices } from "@playwright/test";

import type { HtmlToSvgOptions } from "./src/index.js";

// The fixture pages (`packages/test-fixtures`), built and served by Vite on its fixed port.
const FIXTURES = "http://localhost:5199";

export default defineConfig<HtmlToSvgOptions>({
  testDir: "tests",
  // Live sites (tests/live) run only as the `live` project: `just live-snapshots`.
  testIgnore: process.env.HTML_TO_SVG_LIVE ? [] : ["**/live/**"],
  reporter: process.env.CI ? "github" : "list",
  webServer: {
    command: "pnpm --filter test-fixtures build && pnpm --filter test-fixtures preview",
    url: `${FIXTURES}/index.html`,
    reuseExistingServer: !process.env.CI,
  },
  use: { baseURL: FIXTURES, viewport: { width: 800, height: 800 } },
  projects: [
    ...(process.env.HTML_TO_SVG_LIVE
      ? [{ name: "live", testDir: "tests/live", use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } } }]
      : []),
    { name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 800, height: 800 } } },
    {
      name: "chromium-2x",
      use: { ...devices["Desktop Chrome"], viewport: { width: 800, height: 800 }, deviceScaleFactor: 2 },
    },
    {
      name: "chrome",
      use: { ...devices["Desktop Chrome"], channel: "chrome", viewport: { width: 800, height: 800 } },
    },
  ],
});
