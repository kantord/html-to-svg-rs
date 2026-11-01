//! Pixel comparison against Chrome's screenshot, and a picture of where they differ.

use std::path::Path;

use anyhow::{Context, Result};
use resvg::tiny_skia::{Pixmap, PremultipliedColorU8};

pub struct Score {
    /// Mean of the largest channel difference per pixel, 0..255.
    pub mean: f64,
    /// Share of pixels whose largest channel difference exceeds 32 (visible to the eye).
    pub visible: f64,
    /// Share of visible differences that no pixel within 1 px of the other picture explains:
    /// wrong or missing content, not anti-aliasing or a sub-pixel shift.
    pub misplaced: f64,
}

/// Renders an SVG with resvg (an independent renderer, not Takumi) at the given size. Relative
/// links (externalized assets) are resolved from `dir`, where the SVG lives.
pub fn rasterize_svg(svg: &str, dir: &Path, (w, h): (u32, u32)) -> Result<Pixmap> {
    let options = resvg::usvg::Options {
        resources_dir: Some(dir.to_owned()),
        ..resvg::usvg::Options::default()
    };
    let tree = resvg::usvg::Tree::from_str(svg, &options).context("parse SVG")?;
    let mut pixmap = Pixmap::new(w, h).context("pixmap size")?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

/// Largest channel difference of two pixels.
fn channel_diff(a: PremultipliedColorU8, b: PremultipliedColorU8) -> u8 {
    [
        a.red().abs_diff(b.red()),
        a.green().abs_diff(b.green()),
        a.blue().abs_diff(b.blue()),
    ]
    .into_iter()
    .max()
    .unwrap_or(0)
}

/// Compares `candidate` with `reference` and writes a diff picture: the reference faded to grey,
/// differences explained by a 1 px shift or anti-aliasing in orange, misplaced pixels in red.
pub fn compare(reference: &Pixmap, candidate: &Pixmap, picture: &Path) -> Result<Score> {
    let (w, h) = (reference.width(), reference.height());
    let at = |p: &Pixmap, x: i64, y: i64| -> Option<PremultipliedColorU8> {
        (x >= 0 && y >= 0 && (x as u32) < p.width() && (y as u32) < p.height())
            .then(|| p.pixels()[(y as u32 * p.width() + x as u32) as usize])
    };
    // Whether `px` matches some pixel of `img` within 1 px of (x, y).
    let near = |img: &Pixmap, px: PremultipliedColorU8, x: i64, y: i64| {
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| at(img, x + dx, y + dy).is_some_and(|q| channel_diff(px, q) <= 32))
        })
    };
    let mut out = Pixmap::new(w, h).context("pixmap size")?;
    let (mut sum, mut visible, mut misplaced) = (0.0, 0usize, 0usize);
    for (i, o) in out.pixels_mut().iter_mut().enumerate() {
        let (x, y) = ((i as u32 % w) as i64, (i as u32 / w) as i64);
        let r = at(reference, x, y).expect("in bounds");
        let c = at(candidate, x, y).unwrap_or(r);
        let d = channel_diff(r, c);
        sum += d as f64;
        let grey = ((r.red() as u32 + r.green() as u32 + r.blue() as u32) / 3 / 2 + 128) as u8;
        let (red, green, blue) = if d <= 32 {
            (grey, grey, grey)
        } else if near(candidate, r, x, y) || near(reference, c, x, y) {
            visible += 1;
            (255, 170, 0)
        } else {
            visible += 1;
            misplaced += 1;
            (255, 0, 0)
        };
        *o = PremultipliedColorU8::from_rgba(red, green, blue, 255).expect("opaque");
    }
    out.save_png(picture).context("write diff picture")?;
    let n = (w as usize * h as usize).max(1) as f64;
    Ok(Score {
        mean: sum / n,
        visible: visible as f64 / n,
        misplaced: misplaced as f64 / n,
    })
}
