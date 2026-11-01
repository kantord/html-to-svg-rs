//! One test per tracked bug (`wpt/bugs/<bug>.md`), on a page of our own that isolates it
//! (`tests/regressions/<case>/page.*`, captured by Chrome: `just regression-capture <case>`).
//!
//! Each test converts its capture, draws the SVG with resvg and counts the pixels that differ from
//! Chrome's screenshot by more than anti-aliasing would (`misplaced`, the WPT runner's rule). What
//! it asserts follows the bug's `status`:
//! - `open`: the bug still reproduces (more than `THRESHOLD` misplaced pixels). A fix makes this
//!   fail, with the message to mark the bug fixed;
//! - `fixed`: the page matches Chrome (at most `THRESHOLD`).
//!
//! The pictures of the last run are written to `target/tmp/regressions/<case>/` (ours.png,
//! diff.png: red where misplaced).

use std::path::{Path, PathBuf};

use html_to_svg_rs::{ConvertOptions, convert};
use resvg::tiny_skia::{Pixmap, PremultipliedColorU8};

/// The WPT runner's threshold: rounded edges anti-aliased differently reach about 30.
const THRESHOLD: usize = 30;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The `status:` line of `wpt/bugs/<bug>.md`.
fn bug_status(bug: &str) -> String {
    let path = repo().join("wpt/bugs").join(format!("{bug}.md"));
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.lines()
        .find_map(|l| l.strip_prefix("status:"))
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| panic!("{} has no status line", path.display()))
}

fn channel(a: PremultipliedColorU8, b: PremultipliedColorU8) -> u8 {
    [
        a.red().abs_diff(b.red()),
        a.green().abs_diff(b.green()),
        a.blue().abs_diff(b.blue()),
    ]
    .into_iter()
    .max()
    .unwrap_or(0)
}

/// Pixels where a channel differs by more than 32 and no pixel within 1 px of the other picture
/// explains it; writes a picture of them.
fn misplaced(chrome: &Pixmap, ours: &Pixmap, picture: &Path) -> usize {
    let (w, h) = (chrome.width() as i64, chrome.height() as i64);
    let at = |p: &Pixmap, x: i64, y: i64| p.pixels()[(y * w + x) as usize];
    let near = |a: &Pixmap, b: &Pixmap, x: i64, y: i64| {
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                let (nx, ny) = (x + dx, y + dy);
                nx >= 0 && ny >= 0 && nx < w && ny < h && channel(at(a, x, y), at(b, nx, ny)) <= 32
            })
        })
    };
    let mut out = Pixmap::new(w as u32, h as u32).expect("size");
    let mut count = 0;
    for y in 0..h {
        for x in 0..w {
            let (c, o) = (at(chrome, x, y), at(ours, x, y));
            let bad = channel(c, o) > 32 && !near(chrome, ours, x, y) && !near(ours, chrome, x, y);
            count += usize::from(bad);
            let grey = ((c.red() as u32 + c.green() as u32 + c.blue() as u32) / 9 + 170) as u8;
            let px = if bad {
                PremultipliedColorU8::from_rgba(230, 0, 0, 255)
            } else {
                PremultipliedColorU8::from_rgba(grey, grey, grey, 255)
            };
            out.pixels_mut()[(y * w + x) as usize] = px.expect("opaque");
        }
    }
    out.save_png(picture).expect("write diff.png");
    count
}

/// Converts `tests/regressions/<case>/`, compares it with Chrome, and asserts what `bug`'s status
/// says.
fn case(name: &str, bug: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/regressions")
        .join(name);
    let conversion = convert(&dir, &ConvertOptions::default()).expect("convert");
    let chrome = Pixmap::load_png(dir.join("chrome.png")).expect("chrome.png");
    let tree = resvg::usvg::Tree::from_str(&conversion.svg, &resvg::usvg::Options::default())
        .expect("parse SVG");
    let mut ours = Pixmap::new(chrome.width(), chrome.height()).expect("size");
    ours.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut ours.as_mut(),
    );

    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("regressions")
        .join(name);
    std::fs::create_dir_all(&out).expect("create output folder");
    ours.save_png(out.join("ours.png")).expect("write ours.png");
    let count = misplaced(&chrome, &ours, &out.join("diff.png"));
    println!(
        "{name}: {count} misplaced pixels (pictures in {})",
        out.display()
    );

    match bug_status(bug).as_str() {
        "open" => assert!(
            count > THRESHOLD,
            "{name} now matches Chrome ({count} misplaced pixels): bug {bug} looks fixed; set \
             `status: fixed` in wpt/bugs/{bug}.md"
        ),
        "fixed" => assert!(
            count <= THRESHOLD,
            "{name} differs from Chrome in {count} misplaced pixels, but bug {bug} is marked fixed \
             (pictures in {})",
            out.display()
        ),
        other => panic!("wpt/bugs/{bug}.md: unknown status {other:?}"),
    }
}

#[test]
fn xhtml_canvas() {
    case("xhtml-canvas", "xhtml-tag-case");
}

#[test]
fn canvas_background_image() {
    case("canvas-background-image", "canvas-background-image");
}

#[test]
fn body_background_containment() {
    case("body-background-containment", "body-background-containment");
}

#[test]
fn background_clip_content_box() {
    case(
        "background-clip-content-box",
        "background-clip-not-forwarded",
    );
}

#[test]
fn background_clip_text() {
    case("background-clip-text", "background-clip-not-forwarded");
}

#[test]
fn background_origin_content_box() {
    case(
        "background-origin-content-box",
        "background-origin-not-forwarded",
    );
}

#[test]
fn corner_shape_bevel() {
    case("corner-shape-bevel", "corner-shape-not-forwarded");
}
