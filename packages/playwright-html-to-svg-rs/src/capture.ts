// Writes an html-to-svg capture directory from a Playwright page: the same files the Rust
// `capture` writes (`crates/html-to-svg-rs/src/capture.rs`), so `html-to-svg convert` reads it.
//
//   snapshot.json  DOMSnapshot.captureSnapshot, plus `computedStyles` (the style names, in order)
//   chrome.png     the viewport screenshot (pixels for <img>, <canvas>, form controls)
//   fonts.json     faces Chrome drew each element's text with + web fonts saved under fonts/
//   images.json    images Chrome downloaded, saved under images/

import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

import type { BrowserContext, Page, Request, Response } from "@playwright/test";

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

/**
 * Writes the capture directory of `page`'s viewport into `dir`. `styles` are the computed styles
 * to ask Chrome for (`styles()` of html-to-svg-rs). The fonts and images are those a
 * `recorder` (started before the page loaded) saw; without one, those Chrome keeps for the page
 * (its resource tree), which are the same except that an image reached through a redirect is
 * known only by its final URL.
 */
export async function capture(
  page: Page,
  dir: string,
  styles: string[],
  recorder?: ResourceRecorder,
): Promise<void> {
  await mkdir(join(dir, "fonts"), { recursive: true });
  await mkdir(join(dir, "images"), { recursive: true });

  // Web fonts must be in before layout is read; the conversion assumes scroll position 0, so the
  // page is scrolled to the top for the capture and back where it was afterwards.
  const scrolled = await page.evaluate(async () => {
    const at = { x: window.scrollX, y: window.scrollY };
    window.scrollTo(0, 0);
    await document.fonts.ready;
    return at;
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

    const used = await usedFonts(cdp, snapshot.documents[0]?.nodes);
    const web: Record<string, string> = {};
    const images: Record<string, { file: string; mime: string }> = {};
    const resources = recorder ? recorded(recorder) : await kept(cdp, page);
    for (const [i, resource] of resources.entries()) {
      const body = await resource.body().catch(() => undefined);
      if (!body) continue;
      if (resource.font) {
        const name = `web-${i}.font`;
        await writeFile(join(dir, "fonts", name), body);
        web[name] = resource.urls[0];
      } else {
        const file = `img-${i}`;
        await writeFile(join(dir, "images", file), body);
        for (const url of resource.urls) images[url] = { file, mime: resource.mime };
      }
    }
    await writeFile(join(dir, "fonts.json"), JSON.stringify({ used, web }, null, 2));
    await writeFile(join(dir, "images.json"), JSON.stringify({ images }, null, 2));
  } finally {
    await cdp.detach();
    await page.evaluate(({ x, y }) => window.scrollTo(x, y), scrolled);
  }
}

type Cdp = Awaited<ReturnType<ReturnType<Page["context"]>["newCDPSession"]>>;

/** A font or image the page loaded: every URL it is known by, and how to get its bytes. */
interface Resource {
  font: boolean;
  /** The first is the URL the page asked for; the last the one that answered. */
  urls: [string, ...string[]];
  mime: string;
  body: () => Promise<Buffer>;
}

/** What a recorder saw; an image under every URL of its redirect chain (the page names the first). */
function recorded(recorder: ResourceRecorder): Resource[] {
  return recorder.responses.map((response) => {
    // From the request that answered back to the one the page made.
    const urls: [string, ...string[]] = [response.url()];
    for (let from = response.request().redirectedFrom(); from; from = from.redirectedFrom()) {
      urls.unshift(from.url());
    }
    return {
      font: response.request().resourceType() === "font",
      urls,
      mime: (response.headers()["content-type"] ?? "").split(";", 1)[0]?.trim() ?? "",
      body: () => response.body(),
    };
  });
}

interface FrameTree {
  frame: { id: string };
  childFrames?: FrameTree[];
  resources: { url: string; type: string; mimeType: string; failed?: boolean; canceled?: boolean }[];
}

/**
 * The fonts and images of the page and its frames, as Chrome lists them (`Page.getResourceTree`).
 * Chrome keeps the bytes of images, not of web fonts (`Page.getResourceContent`: "No resource
 * with given URL found"): what it does not keep is downloaded again, with the page's cookies.
 */
async function kept(cdp: Cdp, page: Page): Promise<Resource[]> {
  await cdp.send("Page.enable");
  const { frameTree } = (await cdp.send("Page.getResourceTree")) as unknown as { frameTree: FrameTree };
  const found: Resource[] = [];
  const walk = (tree: FrameTree) => {
    for (const r of tree.resources) {
      if ((r.type !== "Font" && r.type !== "Image") || r.failed || r.canceled || r.url.startsWith("data:")) continue;
      found.push({
        font: r.type === "Font",
        urls: [r.url],
        mime: r.mimeType,
        body: async () => {
          try {
            const { content, base64Encoded } = await cdp.send("Page.getResourceContent", {
              frameId: tree.frame.id,
              url: r.url,
            });
            return Buffer.from(content, base64Encoded ? "base64" : "utf8");
          } catch {
            const response = await page.context().request.get(r.url);
            if (!response.ok()) throw new Error(`${r.url}: HTTP ${response.status()}`);
            return response.body();
          }
        },
      });
    }
    tree.childFrames?.forEach(walk);
  };
  walk(frameTree);
  return found;
}
/** The node arrays of a DOMSnapshot document that this file reads (column-oriented, by node). */
interface SnapshotNodes {
  parentIndex?: number[];
  nodeType?: number[];
  backendNodeId?: number[];
}

/** Which faces Chrome drew each element's text with, keyed by the element's snapshot index. */
async function usedFonts(cdp: Cdp, nodes: SnapshotNodes | undefined): Promise<Record<number, FontUse[]>> {
  const parents = nodes?.parentIndex ?? [];
  const backend = nodes?.backendNodeId ?? [];
  // The elements that have a text child, with the backend id DOM.* commands know them by.
  const elements = new Map<number, number>();
  for (const [i, type] of (nodes?.nodeType ?? []).entries()) {
    const parent = parents[i];
    if (type !== TEXT_NODE || parent === undefined || parent < 0) continue;
    const id = backend[parent];
    if (id !== undefined) elements.set(parent, id);
  }
  const ordered = [...elements].sort(([a], [b]) => a - b);

  await cdp.send("DOM.enable");
  await cdp.send("CSS.enable");
  await cdp.send("DOM.getDocument");
  const { nodeIds } = await cdp.send("DOM.pushNodesByBackendIdsToFrontend", {
    backendNodeIds: ordered.map(([, id]) => id),
  });

  const used: Record<number, FontUse[]> = {};
  for (const [k, [element]] of ordered.entries()) {
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
