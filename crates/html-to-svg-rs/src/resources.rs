//! Images Chrome downloaded (`images/`, `images.json`), so CSS `url(...)` in backgrounds and
//! masks can be drawn without fetching anything again: each known URL is inlined as a `data:` URL,
//! which Takumi decodes itself.

use std::collections::BTreeMap;
use std::path::Path;

use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// File name under `images/`.
    pub file: String,
    pub mime: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Resources {
    /// Absolute URL -> the saved response.
    pub images: BTreeMap<String, Resource>,
}

/// URL -> `data:` URL of every saved image of a capture directory (empty when there is none).
pub fn data_urls(dir: &Path) -> BTreeMap<String, String> {
    let Ok(bytes) = std::fs::read(dir.join("images.json")) else {
        return BTreeMap::new();
    };
    let Ok(resources) = serde_json::from_slice::<Resources>(&bytes) else {
        return BTreeMap::new();
    };
    resources
        .images
        .into_iter()
        .filter_map(|(url, r)| {
            let body = std::fs::read(dir.join("images").join(&r.file)).ok()?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(body);
            Some((url, format!("data:{};base64,{b64}", r.mime)))
        })
        .collect()
}

/// Content hash -> the extension of the URL each saved image was downloaded from (see
/// [`crate::assets`]): an externalized asset keeps its original extension.
pub fn extensions_by_hash(dir: &Path) -> BTreeMap<String, String> {
    let Ok(bytes) = std::fs::read(dir.join("images.json")) else {
        return BTreeMap::new();
    };
    let Ok(resources) = serde_json::from_slice::<Resources>(&bytes) else {
        return BTreeMap::new();
    };
    resources
        .images
        .into_iter()
        .filter_map(|(url, r)| {
            let ext = crate::assets::extension_of_url(&url)?;
            let body = std::fs::read(dir.join("images").join(&r.file)).ok()?;
            Some((crate::assets::hash(&body), ext))
        })
        .collect()
}

/// Replaces every `url(...)` in a CSS value by its `data:` URL. `None` when one cannot be
/// resolved (not captured): the caller drops the declaration and reports it.
pub fn inline_urls(value: &str, data: &BTreeMap<String, String>) -> Option<String> {
    let value = resolve_image_sets(value, data);
    let mut out = String::with_capacity(value.len());
    let mut rest = value.as_str();
    while let Some(at) = rest.find("url(") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 4..];
        let (url, used) = read_url(after)?;
        let resolved = if url.starts_with("data:") {
            url
        } else {
            data.get(&url)?.clone()
        };
        out.push_str(&quoted_url(&resolved));
        rest = &after[used..];
    }
    out.push_str(rest);
    Some(out)
}

/// Replaces each `image-set(...)` (and `-webkit-image-set`) by the `url(...)` of the candidate
/// Chrome downloaded, which is the one it drew at this device scale; else the first candidate.
fn resolve_image_sets(value: &str, data: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    loop {
        let found = ["-webkit-image-set(", "image-set("]
            .iter()
            .filter_map(|p| rest.find(p).map(|i| (i, p.len())))
            .min();
        let Some((at, len)) = found else { break };
        let body = at + len;
        // The parenthesis that closes the image-set, not counting those in quoted strings.
        let Some(end) = closing_paren(&rest[body..]).map(|e| body + e) else {
            break;
        };
        let inner = &rest[body..end];
        let mut candidates: Vec<String> = Vec::new();
        let mut scan = inner;
        while let Some(i) = scan.find("url(") {
            let Some((url, used)) = read_url(&scan[i + 4..]) else {
                break;
            };
            candidates.push(url);
            scan = &scan[i + 4 + used..];
        }
        let pick = candidates
            .iter()
            .find(|u| u.starts_with("data:") || data.contains_key(*u))
            .or(candidates.first());
        out.push_str(&rest[..at]);
        if let Some(url) = pick {
            out.push_str(&quoted_url(url));
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The URL of a `url(` token, from just after `url(`: Chrome writes it quoted, and a quoted URL
/// may hold parentheses (`data:...rgb(1,2,3)...`) and escaped quotes. Returns the URL and how many
/// bytes, up to and including the closing `)`, it took.
fn read_url(after: &str) -> Option<(String, usize)> {
    let start = after.len() - after.trim_start().len();
    let mut chars = after[start..].char_indices();
    let (_, first) = chars.next()?;
    if first != '"' && first != '\'' {
        let end = after.find(')')?;
        return Some((after[..end].trim().to_owned(), end + 1));
    }
    let mut url = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => url.push(chars.next()?.1),
            c if c == first => {
                let tail = &after[start + i + 1..];
                let close = tail.find(')')?;
                tail[..close].trim().is_empty().then_some(())?;
                return Some((url, start + i + 1 + close + 1));
            }
            c => url.push(c),
        }
    }
    None
}

/// `url("...")`, quoting what has to be.
fn quoted_url(url: &str) -> String {
    format!(
        "url(\"{}\")",
        url.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

/// Where the `)` closing an already open parenthesis is, skipping quoted strings.
fn closing_paren(s: &str) -> Option<usize> {
    let mut depth = 1;
    let mut quote = None;
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match (quote, c) {
            (Some(_), '\\') => {
                chars.next();
            }
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_url_ends_at_its_quote_not_at_a_parenthesis() {
        let svg = r#"url("data:image/svg+xml,%3Crect fill='rgb(220,38,38)'/%3E") 0 0"#;
        assert_eq!(inline_urls(svg, &BTreeMap::new()).as_deref(), Some(svg));
        let escaped = r#"url("data:text/plain,a\"b(c)")"#;
        assert_eq!(
            inline_urls(escaped, &BTreeMap::new()).as_deref(),
            Some(r#"url("data:text/plain,a\"b(c)")"#)
        );
        assert_eq!(
            inline_urls(
                r#"image-set(url("data:x,(1)") 1x, url("b.png") 2x)"#,
                &BTreeMap::new()
            )
            .as_deref(),
            Some(r#"url("data:x,(1)")"#)
        );
    }

    #[test]
    fn inlines_known_urls_and_keeps_data_urls() {
        let data = BTreeMap::from([(
            "https://x/a.svg".to_owned(),
            "data:image/svg+xml;base64,AA".to_owned(),
        )]);
        assert_eq!(
            inline_urls(
                r#"url("https://x/a.svg"), url(data:image/png;base64,BB)"#,
                &data
            )
            .as_deref(),
            Some(r#"url("data:image/svg+xml;base64,AA"), url("data:image/png;base64,BB")"#)
        );
        assert_eq!(inline_urls(r#"url("https://x/missing.png")"#, &data), None);
        assert_eq!(
            inline_urls("linear-gradient(red, blue)", &data).as_deref(),
            Some("linear-gradient(red, blue)")
        );
        assert_eq!(
            inline_urls(
                r#"image-set(url("https://x/2x.png") 2x, url("https://x/a.svg") 1x)"#,
                &data
            )
            .as_deref(),
            Some(r#"url("data:image/svg+xml;base64,AA")"#)
        );
    }
}
