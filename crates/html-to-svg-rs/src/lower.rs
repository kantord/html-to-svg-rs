//! Lowers a decoded Chrome snapshot into a Takumi node tree that leaves Takumi no layout to do.
//!
//! Every box becomes `position: absolute` with Chrome's left/top/width/height, relative to its
//! parent's padding box, so Takumi only paints. The tree keeps the DOM nesting (clips, opacity,
//! transforms and filters only work as nesting) and orders siblings by Chrome's paint order.
//! Each text line Chrome laid out becomes its own non-wrapping text node, optionally with a
//! letter-spacing correction so Takumi's shaping fills exactly Chrome's line width.

use std::collections::{BTreeMap, HashMap};

use resvg::tiny_skia::{IntRect, Pixmap};
use serde::Serialize;
use takumi::prelude::*;

use crate::fonts::{FontUse, family_list};
use crate::geometry::Affine;
use crate::resources::inline_urls;
use crate::snapshot::{Element, Fragment, Kind, Page};

/// Elements whose pixels come from Chrome's screenshot instead of being redrawn: replaced
/// content Takumi cannot know (images, canvas, video, frames) and native form controls.
const RASTER_TAGS: &[&str] = &[
    "IMG", "CANVAS", "VIDEO", "IFRAME", "SVG", "OBJECT", "EMBED", "INPUT", "TEXTAREA", "SELECT",
];

/// Box styles copied as they are. Text styles are copied onto the text nodes instead.
const BOX_STYLES: &[&str] = &[
    "background-color",
    "background-image",
    "background-size",
    "background-position",
    "background-repeat",
    "background-clip",
    "background-origin",
    // Padding changes no size (border-box) and no child's place (children are absolute, from the
    // padding box): it only gives Takumi the content box, for background-origin/clip.
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-top-style",
    "border-right-style",
    "border-bottom-style",
    "border-left-style",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-right-radius",
    "border-bottom-left-radius",
    "corner-top-left-shape",
    "corner-top-right-shape",
    "corner-bottom-right-shape",
    "corner-bottom-left-shape",
    "box-shadow",
    "opacity",
    "filter",
    "mix-blend-mode",
    "clip-path",
    "mask-image",
    "mask-size",
    "mask-position",
    "mask-repeat",
    "outline-width",
    "outline-style",
    "outline-color",
    "outline-offset",
    "visibility",
];

const TEXT_STYLES: &[&str] = &[
    "color",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "font-variant-numeric",
    "font-feature-settings",
    "font-variation-settings",
    "font-kerning",
    "word-spacing",
    "text-decoration-line",
    "text-decoration-style",
    "text-decoration-color",
    "text-decoration-thickness",
    "text-shadow",
];

const TRANSPARENT: &str = "rgba(0, 0, 0, 0)";

pub struct Options<'a> {
    pub fonts: &'a Fonts,
    /// Correct each line's letter-spacing so Takumi's text fills Chrome's line width.
    pub fit_text: bool,
    /// Chrome's screenshot of the same viewport, for [`RASTER_TAGS`].
    pub screenshot: Option<&'a Pixmap>,
    pub viewport: (u32, u32),
    /// The faces Chrome drew each element's text with (see [`crate::fonts`]).
    pub used_fonts: &'a BTreeMap<usize, Vec<FontUse>>,
    /// Captured image URL -> `data:` URL (see [`crate::resources`]).
    pub images: &'a BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub boxes: usize,
    pub text_lines: usize,
    /// |Chrome line width - Takumi line width| before and after the letter-spacing fit, in px.
    pub line_width_error: Vec<f64>,
    pub line_width_error_fitted: Vec<f64>,
    pub raster_fallbacks: Vec<String>,
    /// Declarations Takumi could not parse, with how often each was dropped.
    pub dropped: BTreeMap<String, usize>,
    pub warnings: Vec<String>,
}

pub struct Lowered {
    pub root: Node,
    /// Takumi node id -> the `data-*` attributes of the element it came from.
    pub annotations: HashMap<String, BTreeMap<String, String>>,
    pub report: Report,
}

/// What a child needs to know about its parent's box.
#[derive(Clone)]
struct Parent {
    /// Maps the parent's untransformed page coordinates to page coordinates (all ancestor
    /// transforms, the parent's own included).
    to_page: Affine,
    /// The parent's padding-box top-left, in untransformed page coordinates. Absolute children
    /// are positioned from here.
    origin: (f64, f64),
    /// The annotation id the parent's text inherits, if the parent is annotated.
    annotation: Option<String>,
    /// The parent's snapshot index: text nodes look up their element's fonts with it.
    dom_index: usize,
}

struct Lowerer<'a> {
    options: Options<'a>,
    annotations: HashMap<String, BTreeMap<String, String>>,
    report: Report,
    /// Elements whose background was moved to the canvas (CSS background propagation).
    background_on_canvas: Option<usize>,
}

pub fn lower(page: &Page, options: Options<'_>) -> Lowered {
    let (vw, vh) = options.viewport;
    let mut lowerer = Lowerer {
        options,
        annotations: HashMap::new(),
        report: Report::default(),
        background_on_canvas: None,
    };
    let canvas = lowerer.canvas_background(&page.root, (vw as f64, vh as f64));
    let top = Parent {
        to_page: Affine::IDENTITY,
        origin: (0.0, 0.0),
        annotation: None,
        dom_index: usize::MAX,
    };
    let html = lowerer.element(&page.root, &top);
    // The canvas background covers the whole viewport even when the document is shorter, as a
    // layer of its own under the document's boxes.
    let canvas = Node::container([]).with_style(lowerer.style(&canvas));
    let style = lowerer.style(&[
        ("position", "relative".into()),
        ("width", px(vw as f64)),
        ("height", px(vh as f64)),
        ("overflow", "hidden".into()),
    ]);
    Lowered {
        root: Node::container(std::iter::once(canvas).chain(html).collect::<Vec<_>>())
            .with_style(style),
        annotations: lowerer.annotations,
        report: lowerer.report,
    }
}

impl Lowerer<'_> {
    /// The canvas layer: CSS paints the root's background, or else the body's, over the whole
    /// canvas, and that element then does not paint it again on its own box. Images are
    /// positioned against the root element's box (its `background-origin` box) but tile over the
    /// whole canvas: the layer is at least the viewport, its content box is that positioning
    /// area (`background-origin: content-box` with padding up to it), and it paints its border
    /// box (`background-clip: border-box`).
    fn canvas_background(
        &mut self,
        root: &Element,
        (vw, vh): (f64, f64),
    ) -> Vec<(&'static str, String)> {
        let white = vec![
            ("position", "absolute".into()),
            ("left", "0".into()),
            ("top", "0".into()),
            ("width", px(vw)),
            ("height", px(vh)),
            ("background-color", "white".into()),
        ];
        // The snapshot's root box is the document's; `<html>` is its child.
        let Some(html) = (root.tag() == "HTML")
            .then_some(root)
            .or_else(|| child_tagged(root, "HTML"))
        else {
            return white;
        };
        let source = if has_background(html) {
            html
        } else {
            match child_tagged(html, "BODY") {
                // The body's background is not propagated when either has paint, layout or size
                // containment (CSS Containment, "Paint containment"; WPT background-color-body-
                // propagation-008/009).
                Some(body) if contains(html) || contains(body) => return white,
                Some(body) if has_background(body) => body,
                _ => return white,
            }
        };
        self.background_on_canvas = Some(source.dom_index);

        // The positioning area: the root element's box, as its `background-origin` (the used
        // value of the propagated element's, first layer) says.
        let side = |el: &Element, prop: &str, s: &str| {
            parse_px(el.style(&format!("{prop}-{s}"))).unwrap_or(0.0)
        };
        let inset = |s: &str| match source
            .style("background-origin")
            .split(',')
            .next()
            .map(str::trim)
        {
            Some("border-box") => 0.0,
            Some("content-box") => {
                side(html, "border", &format!("{s}-width")) + side(html, "padding", s)
            }
            _ => side(html, "border", &format!("{s}-width")),
        };
        let b = html.bounds;
        let (left, top) = (
            (b.x + inset("left")).max(0.0),
            (b.y + inset("top")).max(0.0),
        );
        let (right, bottom) = (b.x + b.w - inset("right"), b.y + b.h - inset("bottom"));
        let (width, height) = (vw.max(right), vh.max(bottom));

        let color = source.style("background-color");
        let mut decls = vec![
            ("position", "absolute".into()),
            ("box-sizing", "border-box".into()),
            ("left", "0".into()),
            ("top", "0".into()),
            ("width", px(width)),
            ("height", px(height)),
            ("padding-left", px(left)),
            ("padding-top", px(top)),
            ("padding-right", px((width - right).max(0.0))),
            ("padding-bottom", px((height - bottom).max(0.0))),
            // Under a transparent root the canvas is the viewer's white.
            (
                "background-color",
                if is_transparent(color) {
                    "white".into()
                } else {
                    color.to_owned()
                },
            ),
        ];
        let image = source.style("background-image");
        if !image.is_empty() && image != "none" {
            match inline_urls(image, self.options.images) {
                Some(image) => {
                    let layers = layer_count(&image);
                    decls.extend([
                        ("background-image", image),
                        (
                            "background-size",
                            source.style("background-size").to_owned(),
                        ),
                        (
                            "background-position",
                            source.style("background-position").to_owned(),
                        ),
                        (
                            "background-repeat",
                            source.style("background-repeat").to_owned(),
                        ),
                        ("background-origin", vec!["content-box"; layers].join(", ")),
                        ("background-clip", vec!["border-box"; layers].join(", ")),
                    ]);
                }
                None => self.report.warnings.push(format!(
                    "<{}> #{}: canvas background-image url() was not captured",
                    source.tag(),
                    source.dom_index
                )),
            }
        }
        decls.retain(|(_, v)| !v.is_empty());
        decls
    }

    fn element(&mut self, el: &Element, parent: &Parent) -> Option<Node> {
        let Kind::Element { tag, children } = &el.kind else {
            return Some(self.text(el, parent));
        };
        self.report.boxes += 1;

        let own = Affine::parse_css(el.style("transform")).unwrap_or_else(|| {
            self.report.warnings.push(format!(
                "<{tag}> #{}: 3D transform drawn flat",
                el.dom_index
            ));
            Affine::IDENTITY
        });
        let to_page_linear = parent.to_page.then_inner(own);
        let (w, h) = el
            .offset_size
            .or_else(|| to_page_linear.unproject_size((el.bounds.w, el.bounds.h)))
            .unwrap_or((el.bounds.w, el.bounds.h));
        let (ox, oy) = parse_origin(el.style("transform-origin"), (w, h));
        // Where the box's centre moves under its own transform, relative to the box.
        let (lx, ly) = own.apply_linear((w / 2.0 - ox, h / 2.0 - oy));
        let shift = (ox + lx + own.e - w / 2.0, oy + ly + own.f - h / 2.0);
        let (cx, cy) = parent
            .to_page
            .invert()
            .unwrap_or(Affine::IDENTITY)
            .apply(el.bounds.center());
        let (x, y) = (cx - shift.0 - w / 2.0, cy - shift.1 - h / 2.0);

        let annotation = (!el.annotations.is_empty()).then(|| {
            let id = format!("n{}", el.dom_index);
            self.annotations.insert(id.clone(), el.annotations.clone());
            id
        });

        let mut decls: Vec<(&str, String)> = vec![
            ("position", "absolute".into()),
            ("box-sizing", "border-box".into()),
            ("margin", "0".into()),
            ("left", px(x - parent.origin.0)),
            ("top", px(y - parent.origin.1)),
            ("width", px(w)),
            ("height", px(h)),
        ];

        // The legacy `clip` (screen-reader-only text uses `rect(0 0 0 0)`).
        if matches!(el.style("position"), "absolute" | "fixed")
            && let Some([t, r, b, l]) = parse_clip_rect(el.style("clip"), (w, h))
        {
            if r <= l || b <= t {
                return None;
            }
            decls.push((
                "clip-path",
                format!("inset({}px {}px {}px {}px)", t, w - r, h - b, l),
            ));
        }

        if RASTER_TAGS.contains(&tag.as_str()) {
            let image = self.raster(el, tag, &to_page_linear);
            let node = Node::container(image.into_iter().collect::<Vec<_>>())
                .with_style(self.style(&decls));
            return Some(with_id(node, annotation));
        }

        for &name in BOX_STYLES {
            let value = el.style(name);
            if value.is_empty()
                || (name.starts_with("background")
                    && self.background_on_canvas == Some(el.dom_index))
            {
                continue;
            }
            // Chrome keeps some masks only in the prefixed properties.
            let value = match name.strip_prefix("mask-") {
                Some(rest) if value == "none" || value.is_empty() => {
                    el.style(&format!("-webkit-mask-{rest}"))
                }
                _ => value,
            };
            if value.is_empty() || (name == "mask-image" && value == "none") {
                continue;
            }
            // Images Chrome downloaded are inlined as data: URLs; others cannot be drawn.
            let value = if value.contains("url(") {
                match inline_urls(value, self.options.images) {
                    Some(v) => v,
                    None => {
                        self.report.warnings.push(format!(
                            "<{tag}> #{}: {name} url() was not captured",
                            el.dom_index
                        ));
                        continue;
                    }
                }
            } else {
                value.to_owned()
            };
            decls.push((name, value));
        }
        // Takumi draws no scrollbars; a scroll container still clips.
        let overflow = |v: &str| {
            if matches!(v, "auto" | "scroll") {
                "hidden".to_owned()
            } else {
                v.to_owned()
            }
        };
        for name in ["overflow-x", "overflow-y"] {
            if !el.style(name).is_empty() {
                decls.push((name, overflow(el.style(name))));
            }
        }
        if !own.is_identity() {
            decls.push(("transform", el.style("transform").to_owned()));
            decls.push(("transform-origin", format!("{} {}", px(ox), px(oy))));
        }

        let border =
            |side: &str| parse_px(el.style(&format!("border-{side}-width"))).unwrap_or(0.0);
        let child_parent = Parent {
            to_page: parent
                .to_page
                .then_inner(Affine::translate(x + ox, y + oy))
                .then_inner(own)
                .then_inner(Affine::translate(-(x + ox), -(y + oy))),
            origin: (x + border("left"), y + border("top")),
            annotation: annotation.clone().or_else(|| parent.annotation.clone()),
            dom_index: el.dom_index,
        };
        let kids: Vec<Node> = children
            .iter()
            .filter_map(|c| self.element(c, &child_parent))
            .collect();
        Some(with_id(
            Node::container(kids).with_style(self.style(&decls)),
            annotation,
        ))
    }

    /// A text node: one absolutely placed, non-wrapping line per Chrome fragment.
    fn text(&mut self, el: &Element, parent: &Parent) -> Node {
        let Kind::Text { fragments } = &el.kind else {
            unreachable!("called for text nodes")
        };
        let collapse = matches!(
            el.style("white-space-collapse"),
            "" | "collapse" | "preserve-breaks"
        );
        let lines: Vec<Node> = fragments
            .iter()
            .filter_map(|f| self.line(el, f, collapse, parent))
            .collect();
        // The wrapper has no box of its own: it sits at the parent's origin.
        let wrapper = self.style(&[
            ("position", "absolute".into()),
            ("left", "0".into()),
            ("top", "0".into()),
        ]);
        Node::container(lines).with_style(wrapper)
    }

    fn line(
        &mut self,
        el: &Element,
        fragment: &Fragment,
        collapse: bool,
        parent: &Parent,
    ) -> Option<Node> {
        let text = if collapse {
            collapse_spaces(&fragment.text)
        } else {
            fragment.text.trim_end_matches('\n').to_owned()
        };
        if text.trim().is_empty() {
            return None;
        }
        self.report.text_lines += 1;
        let (w, h) = parent
            .to_page
            .unproject_size((fragment.bounds.w, fragment.bounds.h))
            .unwrap_or((fragment.bounds.w, fragment.bounds.h));
        let (cx, cy) = parent
            .to_page
            .invert()
            .unwrap_or(Affine::IDENTITY)
            .apply(fragment.bounds.center());

        let mut decls: Vec<(&str, String)> = TEXT_STYLES
            .iter()
            .filter(|n| !el.style(n).is_empty())
            .map(|&n| (n, el.style(n).to_owned()))
            .collect();
        // The faces Chrome actually used, then the page's own list as a fallback.
        if let Some(uses) = self.options.used_fonts.get(&parent.dom_index) {
            let page_list = el.style("font-family");
            let list = family_list(uses);
            let value = if page_list.is_empty() {
                list
            } else {
                format!("{list}, {page_list}")
            };
            decls.retain(|(k, _)| *k != "font-family");
            decls.push(("font-family", value));
        }
        decls.extend([
            ("white-space", "pre".into()),
            ("line-height", "normal".into()),
            ("margin", "0".into()),
        ]);
        let base_spacing = parse_px(el.style("letter-spacing")).unwrap_or(0.0);
        let mut spacing = base_spacing;

        // A space that ends a line hangs: Chrome's box may not count it. Fit against whichever
        // reading of the line is closer to Chrome's width.
        let trimmed = text.trim_end();
        let measured = self.measure_line(&text, &decls, spacing);
        let measured_trimmed = (trimmed.len() != text.len())
            .then(|| self.measure_line(trimmed, &decls, spacing))
            .flatten();
        let (fit_text, measured) = match (measured, measured_trimmed) {
            (Some(a), Some(b)) if (w - b).abs() < (w - a).abs() => (trimmed, Some(b)),
            (a, _) => (text.as_str(), a),
        };
        if let Some(tw) = measured {
            self.report.line_width_error.push((w - tw).abs());
            let chars = fit_text.chars().count() as f64;
            if self.options.fit_text && chars > 0.0 && (w - tw).abs() > 0.05 {
                spacing += (w - tw) / chars;
                if let Some(fitted) = self.measure_line(fit_text, &decls, spacing) {
                    self.report.line_width_error_fitted.push((w - fitted).abs());
                }
            }
        }
        decls.extend([
            ("letter-spacing", px(spacing)),
            ("position", "absolute".into()),
            ("left", px(cx - w / 2.0 - parent.origin.0)),
            (
                "top",
                px(snap_line_top(cy - h / 2.0, &parent.to_page) - parent.origin.1),
            ),
        ]);
        // Text belongs to its element: each line carries the element's annotation.
        Some(with_id(
            Node::text(text).with_style(self.style(&decls)),
            parent.annotation.clone(),
        ))
    }

    /// Takumi's width for one line of text, laid out on its own.
    fn measure_line(&mut self, text: &str, decls: &[(&str, String)], spacing: f64) -> Option<f64> {
        let mut decls = decls.to_vec();
        decls.push(("letter-spacing", px(spacing)));
        let node = Node::container([Node::text(text.to_owned()).with_style(self.style(&decls))])
            .with_style(self.style(&[("display", "flex".into())]));
        let options = RenderOptions::builder()
            .viewport(Viewport::new((8000, 1000)))
            .node(node)
            .fonts(self.options.fonts)
            .build();
        let measured = takumi::measure(options).ok()?;
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        visit_runs(&measured, &mut |r| {
            lo = lo.min(r.x as f64);
            hi = hi.max((r.x + r.width) as f64);
        });
        (hi > lo).then_some(hi - lo)
    }

    /// Crops the element's box out of Chrome's screenshot, as an image filling the box.
    fn raster(&mut self, el: &Element, tag: &str, to_page: &Affine) -> Option<Node> {
        let label = format!("<{}> #{}", tag.to_lowercase(), el.dom_index);
        let Some(shot) = self.options.screenshot else {
            self.report
                .warnings
                .push(format!("{label}: no screenshot to take its pixels from"));
            return None;
        };
        if !to_page.is_translation() {
            self.report
                .warnings
                .push(format!("{label}: transformed replaced element skipped"));
            return None;
        }
        let b = el.bounds;
        let rect = IntRect::from_xywh(
            b.x.round() as i32,
            b.y.round() as i32,
            b.w.round() as u32,
            b.h.round() as u32,
        )?;
        // As PNG bytes: takumi-svg 0.3.34 writes no `<image>` for `ImageSourceInput::Rgba`
        // (the raster backend does draw it); README, "Upstream candidates".
        let png = shot.clone_rect(rect)?.encode_png().ok()?;
        self.report.raster_fallbacks.push(label);
        let fill = self.style(&[
            ("position", "absolute".into()),
            ("left", "0".into()),
            ("top", "0".into()),
            ("width", "100%".into()),
            ("height", "100%".into()),
        ]);
        Some(
            Node::image(ImageData {
                src: ImageSourceInput::Buffer(png),
                width: None,
                height: None,
            })
            .with_style(fill),
        )
    }

    /// Parses declarations into a Takumi style, dropping (and counting) the ones it rejects.
    fn style(&mut self, decls: &[(&str, String)]) -> Style {
        let join = |d: &[(&str, String)]| {
            d.iter()
                .map(|(k, v)| format!("{k}: {v};"))
                .collect::<String>()
        };
        if let Ok(style) = join(decls).parse() {
            return style;
        }
        let kept: Vec<(&str, String)> = decls
            .iter()
            .filter(|(k, v)| {
                let ok = format!("{k}: {v};").parse::<Style>().is_ok();
                if !ok {
                    *self.report.dropped.entry(format!("{k}: {v}")).or_default() += 1;
                }
                ok
            })
            .cloned()
            .collect();
        join(&kept).parse().unwrap_or_default()
    }
}

fn visit_runs(node: &MeasuredNode, f: &mut impl FnMut(&MeasuredTextRun)) {
    node.runs.iter().for_each(&mut *f);
    node.children.iter().for_each(|c| visit_runs(c, f));
}

fn child_tagged<'e>(el: &'e Element, tag: &str) -> Option<&'e Element> {
    match &el.kind {
        Kind::Element { children, .. } => children.iter().find(|c| c.tag() == tag),
        Kind::Text { .. } => None,
    }
}

fn with_id(node: Node, id: Option<String>) -> Node {
    match id {
        Some(id) => node.with_id(id),
        None => node,
    }
}

/// How many layers a computed `background-image` has: its commas outside parentheses, plus one.
fn layer_count(value: &str) -> usize {
    let mut depth = 0i32;
    1 + value
        .chars()
        .filter(|&c| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            c == ',' && depth == 0
        })
        .count()
}

/// Whether an element has a background to propagate to the canvas: a colour or an image.
fn has_background(el: &Element) -> bool {
    !is_transparent(el.style("background-color"))
        || !matches!(el.style("background-image"), "" | "none")
}

/// Whether an element has the containment that stops the body's background from propagating.
fn contains(el: &Element) -> bool {
    el.style("contain")
        .split_whitespace()
        .any(|v| matches!(v, "size" | "layout" | "paint" | "strict" | "content"))
}

fn is_transparent(color: &str) -> bool {
    color.is_empty() || color == TRANSPARENT || color == "transparent"
}

fn px(v: f64) -> String {
    format!("{}px", (v * 1000.0).round() / 1000.0)
}

fn parse_px(v: &str) -> Option<f64> {
    v.trim().strip_suffix("px")?.trim().parse().ok()
}

/// Chrome's computed `transform-origin` is `Xpx Ypx [Zpx]`; anything else falls back to the centre.
fn parse_origin(v: &str, (w, h): (f64, f64)) -> (f64, f64) {
    let mut parts = v.split_whitespace().map(parse_px);
    match (parts.next().flatten(), parts.next().flatten()) {
        (Some(x), Some(y)) => (x, y),
        _ => (w / 2.0, h / 2.0),
    }
}

/// Collapses each run of white space to one space, as `white-space-collapse: collapse` does
/// within a line. The spaces at the edges stay: they separate this text from its neighbours.
fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(ch);
            space = false;
        }
    }
    out
}

/// Puts a line's top edge on a whole device pixel, as Chrome (and Takumi's own raster backend)
/// snap text; Takumi's SVG backend keeps the fractional position, which measured 3x the
/// visibly different pixels (README, "Text"). Under rotation or scale there is no pixel grid to
/// snap to, so the position is left as it is.
fn snap_line_top(y: f64, to_page: &Affine) -> f64 {
    if to_page.is_translation() {
        (y + to_page.f).round() - to_page.f
    } else {
        y
    }
}

/// Chrome's computed legacy `clip`: `auto` or `rect(T, R, B, L)` with px or `auto` edges, as
/// offsets from the border box's top-left. `None` for `auto`.
fn parse_clip_rect(v: &str, (w, h): (f64, f64)) -> Option<[f64; 4]> {
    let inner = v.trim().strip_prefix("rect(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split([',', ' ']).filter(|p| !p.is_empty()).collect();
    let [t, r, b, l] = parts[..] else { return None };
    let edge = |p: &str, auto: f64| if p == "auto" { Some(auto) } else { parse_px(p) };
    Some([edge(t, 0.0)?, edge(r, w)?, edge(b, h)?, edge(l, 0.0)?])
}
