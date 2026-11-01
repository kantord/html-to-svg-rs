import { fileURLToPath } from "node:url";

import { defineConfig, devices } from "@playwright/test";

import type { HtmlToSvgOptions } from "./src/index.js";

// The binary `pnpm test` builds (`cargo build -p html-to-svg-cli`) at the workspace root.
const binary =
  process.env.HTML_TO_SVG_BIN ??
  fileURLToPath(new URL("../../target/debug/html-to-svg", import.meta.url));

// The fixture pages (`packages/test-fixtures`), built and served by Vite on its fixed port.
const FIXTURES = "http://localhost:5199";

export default defineConfig<HtmlToSvgOptions>({
  testDir: "tests",
  reporter: process.env.CI ? "github" : "list",
  webServer: {
    command: "pnpm --filter test-fixtures build && pnpm --filter test-fixtures preview",
    url: `${FIXTURES}/index.html`,
    reuseExistingServer: !process.env.CI,
  },
  use: { baseURL: FIXTURES, htmlToSvgBinary: binary, viewport: { width: 800, height: 800 } },
  projects: [
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
