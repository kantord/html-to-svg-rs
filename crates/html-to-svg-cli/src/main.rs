//! html-to-svg: Chrome lays a page out, Takumi paints Chrome's boxes into SVG
//! (the `html-to-svg-rs` library does the work).
//!
//! ```text
//! html-to-svg capture <url> <dir> [--size WxH] [--settle SECONDS] [--no-sandbox]
//!                         Chrome -> dir/snapshot.json, chrome.png, fonts.json, fonts/
//! html-to-svg convert <dir> [--font F]... [--no-fit] [--json] [--assets DIR [--assets-href P]]
//!                         snapshot -> dir/out.svg + dir/takumi.png (fonts.json's faces are
//!                         registered by themselves; --font adds more). --assets writes
//!                         embedded images to DIR as <sha256>.<ext>, shared between SVGs;
//!                         the SVG links them by a path relative to <dir>, or by P + name
//! html-to-svg diff <dir>   out.svg (via resvg) and takumi.png vs chrome.png
//! html-to-svg run <url> <dir> [any of the above]   all three
//! html-to-svg styles      the computed styles a capture asks Chrome for, as a JSON array
//!                         (for captures made outside this binary, e.g. from Playwright)
//! ```

mod diff;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use html_to_svg_rs::{AssetsOptions, ConvertOptions, Report, capture};
use resvg::tiny_skia::Pixmap;

struct Args {
    positional: Vec<String>,
    fonts: Vec<PathBuf>,
    size: (u32, u32),
    settle: f64,
    fit_text: bool,
    /// Run Chrome without its sandbox (also `HTML_TO_SVG_NO_SANDBOX=1`): only where it cannot
    /// have one, e.g. some containers.
    no_sandbox: bool,
    /// `convert` prints its report as one JSON object instead of text.
    json: bool,
    /// `convert` writes embedded images here instead of inlining them.
    assets: Option<PathBuf>,
    /// What the SVG puts before an asset's file name; default: the path from the SVG's folder.
    assets_href: Option<String>,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        positional: Vec::new(),
        fonts: Vec::new(),
        size: (1024, 768),
        settle: 2.0,
        fit_text: true,
        no_sandbox: std::env::var("HTML_TO_SVG_NO_SANDBOX").is_ok_and(|v| v == "1"),
        json: false,
        assets: None,
        assets_href: None,
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
            "--json" => args.json = true,
            "--no-sandbox" => args.no_sandbox = true,
            "--assets" => args.assets = Some(it.next().context("--assets needs a folder")?.into()),
            "--assets-href" => {
                args.assets_href = Some(it.next().context("--assets-href needs a prefix")?)
            }
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
        ["styles"] => {
            println!("{}", serde_json::to_string(html_to_svg_rs::STYLES)?);
            Ok(())
        }
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
        sandbox: !args.no_sandbox,
    };
    capture::capture(&url, dir, &settings).await?;
    println!(
        "captured {url} -> {}/snapshot.json, chrome.png",
        dir.display()
    );
    Ok(())
}

fn convert(dir: &Path, args: &Args) -> Result<()> {
    let conversion = html_to_svg_rs::convert(
        dir,
        &ConvertOptions {
            extra_fonts: args.fonts.clone(),
            fit_text: args.fit_text,
            fallback_size: args.size,
            assets: args
                .assets
                .as_ref()
                .map(|assets| assets_options(dir, assets, args.assets_href.as_deref()))
                .transpose()?,
        },
    )?;
    std::fs::write(dir.join("out.svg"), &conversion.svg)?;
    std::fs::write(dir.join("takumi.png"), &conversion.png)?;
    if args.json {
        let report = serde_json::json!({
            "fonts": conversion.fonts,
            "report": conversion.report,
            "assets": conversion.assets,
        });
        println!("{report}");
        return Ok(());
    }
    let registered = &conversion.fonts;
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
    print_report(&conversion.report);
    if let Some(assets) = &args.assets {
        println!(
            "assets: {} file(s) in {}",
            conversion.assets.len(),
            assets.display()
        );
    }
    println!("wrote {0}/out.svg, {0}/takumi.png", dir.display());
    Ok(())
}

/// The SVG (written to `dir`) links each asset by `href` + file name; by default `href` is the
/// path from `dir` to the assets folder.
fn assets_options(dir: &Path, assets: &Path, href: Option<&str>) -> Result<AssetsOptions> {
    std::fs::create_dir_all(assets).with_context(|| format!("create {}", assets.display()))?;
    let href_prefix = match href {
        Some(href) => href.to_owned(),
        None => {
            let from = std::fs::canonicalize(dir)?;
            let to = std::fs::canonicalize(assets)?;
            let common = from
                .components()
                .zip(to.components())
                .take_while(|(a, b)| a == b)
                .count();
            let mut parts: Vec<String> = from
                .components()
                .skip(common)
                .map(|_| "..".into())
                .collect();
            parts.extend(
                to.components()
                    .skip(common)
                    .map(|c| c.as_os_str().to_string_lossy().into_owned()),
            );
            parts.iter().map(|p| format!("{p}/")).collect()
        }
    };
    Ok(AssetsOptions {
        dir: assets.to_owned(),
        href_prefix,
    })
}

fn print_report(r: &Report) {
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
    let svg = diff::rasterize_svg(&std::fs::read_to_string(dir.join("out.svg"))?, dir, size)?;
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
