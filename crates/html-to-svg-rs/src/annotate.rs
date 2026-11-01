//! Turns the patched takumi-svg's `data-takumi-node="nX"` group markers into the element's real
//! `data-*` attributes (plus `data-html-to-svg-node`, the DOM index), so a query like
//! `svg.querySelectorAll('[data-testid="card"]')` finds every piece the element painted.
//!
//! Our own attribute has a name a page is not going to use, so the page's `data-*` (its own
//! `data-node` too) reach the SVG as they are, and no attribute is written twice. Names XML does
//! not allow (HTML allows more, e.g. `data-a;b` set from script) are left out: one would make the
//! whole SVG unreadable.

use std::collections::{BTreeMap, HashMap};

pub fn apply(svg: &str, annotations: &HashMap<String, BTreeMap<String, String>>) -> String {
    const MARKER: &str = "data-takumi-node=\"";
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find(MARKER) {
        out.push_str(&rest[..at]);
        let after = &rest[at + MARKER.len()..];
        let end = after.find('"').unwrap_or(after.len());
        let id = &after[..end];
        out.push_str(&format!(
            "data-html-to-svg-node=\"{}\"",
            escape(id.trim_start_matches('n'))
        ));
        for (name, value) in annotations
            .get(id)
            .into_iter()
            .flatten()
            .filter(|(name, _)| xml_name(name) && name.as_str() != "data-html-to-svg-node")
        {
            out.push_str(&format!(" {name}=\"{}\"", escape(value)));
        }
        rest = &after[(end + 1).min(after.len())..];
    }
    out.push_str(rest);
    out
}

/// Whether `name` is an attribute name XML accepts (a conservative subset of `Name`: no colon,
/// which would mean a namespace prefix).
fn xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '\u{B7}'))
}

fn escape(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_marker_with_annotations() {
        let mut map = HashMap::new();
        map.insert(
            "n7".to_owned(),
            BTreeMap::from([
                ("data-testid".to_owned(), "a\"b".to_owned()),
                // The page's own `data-node` is kept; a name XML forbids is left out.
                ("data-node".to_owned(), "page's".to_owned()),
                ("data-a;b".to_owned(), "x".to_owned()),
            ]),
        );
        let svg = r#"<g data-takumi-node="n7"><rect/></g><g data-takumi-node="n9"/>"#;
        assert_eq!(
            apply(svg, &map),
            r#"<g data-html-to-svg-node="7" data-node="page's" data-testid="a&quot;b"><rect/></g><g data-html-to-svg-node="9"/>"#
        );
    }
}
