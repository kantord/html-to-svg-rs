// One page per fixture: every `pages/*.html` is an entry (served in dev, built by `vite build`),
// and `public/` holds files served as they are (images the fixtures download).
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";

const pages = fileURLToPath(new URL("./pages/", import.meta.url));

/** The port the Playwright tests expect the fixtures on (see their playwright.config.ts). */
export const PORT = 5199;

export default defineConfig({
  root: pages,
  publicDir: fileURLToPath(new URL("./public/", import.meta.url)),
  server: { port: PORT, strictPort: true },
  preview: { port: PORT, strictPort: true },
  build: {
    outDir: fileURLToPath(new URL("./dist/", import.meta.url)),
    emptyOutDir: true,
    rollupOptions: {
      input: Object.fromEntries(
        readdirSync(pages)
          .filter((f) => f.endsWith(".html"))
          .map((f) => [f.replace(/\.html$/, ""), pages + f]),
      ),
    },
  },
});
