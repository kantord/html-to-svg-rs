//! chrome-svg: Chrome lays a page out, Takumi paints Chrome's boxes into SVG.
//!
//! ```text
//! chrome-svg capture <url> <dir> [--size WxH] [--settle SECONDS]
//!                         Chrome -> dir/snapshot.json, chrome.png, fonts.json, fonts/
//! chrome-svg convert <dir> [--font F]... [--no-fit]
//!                         snapshot -> dir/out.svg + dir/takumi.png (fonts.json's faces are
//!                         registered by themselves; --font adds more)
//! chrome-svg diff <dir>   out.svg (via resvg) and takumi.png vs chrome.png
//! chrome-svg run <url> <dir> [any of the above]   all three
//! ```

mod annotate;
mod capture;
mod diff;
mod fonts;
mod geometry;
mod lower;
mod resources;
mod snapshot;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use resvg::tiny_skia::Pixmap;
use takumi::prelude::*;

struct Args {
    positional: Vec<String>,
    fonts: Vec<PathBuf>,
    size: (u32, u32),
    settle: f64,
    fit_text: bool,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        positional: Vec::new(),
        fonts: Vec::new(),
        size: (1024, 768),
        settle: 2.0,
        fit_text: true,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--font" => args
                .fonts
                .push(it.next().context("--font needs a path")?.into()),
            "--size" => {
                let v = it.next().context("--size needs WxH")?;
                let (w, h) = v.split_once('x').context("--size is WxH")?;
                args.size = (w.parse()?, h.parse()?);
            }
            "--settle" => args.settle = it.next().context("--settle needs seconds")?.parse()?,
            "--no-fit" => args.fit_text = false,
            _ => args.positional.push(a),
        }
    }
    Ok(args)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args()?;
    let p: Vec<&str> = args.positional.iter().map(String::as_str).collect();
    match p[..] {
        ["capture", url, dir] => capture(url, Path::new(dir), &args).await,
        ["convert", dir] => convert(Path::new(dir), &args),
        ["diff", dir] => diff(Path::new(dir)),
        ["run", url, dir] => {
            capture(url, Path::new(dir), &args).await?;
            convert(Path::new(dir), &args)?;
            diff(Path::new(dir))
        }
        _ => bail!("usage: see the top of src/main.rs or the README"),
    }
}

async fn capture(url: &str, dir: &Path, args: &Args) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let url = if url.contains("://") {
        url.to_owned()
    } else {
        format!(
            "file://{}",
            std::fs::canonicalize(url)
                .with_context(|| format!("find {url}"))?
                .display()
        )
    };
    let settings = capture::Settings {
        size: args.size,
        settle: std::time::Duration::from_secs_f64(args.settle),
    };
    capture::capture(&url, dir, &settings).await?;
    println!(
        "captured {url} -> {}/snapshot.json, chrome.png",
        dir.display()
    );
    Ok(())
}

fn convert(dir: &Path, args: &Args) -> Result<()> {
    let page = snapshot::Page::decode(&std::fs::read_to_string(dir.join("snapshot.json"))?)?;
    let screenshot = Pixmap::load_png(dir.join("chrome.png")).ok();
    let viewport = screenshot
        .as_ref()
        .map(|s| (s.width(), s.height()))
        .unwrap_or(args.size);

    // Takumi reads no system fonts: register the same files Chrome used.
    let mut fonts = Fonts::default();
    for path in &args.fonts {
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
    println!(
        "fonts: {} face(s) Chrome used registered{}",
        registered.faces,
        if registered.missing.is_empty() {
            String::new()
        } else {
            format!("; not found: {}", registered.missing.join(", "))
        }
    );
    if args.fonts.is_empty() && registered.faces == 0 {
        eprintln!("warning: no fonts (no fonts.json, no --font); Takumi draws no text");
    }

    let images = resources::data_urls(dir);
    let lowered = lower::lower(
        &page,
        lower::Options {
            fonts: &fonts,
            fit_text: args.fit_text,
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
    std::fs::write(
        dir.join("out.svg"),
        annotate::apply(&svg, &lowered.annotations),
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
    std::fs::write(dir.join("takumi.png"), png)?;

    print_report(&lowered.report);
    println!("wrote {0}/out.svg, {0}/takumi.png", dir.display());
    Ok(())
}

fn print_report(r: &lower::Report) {
    let stats = |v: &[f64]| {
        let mean = v.iter().sum::<f64>() / v.len().max(1) as f64;
        let max = v.iter().copied().fold(0.0, f64::max);
        format!("mean {mean:.2}px, max {max:.2}px over {} lines", v.len())
    };
    println!("boxes: {}, text lines: {}", r.boxes, r.text_lines);
    println!(
        "line width error, Takumi vs Chrome: {}",
        stats(&r.line_width_error)
    );
    if !r.line_width_error_fitted.is_empty() {
        println!(
            "  after letter-spacing fit:        {}",
            stats(&r.line_width_error_fitted)
        );
    }
    if !r.raster_fallbacks.is_empty() {
        println!(
            "raster fallbacks (pixels from Chrome): {}",
            r.raster_fallbacks.join(", ")
        );
    }
    for (decl, n) in &r.dropped {
        println!("dropped (Takumi rejected) x{n}: {decl}");
    }
    for w in &r.warnings {
        println!("warning: {w}");
    }
}

fn diff(dir: &Path) -> Result<()> {
    let reference = Pixmap::load_png(dir.join("chrome.png")).context("read chrome.png")?;
    let size = (reference.width(), reference.height());
    let svg = diff::rasterize_svg(&std::fs::read_to_string(dir.join("out.svg"))?, size)?;
    svg.save_png(dir.join("svg-via-resvg.png"))?;
    let takumi = Pixmap::load_png(dir.join("takumi.png")).context("read takumi.png")?;
    for (name, candidate) in [("svg", &svg), ("takumi", &takumi)] {
        let score = diff::compare(&reference, candidate, &dir.join(format!("diff-{name}.png")))?;
        println!(
            "{name:>6} vs chrome: mean channel diff {:.2}/255, visibly different pixels {:.2}%, misplaced {:.2}%  (diff-{name}.png)",
            score.mean,
            score.visible * 100.0,
            score.misplaced * 100.0
        );
    }
    Ok(())
}
