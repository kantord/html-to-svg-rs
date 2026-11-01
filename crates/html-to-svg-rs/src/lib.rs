//! Chrome lays a page out, Takumi paints Chrome's boxes into SVG.
//!
//! [`capture::capture`] drives headless Chromium and writes a capture directory (`snapshot.json`,
//! `chrome.png`, `fonts.json` + `fonts/`, `images.json` + `images/`). [`convert`] turns such a
//! directory into an SVG without a browser. See the workspace README for how the lowering works.

mod annotate;
mod assets;
pub mod capture;
mod fonts;
mod geometry;
mod lower;
mod resources;
mod snapshot;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use resvg::tiny_skia::Pixmap;
use takumi::prelude::*;

pub use assets::AssetsOptions;
pub use fonts::Registered as RegisteredFonts;
pub use lower::Report;
/// The computed styles a capture must ask Chrome for (`DOMSnapshot.captureSnapshot`), in order.
pub use snapshot::STYLES;

/// What [`convert`] needs besides the capture directory.
pub struct ConvertOptions {
    /// Font files registered on top of the faces Chrome used (`fonts.json`).
    pub extra_fonts: Vec<PathBuf>,
    /// Correct each line's letter-spacing so Takumi's text fills Chrome's line width.
    pub fit_text: bool,
    /// Viewport used when the capture has no `chrome.png` to take the size from.
    pub fallback_size: (u32, u32),
    /// Write embedded images to a shared folder, named by content hash, instead of inlining them
    /// as `data:` URLs in every SVG.
    pub assets: Option<AssetsOptions>,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            extra_fonts: Vec::new(),
            fit_text: true,
            fallback_size: (1024, 768),
            assets: None,
        }
    }
}

pub struct Conversion {
    /// The SVG, with each annotated element's `data-*` attributes on its groups.
    pub svg: String,
    /// Takumi's own PNG raster of the same node tree.
    pub png: Vec<u8>,
    /// Asset files the SVG refers to, under [`AssetsOptions::dir`] (empty without `assets`).
    pub assets: Vec<String>,
    /// The faces of `fonts.json` that were found and registered (not counting `extra_fonts`).
    pub fonts: RegisteredFonts,
    pub report: Report,
}

/// Converts a capture directory (written by [`capture::capture`]) into SVG.
pub fn convert(dir: &Path, options: &ConvertOptions) -> Result<Conversion> {
    let page = snapshot::Page::decode(
        &std::fs::read_to_string(dir.join("snapshot.json")).context("read snapshot.json")?,
    )?;
    let screenshot = Pixmap::load_png(dir.join("chrome.png")).ok();
    let viewport = screenshot
        .as_ref()
        .map(|s| (s.width(), s.height()))
        .unwrap_or(options.fallback_size);

    // Takumi reads no system fonts: register the same files Chrome used.
    let mut fonts = Fonts::default();
    for path in &options.extra_fonts {
        let bytes = std::fs::read(path).with_context(|| format!("read font {}", path.display()))?;
        fonts
            .register(FontResource::new(bytes))
            .with_context(|| format!("register {}", path.display()))?;
    }
    let used = match std::fs::read(dir.join("fonts.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("parse fonts.json")?,
        Err(_) => fonts::FontsFile::default(),
    };
    let registered = fonts::register(dir, &used, &mut fonts);

    let images = resources::data_urls(dir);
    let lowered = lower::lower(
        &page,
        lower::Options {
            fonts: &fonts,
            fit_text: options.fit_text,
            screenshot: screenshot.as_ref(),
            viewport,
            used_fonts: &used.used,
            images: &images,
        },
    );

    let svg = takumi::render_svg(
        SvgOptions::builder()
            .viewport(Viewport::new(viewport))
            .fonts(&fonts)
            .node(lowered.root.clone())
            .build(),
    )?;
    let image = takumi::render(
        RenderOptions::builder()
            .viewport(Viewport::new(viewport))
            .node(lowered.root)
            .fonts(&fonts)
            .build(),
    )?;
    let mut png = Vec::new();
    takumi::write_image(&image, &mut png, OutputFormat::Png)?;

    let svg = annotate::apply(&svg, &lowered.annotations);
    let (svg, assets) = match &options.assets {
        Some(assets) => assets::externalize(&svg, assets, &resources::extensions_by_hash(dir))?,
        None => (svg, Vec::new()),
    };

    Ok(Conversion {
        svg,
        assets,
        png,
        fonts: registered,
        report: lowered.report,
    })
}
