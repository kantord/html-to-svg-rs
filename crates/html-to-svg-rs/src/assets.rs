//! Moves the images an SVG embeds as `data:` URLs into a shared folder, named by content, so many
//! SVGs that show the same image (a logo on every page) store it once.
//!
//! Every quoted `"data:<mime>;base64,..."` attribute value of the SVG (`<image href>`, pattern
//! images) is decoded, written to `<dir>/<sha256>.<ext>` unless that file exists, and replaced by
//! `<href_prefix><sha256>.<ext>`. The extension is the original URL's when the bytes are an image
//! Chrome downloaded, else the MIME type's (crops of Chrome's screenshot are `.png`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine;
use sha2::{Digest, Sha256};

/// Where externalized assets go and how the SVG refers to them.
#[derive(Debug, Clone)]
pub struct AssetsOptions {
    /// The shared folder; created when missing.
    pub dir: PathBuf,
    /// Put before each file name in the SVG: a path relative to where the SVG is served from
    /// (e.g. `../assets/`) or a URL (e.g. `https://cdn.example/assets/`).
    pub href_prefix: String,
}

/// Content hash (hex SHA-256) of some bytes: the asset's file name without extension.
pub fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Replaces the SVG's base64 `data:` URLs by files in `options.dir`. `known` maps a content hash
/// to the extension of the URL those bytes were downloaded from. Returns the new SVG and the
/// file names it refers to (each once, in order of first use).
pub fn externalize(
    svg: &str,
    options: &AssetsOptions,
    known: &BTreeMap<String, String>,
) -> Result<(String, Vec<String>)> {
    std::fs::create_dir_all(&options.dir)
        .with_context(|| format!("create {}", options.dir.display()))?;
    let mut out = String::with_capacity(svg.len() / 4);
    let mut files = Vec::new();
    let mut rest = svg;
    while let Some(at) = rest.find("\"data:") {
        out.push_str(&rest[..=at]);
        let value_start = at + 1;
        let Some(len) = rest[value_start..].find('"') else {
            break;
        };
        let value = &rest[value_start..value_start + len];
        match decode(value) {
            Some((mime, bytes)) => {
                let digest = hash(&bytes);
                let ext = known
                    .get(&digest)
                    .cloned()
                    .unwrap_or_else(|| extension_for_mime(mime).to_owned());
                let name = format!("{digest}.{ext}");
                write_once(&options.dir.join(&name), &bytes)?;
                out.push_str(&escape(&options.href_prefix));
                out.push_str(&name);
                if !files.contains(&name) {
                    files.push(name);
                }
            }
            // Not base64 (e.g. a URL-encoded SVG): small, left inline.
            None => out.push_str(value),
        }
        rest = &rest[value_start + len..];
    }
    out.push_str(rest);
    Ok((out, files))
}

/// `data:<mime>[;params];base64,<payload>` -> (mime, bytes).
fn decode(value: &str) -> Option<(&str, Vec<u8>)> {
    let (header, payload) = value.strip_prefix("data:")?.split_once(',')?;
    let mime = header.strip_suffix(";base64")?;
    let mime = mime.split(';').next().unwrap_or(mime);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .ok()?;
    Some((mime, bytes))
}

fn extension_for_mime(mime: &str) -> &str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        _ => "bin",
    }
}

/// The extension of a URL's last path segment, when it looks like one (`logo.PNG` -> `png`).
pub fn extension_of_url(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let last = path.rsplit('/').next()?;
    let (_, ext) = last.rsplit_once('.')?;
    (!ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| ext.to_ascii_lowercase())
}

/// Writes a content-addressed file unless it is already there. Several conversions may write
/// the same asset at once (parallel test workers): each writes a private temporary file and
/// renames it into place, which is atomic, and the contents are equal whoever wins.
fn write_once(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("move into {}", path.display()))?;
    Ok(())
}

fn escape(v: &str) -> String {
    v.replace('&', "&amp;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_data_urls_into_content_addressed_files() {
        let dir = std::env::temp_dir().join(format!("html-to-svg-assets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let png = base64::engine::general_purpose::STANDARD.encode(b"fake png");
        let jpeg = base64::engine::general_purpose::STANDARD.encode(b"fake jpeg");
        let svg = format!(
            r#"<image href="data:image/png;base64,{png}"/><image href="data:image/png;base64,{png}"/><image href="data:image/jpeg;base64,{jpeg}"/><image href="data:image/svg+xml,%3Csvg/%3E"/>"#
        );
        // The JPEG was downloaded from a URL ending in `.jpeg`: that wins over the MIME's `jpg`.
        let known = BTreeMap::from([(hash(b"fake jpeg"), "jpeg".to_owned())]);
        let options = AssetsOptions {
            dir: dir.clone(),
            href_prefix: "../assets/".to_owned(),
        };

        let (out, files) = externalize(&svg, &options, &known).unwrap();

        let (p, j) = (hash(b"fake png"), hash(b"fake jpeg"));
        assert_eq!(
            out,
            format!(
                r#"<image href="../assets/{p}.png"/><image href="../assets/{p}.png"/><image href="../assets/{j}.jpeg"/><image href="data:image/svg+xml,%3Csvg/%3E"/>"#
            )
        );
        assert_eq!(files, [format!("{p}.png"), format!("{j}.jpeg")]);
        assert_eq!(std::fs::read(dir.join(&files[0])).unwrap(), b"fake png");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_url_extensions() {
        assert_eq!(
            extension_of_url("https://x/a/logo.PNG?v=2").as_deref(),
            Some("png")
        );
        assert_eq!(
            extension_of_url("https://x/a/photo.jpeg#top").as_deref(),
            Some("jpeg")
        );
        assert_eq!(extension_of_url("https://x/a/image"), None);
        assert_eq!(extension_of_url("https://x.example/"), None);
    }
}
