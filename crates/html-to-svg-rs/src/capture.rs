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
    EventRequestWillBeSent, EventResponseReceived, GetResponseBodyParams, ResourceType,
};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use serde_json::Value;

use crate::fonts::{FontUse, FontsFile};
use crate::resources::{Resource, Resources};
use crate::snapshot::STYLES;

/// The Chromium binary: `$CHROME`, else whatever chromiumoxide finds on the system.
fn chrome_path() -> Option<String> {
    std::env::var("CHROME").ok()
}

pub struct Settings {
    pub size: (u32, u32),
    /// Extra time after load for late content (lazy images, client rendering).
    pub settle: Duration,
    /// Run Chrome's sandbox. Turn it off only where Chrome cannot have one (some containers): a
    /// captured page is untrusted content, and without the sandbox a renderer exploit runs as the
    /// user.
    pub sandbox: bool,
}

/// A Chrome profile of its own for one capture, removed afterwards: captures running at once
/// would otherwise fight over one profile (Chrome's `SingletonLock`), and one page's cookies and
/// storage would be seen by the next.
struct Profile(std::path::PathBuf);

impl Profile {
    fn new() -> Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "html-to-svg-chrome-{}-{nanos}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        Ok(Self(dir))
    }
}

impl Profile {
    /// A fontconfig file in the profile: the system's, with greyscale unhinted glyphs.
    fn fontconfig(&self) -> Result<std::path::PathBuf> {
        let path = self.0.join("fonts.conf");
        std::fs::write(
            &path,
            r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <include ignore_missing="yes">/etc/fonts/fonts.conf</include>
  <match target="font">
    <edit name="antialias" mode="assign"><bool>true</bool></edit>
    <edit name="hinting" mode="assign"><bool>false</bool></edit>
    <edit name="hintstyle" mode="assign"><const>hintnone</const></edit>
    <edit name="rgba" mode="assign"><const>none</const></edit>
    <edit name="lcdfilter" mode="assign"><const>lcdnone</const></edit>
  </match>
</fontconfig>
"#,
        )
        .with_context(|| format!("write {}", path.display()))?;
        Ok(path)
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub async fn capture(url: &str, out: &Path, settings: &Settings) -> Result<()> {
    let (width, height) = settings.size;
    let profile = Profile::new()?;
    let mut config = BrowserConfig::builder()
        .user_data_dir(&profile.0)
        .new_headless_mode()
        .window_size(width, height)
        .viewport(Viewport {
            width,
            height,
            device_scale_factor: Some(1.0),
            ..Viewport::default()
        })
        // Same pixels on every machine: no GPU raster differences, no scrollbars, and greyscale
        // unhinted glyphs whatever the system's fontconfig says. Chrome takes subpixel (LCD)
        // rendering from fontconfig, not only from its flags: a system set to `rgba rgb` gave
        // colour-fringed text (3% of the dashboard fixture's pixels) despite --disable-lcd-text.
        .arg("--disable-gpu")
        .arg("--hide-scrollbars")
        .arg("--font-render-hinting=none")
        .arg("--disable-lcd-text")
        .env(
            "FONTCONFIG_FILE",
            profile.fontconfig()?.display().to_string(),
        );
    if !settings.sandbox {
        config = config.no_sandbox();
    }
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
    // A redirect keeps the request's id; the URLs it went through are the ones the page names.
    let mut redirects_seen = page.event_listener::<EventRequestWillBeSent>().await?;
    let redirects = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::<
        String,
        Vec<String>,
    >::new()));
    let redirect_listener = {
        let redirects = redirects.clone();
        tokio::spawn(async move {
            while let Some(event) = redirects_seen.next().await {
                if let Some(from) = &event.redirect_response {
                    redirects
                        .lock()
                        .expect("not poisoned")
                        .entry(event.request_id.inner().clone())
                        .or_default()
                        .push(from.url.clone());
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
    redirect_listener.abort();
    let requests = std::mem::take(&mut *wanted.lock().expect("not poisoned"));
    let redirects = std::mem::take(&mut *redirects.lock().expect("not poisoned"));
    let mut images = Resources::default();
    for (i, (id, url, is_font, mime)) in requests.into_iter().enumerate() {
        let earlier = redirects.get(id.inner()).cloned().unwrap_or_default();
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
            // Under every URL of its redirect chain: the page's CSS names the first one.
            for url in earlier.into_iter().chain([url]) {
                images.images.insert(
                    url,
                    Resource {
                        file: name.clone(),
                        mime: mime.clone(),
                    },
                );
            }
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
