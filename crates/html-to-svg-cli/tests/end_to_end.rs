//! The whole conversion on a committed Chrome capture of `pages/dashboard.html`, without a
//! browser: snapshot -> Takumi -> SVG, then the SVG (rendered by resvg) against Chrome's pixels.
//! Recapture the fixture with `html-to-svg capture pages/dashboard.html <dir> --size 1024x640`.

use std::path::{Path, PathBuf};
use std::process::Command;

const INTER: &str = "/usr/share/fonts/opentype/inter";
const MONO: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf";

fn fonts() -> Option<Vec<String>> {
    let files: Vec<String> = ["Regular", "Medium", "SemiBold", "Bold", "Italic"]
        .iter()
        .map(|w| format!("{INTER}/Inter-{w}.otf"))
        .chain([MONO.to_owned()])
        .collect();
    files.iter().all(|f| Path::new(f).exists()).then_some(files)
}

fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_html-to-svg"))
        .args(args)
        .output()
        .expect("run html-to-svg");
    assert!(
        out.status.success(),
        "html-to-svg {args:?} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 output")
}

/// The number after `label` in the tool's report, e.g. `visibly different pixels 0.28%`.
fn number_after(text: &str, line_start: &str, label: &str) -> f64 {
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with(line_start))
        .expect("report line");
    let rest = &line[line.find(label).expect("label") + label.len()..];
    rest.trim_start()
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn dashboard_converts_close_to_chrome_with_annotations() {
    // The fixture was captured with Inter and DejaVu Sans Mono; other fonts would not compare.
    if fonts().is_none() {
        // The test container (`just test`) has them: there, a missing font is a failure.
        assert!(
            std::env::var("HTML_TO_SVG_TEST_ENV").as_deref() != Ok("container"),
            "the Inter and DejaVu Sans Mono font files are missing from the test container"
        );
        eprintln!("skipped: the Inter and DejaVu Sans Mono font files are not installed");
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dashboard");
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["snapshot.json", "chrome.png", "fonts.json"] {
        std::fs::copy(Path::new("tests/fixtures/dashboard").join(f), dir.join(f)).unwrap();
    }
    let dir_arg = dir.to_str().unwrap();
    // No --font: fonts.json names the faces Chrome used, found again through fc-list.
    let report = run(&["convert", dir_arg]);
    assert!(
        report.contains("fonts: 5 face(s)") && !report.contains("not found"),
        "the faces Chrome used were not all registered:\n{report}"
    );
    let scores = run(&["diff", dir_arg]);

    // Measured 0.19% visibly different, 0.03% misplaced when the fixture was captured
    // (2026-10-31); the bound leaves room for renderer updates.
    let svg_off = number_after(&scores, "svg vs chrome", "visibly different pixels");
    assert!(
        svg_off < 1.0,
        "SVG drifted from Chrome: {svg_off}% of pixels\n{scores}"
    );
    // Same fonts and font features: Takumi's shaping should land within a pixel of Chrome's.
    let line_err = number_after(&report, "line width error", "mean");
    assert!(
        line_err < 0.5,
        "text widths drifted: {line_err}px\n{report}"
    );

    let svg = std::fs::read_to_string(dir.join("out.svg")).unwrap();
    for id in [
        "header",
        "summary",
        "highlight",
        "row-us",
        "badge",
        "sparkline",
    ] {
        assert!(
            svg.contains(&format!("data-testid=\"{id}\"")),
            "annotation {id} missing from the SVG"
        );
    }
    assert!(
        svg.contains("<image"),
        "the canvas raster fallback is missing"
    );
    assert!(
        !svg.contains("data-takumi-node"),
        "internal markers must be replaced"
    );
}
