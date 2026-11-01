// Writes an html-to-svg capture directory from a Playwright page: the same files the Rust
// `capture` writes (`crates/html-to-svg-rs/src/capture.rs`), so `html-to-svg convert` reads it.
//
//   snapshot.json  DOMSnapshot.captureSnapshot, plus `computedStyles` (the style names, in order)
//   chrome.png     the viewport screenshot (pixels for <img>, <canvas>, form controls)
//   fonts.json     faces Chrome drew each element's text with + web fonts saved under fonts/
//   images.json    images Chrome downloaded, saved under images/

import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import type { BrowserContext, Page, Response } from "@playwright/test";

/**
 * Font and image responses of every page of a browser context, from when recording started:
 * the capture saves their bodies. Recording must start before the page navigates.
 */
export class ResourceRecorder {
  readonly responses: Response[] = [];
  private readonly listener = (response: Response) => {
    const type = response.request().resourceType();
    if (type === "font" || type === "image") this.responses.push(response);
  };

  constructor(private readonly context: BrowserContext) {
    context.on("response", this.listener);
  }

  stop(): void {
    this.context.off("response", this.listener);
  }
}

interface FontUse {
  family: string;
  postscript: string;
  custom: boolean;
  glyphs: number;
}

const TEXT_NODE = 3;

export async function capture(
  page: Page,
  dir: string,
  styles: string[],
  recorder: ResourceRecorder,
): Promise<void> {
  await mkdir(join(dir, "fonts"), { recursive: true });
  await mkdir(join(dir, "images"), { recursive: true });

  // Web fonts must be in before layout is read; the conversion assumes scroll position 0.
  await page.evaluate(async () => {
    window.scrollTo(0, 0);
    await document.fonts.ready;
  });

  const cdp = await page.context().newCDPSession(page);
  try {
    const snapshot = await cdp.send("DOMSnapshot.captureSnapshot", {
      computedStyles: styles,
      includePaintOrder: true,
      includeDOMRects: true,
    });
    // Chrome's style arrays are positional: keep the names with them.
    const snapshotJson = { ...snapshot, computedStyles: styles };
    await writeFile(join(dir, "snapshot.json"), JSON.stringify(snapshotJson));

    // CSS pixels, so the picture lines up with the snapshot at any device scale factor.
    await page.screenshot({ path: join(dir, "chrome.png"), scale: "css" });

    const used = await usedFonts(cdp, snapshot);
    const web: Record<string, string> = {};
    const images: Record<string, { file: string; mime: string }> = {};
    for (const [i, response] of recorder.responses.entries()) {
      const body = await response.body().catch(() => undefined);
      if (!body) continue;
      if (response.request().resourceType() === "font") {
        const name = `web-${i}.font`;
        await writeFile(join(dir, "fonts", name), body);
        web[name] = response.url();
      } else {
        const file = `img-${i}`;
        await writeFile(join(dir, "images", file), body);
        const mime = (response.headers()["content-type"] ?? "").split(";")[0].trim();
        images[response.url()] = { file, mime };
      }
    }
    await writeFile(join(dir, "fonts.json"), JSON.stringify({ used, web }, null, 2));
    await writeFile(join(dir, "images.json"), JSON.stringify({ images }, null, 2));
  } finally {
    await cdp.detach();
  }
}

type Cdp = Awaited<ReturnType<ReturnType<Page["context"]>["newCDPSession"]>>;
/** The part of a DOMSnapshot result this file reads. */
interface Snapshot {
  documents: { nodes: { parentIndex?: number[]; nodeType?: number[]; backendNodeId?: number[] } }[];
}

/** Which faces Chrome drew each element's text with, keyed by the element's snapshot index. */
async function usedFonts(cdp: Cdp, snapshot: unknown): Promise<Record<number, FontUse[]>> {
  const nodes = (snapshot as Snapshot).documents[0]?.nodes ?? {};
  const parents = nodes.parentIndex ?? [];
  const types = nodes.nodeType ?? [];
  const backend = nodes.backendNodeId ?? [];
  const elements = [
    ...new Set(types.flatMap((t, i) => (t === TEXT_NODE && parents[i] >= 0 ? [parents[i]] : []))),
  ].sort((a, b) => a - b);

  await cdp.send("DOM.enable");
  await cdp.send("CSS.enable");
  await cdp.send("DOM.getDocument");
  const { nodeIds } = await cdp.send("DOM.pushNodesByBackendIdsToFrontend", {
    backendNodeIds: elements.map((i) => backend[i]),
  });

  const used: Record<number, FontUse[]> = {};
  for (const [k, element] of elements.entries()) {
    const nodeId = nodeIds[k];
    if (!nodeId) continue;
    const result = await cdp.send("CSS.getPlatformFontsForNode", { nodeId }).catch(() => undefined);
    const list = (result?.fonts ?? []).map((f) => ({
      family: f.familyName,
      postscript: f.postScriptName,
      custom: f.isCustomFont,
      glyphs: f.glyphCount,
    }));
    if (list.length > 0) used[element] = list;
  }
  return used;
}
