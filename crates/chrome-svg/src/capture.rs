//! Drives headless Chromium over CDP: loads a page at a fixed viewport, then writes
//! `snapshot.json` (raw `DOMSnapshot.captureSnapshot`), `chrome.png` (the reference picture) and
//! the fonts Chrome drew the text with (`fonts/`, `fonts.json`; see [`crate::fonts`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::css;
use chromiumoxide::cdp::browser_protocol::dom::{
    self, BackendNodeId, GetDocumentParams, PushNodesByBackendIdsToFrontendParams,
};
use chromiumoxide::cdp::browser_protocol::dom_snapshot::CaptureSnapshotParams;
use chromiumoxide::cdp::browser_protocol::network::{
    EventResponseReceived, GetResponseBodyParams, ResourceType,
};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use serde_json::Value;

use crate::fonts::{FontUse, FontsFile};
use crate::resources::{Resource, Resources};
use crate::snapshot::STYLES;

/// The Chromium binary: `$CHROME`, else the one preinstalled in Claude's cloud containers,
/// else whatever chromiumoxide finds on the system.
fn chrome_path() -> Option<String> {
    std::env::var("CHROME").ok().or_else(|| {
        let preinstalled = "/opt/pw-browsers/chromium";
        Path::new(preinstalled)
            .exists()
            .then(|| preinstalled.to_owned())
    })
}

pub struct Settings {
    pub size: (u32, u32),
    /// Extra time after load for late content (lazy images, client rendering).
    pub settle: Duration,
}

pub async fn capture(url: &str, out: &Path, settings: &Settings) -> Result<()> {
    let (width, height) = settings.size;
    let mut config = BrowserConfig::builder()
        .no_sandbox()
        .new_headless_mode()
        .window_size(width, height)
        .viewport(Viewport {
            width,
            height,
            device_scale_factor: Some(1.0),
            ..Viewport::default()
        })
        // Same pixels every run: no GPU raster differences, no scrollbars.
        .arg("--disable-gpu")
        .arg("--hide-scrollbars")
        .arg("--font-render-hinting=none");
    // Chromium does not read HTTPS_PROXY; the proxy's CA is in the NSS store it does read.
    if let Ok(proxy) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("https_proxy")) {
        config = config.arg(format!("--proxy-server={proxy}"));
    }
    if let Some(path) = chrome_path() {
        config = config.chrome_executable(path);
    }
    let (mut browser, mut handler) = Browser::launch(config.build().map_err(anyhow::Error::msg)?)
        .await
        .context("launch Chromium")?;
    let events = tokio::spawn(async move { while handler.next().await.is_some() {} });

    // Listen for font and image responses before navigating, so none is missed.
    let page = browser.new_page("about:blank").await.context("open tab")?;
    let mut responses = page.event_listener::<EventResponseReceived>().await?;
    let wanted = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let listener = {
        let wanted = wanted.clone();
        tokio::spawn(async move {
            while let Some(event) = responses.next().await {
                if matches!(event.r#type, ResourceType::Font | ResourceType::Image) {
                    let entry = (
                        event.request_id.clone(),
                        event.response.url.clone(),
                        event.r#type == ResourceType::Font,
                        event.response.mime_type.clone(),
                    );
                    wanted.lock().expect("not poisoned").push(entry);
                }
            }
        })
    };
    page.goto(url).await.context("navigate")?;
    page.wait_for_navigation().await?;
    tokio::time::sleep(settings.settle).await;
    // Web fonts must be in before layout is read; back to the top for a viewport capture.
    page.evaluate("window.scrollTo(0, 0); document.fonts.ready.then(() => true)")
        .await?;

    let mut params = CaptureSnapshotParams::new(STYLES.iter().map(|s| (*s).to_owned()).collect());
    params.include_paint_order = Some(true);
    params.include_dom_rects = Some(true);
    let snapshot = page
        .execute(params)
        .await
        .context("DOMSnapshot.captureSnapshot")?;
    let mut snapshot_json = serde_json::to_value(&snapshot.result)?;
    // The style arrays are positional: keep the names with them, so a later STYLES list still
    // reads this capture correctly.
    snapshot_json["computedStyles"] = serde_json::json!(STYLES);
    std::fs::write(
        out.join("snapshot.json"),
        serde_json::to_vec(&snapshot_json)?,
    )?;

    let png = page
        .screenshot(
            ScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .build(),
        )
        .await
        .context("screenshot")?;
    std::fs::write(out.join("chrome.png"), png)?;

    let mut fonts = FontsFile {
        used: used_fonts(&page, &snapshot_json).await?,
        ..FontsFile::default()
    };
    let fonts_dir = out.join("fonts");
    let images_dir = out.join("images");
    std::fs::create_dir_all(&fonts_dir)?;
    std::fs::create_dir_all(&images_dir)?;
    listener.abort();
    let requests = std::mem::take(&mut *wanted.lock().expect("not poisoned"));
    let mut images = Resources::default();
    for (i, (id, url, is_font, mime)) in requests.into_iter().enumerate() {
        let Ok(body) = page.execute(GetResponseBodyParams::new(id)).await else {
            eprintln!("warning: body not available: {url}");
            continue;
        };
        let bytes = if body.result.base64_encoded {
            base64::engine::general_purpose::STANDARD.decode(&body.result.body)?
        } else {
            body.result.body.clone().into_bytes()
        };
        if is_font {
            let name = format!("web-{i}.font");
            std::fs::write(fonts_dir.join(&name), bytes)?;
            fonts.web.insert(name, url);
        } else {
            let name = format!("img-{i}");
            std::fs::write(images_dir.join(&name), bytes)?;
            images.images.insert(url, Resource { file: name, mime });
        }
    }
    std::fs::write(out.join("fonts.json"), serde_json::to_vec_pretty(&fonts)?)?;
    std::fs::write(out.join("images.json"), serde_json::to_vec_pretty(&images)?)?;

    browser.close().await?;
    browser.wait().await?;
    events.abort();
    Ok(())
}

/// Which font Chrome drew each element's text with: `CSS.getPlatformFontsForNode` for every
/// element that has a text child in the snapshot, keyed by the element's snapshot index.
async fn used_fonts(
    page: &chromiumoxide::Page,
    snapshot: &Value,
) -> Result<BTreeMap<usize, Vec<FontUse>>> {
    let nodes = &snapshot["documents"][0]["nodes"];
    let as_ints = |v: &Value| -> Vec<i64> {
        v.as_array()
            .map(|a| a.iter().filter_map(Value::as_i64).collect())
            .unwrap_or_default()
    };
    let parents = as_ints(&nodes["parentIndex"]);
    let types = as_ints(&nodes["nodeType"]);
    let backend = as_ints(&nodes["backendNodeId"]);
    let text_parents: BTreeSet<usize> = types
        .iter()
        .enumerate()
        .filter(|&(_, &t)| t == 3)
        .filter_map(|(i, _)| usize::try_from(*parents.get(i)?).ok())
        .collect();
    let elements: Vec<usize> = text_parents.into_iter().collect();

    page.execute(dom::EnableParams::default()).await?;
    page.execute(css::EnableParams::default()).await?;
    page.execute(GetDocumentParams::default()).await?;
    let ids = page
        .execute(PushNodesByBackendIdsToFrontendParams::new(
            elements
                .iter()
                .map(|&i| BackendNodeId::new(backend[i]))
                .collect::<Vec<_>>(),
        ))
        .await
        .context("DOM.pushNodesByBackendIdsToFrontend")?;

    let mut used = BTreeMap::new();
    for (&element, node_id) in elements.iter().zip(&ids.result.node_ids) {
        let Ok(fonts) = page
            .execute(css::GetPlatformFontsForNodeParams::new(*node_id))
            .await
        else {
            continue;
        };
        let list: Vec<FontUse> = fonts
            .result
            .fonts
            .iter()
            .map(|f| FontUse {
                family: f.family_name.clone(),
                postscript: f.post_script_name.clone(),
                custom: f.is_custom_font,
                glyphs: f.glyph_count,
            })
            .collect();
        if !list.is_empty() {
            used.insert(element, list);
        }
    }
    Ok(used)
}
