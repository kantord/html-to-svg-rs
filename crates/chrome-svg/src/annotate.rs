//! Turns the patched takumi-svg's `data-takumi-node="nX"` group markers into the element's real
//! `data-*` attributes (plus `data-node`, the DOM index), so a query like
//! `svg.querySelectorAll('[data-testid="card"]')` finds every piece the element painted.

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
            "data-node=\"{}\"",
            escape(id.trim_start_matches('n'))
        ));
        for (name, value) in annotations.get(id).into_iter().flatten() {
            out.push_str(&format!(" {name}=\"{}\"", escape(value)));
        }
        rest = &after[(end + 1).min(after.len())..];
    }
    out.push_str(rest);
    out
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
            BTreeMap::from([("data-testid".to_owned(), "a\"b".to_owned())]),
        );
        let svg = r#"<g data-takumi-node="n7"><rect/></g><g data-takumi-node="n9"/>"#;
        assert_eq!(
            apply(svg, &map),
            r#"<g data-node="7" data-testid="a&quot;b"><rect/></g><g data-node="9"/>"#
        );
    }
}
