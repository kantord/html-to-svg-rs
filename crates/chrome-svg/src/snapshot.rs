//! Chrome's `DOMSnapshot.captureSnapshot` result, decoded into a tree of laid-out boxes.
//!
//! The raw format is column-oriented (one array per property, strings interned in a shared table,
//! `-1` for "absent"). [`Page::decode`] turns the first document into a tree that only holds what
//! the lowering needs: boxes with Chrome's geometry, the computed styles we asked for, `data-*`
//! annotations, paint order, and one fragment per text line. Frames (`documents[1..]`) are ignored.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The computed styles requested from Chrome. The lowering reads them by name from [`Element::style`].
pub const STYLES: &[&str] = &[
    "display",
    "visibility",
    "opacity",
    "background-color",
    "background-image",
    "background-size",
    "background-position",
    "background-repeat",
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
    "box-shadow",
    "overflow-x",
    "overflow-y",
    "transform",
    "transform-origin",
    "filter",
    "mix-blend-mode",
    "clip-path",
    "clip",
    "position",
    "mask-image",
    "mask-size",
    "mask-position",
    "mask-repeat",
    "mask-mode",
    "-webkit-mask-image",
    "-webkit-mask-size",
    "-webkit-mask-position",
    "-webkit-mask-repeat",
    "outline-width",
    "outline-style",
    "outline-color",
    "outline-offset",
    "color",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "letter-spacing",
    "font-variant-numeric",
    "font-feature-settings",
    "font-variation-settings",
    "font-kerning",
    "word-spacing",
    "white-space-collapse",
    "text-decoration-line",
    "text-decoration-style",
    "text-decoration-color",
    "text-decoration-thickness",
    "text-shadow",
];

// ---- raw CDP shape (only the fields we read) ----

#[derive(Deserialize)]
struct Raw {
    documents: Vec<RawDocument>,
    strings: Vec<String>,
    /// The style names the capture asked for, in order: Chrome's style arrays are positional.
    /// Written by `capture` next to Chrome's own fields; not part of the CDP result.
    #[serde(rename = "computedStyles")]
    computed_styles: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawDocument {
    nodes: RawNodes,
    layout: RawLayout,
    text_boxes: RawTextBoxes,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawNodes {
    parent_index: Vec<i64>,
    node_type: Vec<i64>,
    node_name: Vec<i64>,
    attributes: Vec<Vec<i64>>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawLayout {
    node_index: Vec<usize>,
    styles: Vec<Vec<i64>>,
    bounds: Vec<Vec<f64>>,
    text: Vec<i64>,
    paint_orders: Vec<i64>,
    offset_rects: Vec<Vec<f64>>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RawTextBoxes {
    layout_index: Vec<usize>,
    bounds: Vec<Vec<f64>>,
    start: Vec<usize>,
    length: Vec<usize>,
}

// ---- decoded tree ----

/// An axis-aligned rectangle in CSS pixels, page space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn from_slice(v: &[f64]) -> Option<Self> {
        match v {
            [x, y, w, h, ..] => Some(Self {
                x: *x,
                y: *y,
                w: *w,
                h: *h,
            }),
            _ => None,
        }
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// One line of a text node as Chrome laid it out.
#[derive(Debug, Clone)]
pub struct Fragment {
    pub text: String,
    /// Bounding box in page space (after transforms).
    pub bounds: Rect,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Element {
        /// Upper-case tag name, as the DOM reports it.
        tag: String,
        children: Vec<Element>,
    },
    Text {
        fragments: Vec<Fragment>,
    },
}

/// A DOM node that has a layout box.
#[derive(Debug, Clone)]
pub struct Element {
    /// Index of the DOM node in the snapshot; stable name for annotations and logs.
    pub dom_index: usize,
    pub kind: Kind,
    /// Bounding box in page space, after transforms.
    pub bounds: Rect,
    /// `offsetWidth`/`offsetHeight`: the box size before transforms, when Chrome reports one.
    pub offset_size: Option<(f64, f64)>,
    pub style: HashMap<String, String>,
    /// `data-*` attributes, in source order of name.
    pub annotations: BTreeMap<String, String>,
    /// Chrome's global paint order; siblings are emitted in this order.
    pub paint_order: i64,
}

impl Element {
    pub fn style(&self, name: &str) -> &str {
        self.style.get(name).map(String::as_str).unwrap_or("")
    }

    pub fn tag(&self) -> &str {
        match &self.kind {
            Kind::Element { tag, .. } => tag,
            Kind::Text { .. } => "#text",
        }
    }
}

/// The decoded first document of a snapshot.
#[derive(Debug)]
pub struct Page {
    pub root: Element,
}

const TEXT_NODE: i64 = 3;

impl Page {
    pub fn decode(json: &str) -> Result<Self> {
        let raw: Raw = serde_json::from_str(json).context("parse DOMSnapshot JSON")?;
        let doc = raw.documents.first().context("snapshot has no documents")?;
        let names = raw.computed_styles.as_deref().context(
            "snapshot.json has no `computedStyles` list (captured by an older chrome-svg); capture it again",
        )?;
        let s = |i: i64| -> &str {
            usize::try_from(i)
                .ok()
                .and_then(|i| raw.strings.get(i))
                .map_or("", String::as_str)
        };
        let n = &doc.nodes;
        let l = &doc.layout;

        // Text boxes grouped by the layout node they belong to.
        let mut boxes: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, &li) in doc.text_boxes.layout_index.iter().enumerate() {
            boxes.entry(li).or_default().push(i);
        }

        // The decoded box of every DOM node that has a layout object.
        let mut elements: HashMap<usize, Element> = HashMap::new();
        // Text a node owns through an extra layout object (generated content of `::before`,
        // list markers): attached as text children of that node once the tree is built.
        let mut generated: Vec<(usize, Element)> = Vec::new();
        for (li, &dom) in l.node_index.iter().enumerate() {
            let Some(bounds) = l.bounds.get(li).and_then(|b| Rect::from_slice(b)) else {
                continue;
            };
            let style = l
                .styles
                .get(li)
                .map(|values| {
                    names
                        .iter()
                        .zip(values)
                        .map(|(name, &v)| (name.clone(), s(v).to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            let duplicate = elements.contains_key(&dom);
            if duplicate && !boxes.contains_key(&li) {
                // A further layout object of the same node that draws no text: nothing to add.
                continue;
            }
            let kind = if duplicate || n.node_type.get(dom) == Some(&TEXT_NODE) {
                let text: Vec<u16> = s(l.text.get(li).copied().unwrap_or(-1))
                    .encode_utf16()
                    .collect();
                let fragments = boxes
                    .get(&li)
                    .into_iter()
                    .flatten()
                    .filter_map(|&b| {
                        let tb = &doc.text_boxes;
                        let start = tb.start[b];
                        let end = (start + tb.length[b]).min(text.len());
                        Some(Fragment {
                            // Start and length count UTF-16 code units.
                            text: String::from_utf16_lossy(text.get(start..end)?),
                            bounds: Rect::from_slice(&tb.bounds[b])?,
                        })
                    })
                    .collect();
                Kind::Text { fragments }
            } else {
                Kind::Element {
                    tag: s(n.node_name[dom]).to_owned(),
                    children: Vec::new(),
                }
            };
            let annotations = n
                .attributes
                .get(dom)
                .map(|pairs| {
                    pairs
                        .chunks(2)
                        .filter_map(|kv| match kv {
                            [k, v] if s(*k).starts_with("data-") => {
                                Some((s(*k).to_owned(), s(*v).to_owned()))
                            }
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let element = Element {
                dom_index: dom,
                kind,
                bounds,
                offset_size: l
                    .offset_rects
                    .get(li)
                    .and_then(|r| Rect::from_slice(r))
                    .map(|r| (r.w, r.h)),
                style,
                annotations,
                paint_order: l.paint_orders.get(li).copied().unwrap_or(0),
            };
            if duplicate {
                generated.push((dom, element));
            } else {
                elements.insert(dom, element);
            }
        }
        for (dom, text) in generated {
            if let Some(Element {
                kind: Kind::Element { children, .. },
                ..
            }) = elements.get_mut(&dom)
            {
                children.push(text);
            }
        }

        // Attach every box to the nearest ancestor that has one (skips `display: contents` and
        // other box-less ancestors). Deepest first, so children are complete before they move.
        let parent_of = |mut dom: usize| -> Option<usize> {
            loop {
                let p = usize::try_from(*n.parent_index.get(dom)?).ok()?;
                if elements.contains_key(&p) {
                    return Some(p);
                }
                dom = p;
            }
        };
        let depth = |mut dom: usize| {
            let mut d = 0;
            while let Some(p) = n
                .parent_index
                .get(dom)
                .and_then(|&p| usize::try_from(p).ok())
            {
                d += 1;
                dom = p;
            }
            d
        };
        let mut order: Vec<usize> = elements.keys().copied().collect();
        order.sort_by_key(|&d| std::cmp::Reverse((depth(d), d)));
        let parents: HashMap<usize, Option<usize>> =
            order.iter().map(|&d| (d, parent_of(d))).collect();
        let mut root = None;
        for dom in order {
            let el = elements.remove(&dom).expect("each node is visited once");
            match parents[&dom] {
                Some(p) => match &mut elements.get_mut(&p).expect("parent still present").kind {
                    Kind::Element { children, .. } => children.push(el),
                    Kind::Text { .. } => {}
                },
                None if root.is_none() => root = Some(el),
                None => bail!("snapshot has more than one root box (node {dom})"),
            }
        }
        let mut root = root.context("snapshot has no layout boxes")?;
        sort_by_paint_order(&mut root);
        Ok(Self { root })
    }
}

/// Orders siblings as Chrome painted them. This is exact when no subtree's paint interleaves with
/// a sibling's (the common case); a positioned grandchild painting above a later uncle cannot be
/// expressed as tree order and is a known limitation (README).
fn sort_by_paint_order(el: &mut Element) {
    if let Kind::Element { children, .. } = &mut el.kind {
        children.sort_by_key(|c| (c.paint_order, c.dom_index));
        children.iter_mut().for_each(sort_by_paint_order);
    }
}
