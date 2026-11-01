//! Node.js bindings of html-to-svg-rs, built by napi-rs into the npm package `html-to-svg-rs`
//! (`packages/html-to-svg-rs`): the converter runs in the Node process, no CLI binary needed.
//!
//! A capture directory (snapshot.json, chrome.png, fonts.json + fonts/, images.json + images/, as
//! written by the capture of `playwright-html-to-svg-rs` or `html-to-svg capture`) goes in; the
//! SVG and a report come out. `convert` runs on libuv's thread pool and returns a Promise.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use html_to_svg_rs::{AssetsOptions, ConvertOptions as Options};
use napi::bindgen_prelude::{AsyncTask, Buffer};
use napi::{Env, Error, Result, Status, Task};
use napi_derive::napi;

/// Options of `convert`.
#[napi(object)]
#[derive(Default)]
pub struct ConvertOptions {
    /// Font files registered on top of the faces Chrome used (the capture's fonts.json).
    pub extra_fonts: Option<Vec<String>>,
    /// Correct each line's letter-spacing so the text fills Chrome's line width (default true).
    pub fit_text: Option<bool>,
    /// Write embedded images to this folder, named by content hash, instead of inlining them.
    pub assets_dir: Option<String>,
    /// What the SVG puts before an asset's file name. Default: the path from `svgDir` (or the
    /// capture directory) to `assetsDir`.
    pub assets_href: Option<String>,
    /// Where the SVG will be written, for the default `assetsHref`. Default: the capture directory.
    pub svg_dir: Option<String>,
}

/// Faces of the capture's fonts.json that were found and registered.
#[napi(object)]
pub struct Fonts {
    pub faces: u32,
    /// Faces Chrome used that were not found (their text falls back to another face).
    pub missing: Vec<String>,
}

/// What the conversion did and could not do.
#[napi(object)]
pub struct Report {
    pub boxes: u32,
    pub text_lines: u32,
    /// |Chrome line width - our line width| per text line, in px, before and after fitting.
    pub line_width_error: Vec<f64>,
    pub line_width_error_fitted: Vec<f64>,
    /// Elements whose pixels were cropped from Chrome's screenshot (`<img>`, `<canvas>`, ...).
    pub raster_fallbacks: Vec<String>,
    /// CSS declarations the painter rejected, with how often each was dropped.
    pub dropped: HashMap<String, u32>,
    /// Things that were not drawn as Chrome drew them.
    pub warnings: Vec<String>,
}

#[napi(object)]
pub struct Conversion {
    /// The SVG. Elements with `data-*` attributes keep them on the groups they painted.
    pub svg: String,
    /// The same picture rasterized by the converter's own renderer, as PNG.
    pub png: Buffer,
    /// Files under `assetsDir` the SVG links to (empty without it).
    pub assets: Vec<String>,
    pub fonts: Fonts,
    pub report: Report,
}

/// The computed styles a capture must ask Chrome for (`DOMSnapshot.captureSnapshot`), in order.
#[napi]
pub fn styles() -> Vec<String> {
    html_to_svg_rs::STYLES
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

/// Converts a capture directory into an SVG, off the main thread.
#[napi(ts_return_type = "Promise<Conversion>")]
pub fn convert(dir: String, options: Option<ConvertOptions>) -> AsyncTask<Convert> {
    AsyncTask::new(Convert {
        dir: PathBuf::from(dir),
        options: options.unwrap_or_default(),
    })
}

pub struct Convert {
    dir: PathBuf,
    options: ConvertOptions,
}

/// The converter's result, before it becomes JavaScript values on the main thread.
pub struct Converted {
    conversion: html_to_svg_rs::Conversion,
}

impl Task for Convert {
    type Output = Converted;
    type JsValue = Conversion;

    fn compute(&mut self) -> Result<Self::Output> {
        let o = &self.options;
        let assets = match &o.assets_dir {
            Some(assets) => {
                let svg_dir = o.svg_dir.as_deref().map_or(self.dir.as_path(), Path::new);
                Some(
                    AssetsOptions::for_svg_in(svg_dir, Path::new(assets), o.assets_href.as_deref())
                        .map_err(error)?,
                )
            }
            None => None,
        };
        let options = Options {
            extra_fonts: o.extra_fonts.iter().flatten().map(PathBuf::from).collect(),
            fit_text: o.fit_text.unwrap_or(true),
            assets,
            ..Options::default()
        };
        let conversion = html_to_svg_rs::convert(&self.dir, &options).map_err(error)?;
        Ok(Converted { conversion })
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        let c = output.conversion;
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Ok(Conversion {
            svg: c.svg,
            png: c.png.into(),
            assets: c.assets,
            fonts: Fonts {
                faces: count(c.fonts.faces),
                missing: c.fonts.missing,
            },
            report: Report {
                boxes: count(c.report.boxes),
                text_lines: count(c.report.text_lines),
                line_width_error: c.report.line_width_error,
                line_width_error_fitted: c.report.line_width_error_fitted,
                raster_fallbacks: c.report.raster_fallbacks,
                dropped: c
                    .report
                    .dropped
                    .into_iter()
                    .map(|(k, v)| (k, count(v)))
                    .collect(),
                warnings: c.report.warnings,
            },
        })
    }
}

/// An error of the converter, with its chain of causes (`a: b: c`), as a JavaScript error.
fn error(e: anyhow::Error) -> Error {
    Error::new(Status::GenericFailure, format!("{e:#}"))
}
