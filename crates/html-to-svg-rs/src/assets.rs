//! Moves the images an SVG embeds as `data:` URLs into a shared folder, named by content, so many
//! SVGs that show the same image (a logo on every page) store it once.
//!
//! The `href` of every `<image>` the renderer wrote with a base64 `data:` URL is decoded, written
//! to `<dir>/<sha256>.<ext>` unless that file exists, and replaced by
//! `<href_prefix><sha256>.<ext>`. Nothing else is touched: the page's own `data-*` attributes
//! can hold `data:` URLs too, and must reach the SVG as they are.
//!
//! The folder may be served from a site, so only raster images go there, and their type is read
//! from their bytes, never taken from the page: a page cannot place an `.svg` or `.html` file
//! (that would run its scripts on the serving site) by naming a URL or a MIME type. A downloaded
//! image keeps its URL's extension when that names the same type (`photo.jpeg` stays `.jpeg`);
//! otherwise the type's own extension is used (`avatar.php` holding a PNG becomes `.png`).
//! Everything else (SVG images, unknown bytes) stays inline.

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

impl AssetsOptions {
    /// Assets in `assets` (created when missing), linked from an SVG that is written to `svg_dir`:
    /// by `href` + file name, or by default by the path from `svg_dir` to `assets`.
    pub fn for_svg_in(svg_dir: &Path, assets: &Path, href: Option<&str>) -> Result<Self> {
        std::fs::create_dir_all(assets).with_context(|| format!("create {}", assets.display()))?;
        let href_prefix = match href {
            Some(href) => href.to_owned(),
            None => {
                let from = std::fs::canonicalize(svg_dir)
                    .with_context(|| format!("find {}", svg_dir.display()))?;
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
        Ok(Self {
            dir: assets.to_owned(),
            href_prefix,
        })
    }
}

/// Content hash (hex SHA-256) of some bytes: the asset's file name without extension.
pub fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Replaces the base64 `data:` URLs of the SVG's `<image>` elements by files in `options.dir`.
/// `known` maps a content hash to the extension of the URL those bytes were downloaded from.
/// Returns the new SVG and the file names it refers to (each once, in order of first use).
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
    while let Some(at) = rest.find("<image") {
        // The tag's attributes, up to its `>`; the value of its `href`, if a `data:` URL.
        let tag_end = rest[at..].find('>').map_or(rest.len(), |e| at + e);
        let href = ["href=\"data:", "xlink:href=\"data:"]
            .iter()
            .filter_map(|a| {
                rest[at..tag_end]
                    .find(a)
                    .map(|i| at + i + a.len() - "data:".len())
            })
            .min();
        let Some(value_start) = href else {
            out.push_str(&rest[..tag_end]);
            rest = &rest[tag_end..];
            continue;
        };
        let Some(len) = rest[value_start..tag_end].find('"') else {
            out.push_str(&rest[..tag_end]);
            rest = &rest[tag_end..];
            continue;
        };
        out.push_str(&rest[..value_start]);
        let value = &rest[value_start..value_start + len];
        match decode(value).and_then(|bytes| Some((raster_type(&bytes)?, bytes))) {
            Some((kind, bytes)) => {
                let digest = hash(&bytes);
                let ext = known
                    .get(&digest)
                    .filter(|ext| kind.extensions.contains(&ext.as_str()))
                    .map_or(kind.extensions[0], String::as_str);
                let name = format!("{digest}.{ext}");
                write_once(&options.dir.join(&name), &bytes)?;
                out.push_str(&escape(&options.href_prefix));
                out.push_str(&name);
                if !files.contains(&name) {
                    files.push(name);
                }
            }
            // Not base64, not a raster image we know by its bytes (an SVG): left inline.
            None => out.push_str(value),
        }
        rest = &rest[value_start + len..];
    }
    out.push_str(rest);
    Ok((out, files))
}

/// `data:<mime>[;params];base64,<payload>` -> the bytes. The MIME type is the page's word and is
/// not used: the bytes say what they are (`raster_type`).
fn decode(value: &str) -> Option<Vec<u8>> {
    let (header, payload) = value.strip_prefix("data:")?.split_once(',')?;
    header.ends_with(";base64").then_some(())?;
    base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .ok()
}

/// A raster image type: the extensions that name it, its own first.
struct RasterType {
    extensions: &'static [&'static str],
}

/// The raster type of some bytes, by their signature; `None` for anything else (SVG, HTML, ...).
fn raster_type(bytes: &[u8]) -> Option<RasterType> {
    let extensions: &'static [&'static str] = match bytes {
        [0x89, b'P', b'N', b'G', ..] => &["png", "apng"],
        [0xFF, 0xD8, 0xFF, ..] => &["jpg", "jpeg", "jpe", "jfif"],
        [b'G', b'I', b'F', b'8', ..] => &["gif"],
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => &["webp"],
        [
            _,
            _,
            _,
            _,
            b'f',
            b't',
            b'y',
            b'p',
            b'a',
            b'v',
            b'i',
            b'f',
            ..,
        ] => &["avif"],
        [b'B', b'M', ..] => &["bmp"],
        [0, 0, 1, 0, ..] => &["ico"],
        _ => return None,
    };
    Some(RasterType { extensions })
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
    // Unique per process and per call: threads of one process may write the same asset at once.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp-{}-{n}", std::process::id()));
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

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake png";
    const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0fake jpeg";

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn options(name: &str) -> AssetsOptions {
        let dir =
            std::env::temp_dir().join(format!("html-to-svg-assets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        AssetsOptions {
            dir,
            href_prefix: "../assets/".to_owned(),
        }
    }

    #[test]
    fn moves_image_data_urls_into_content_addressed_files() {
        let (png, jpeg) = (b64(PNG), b64(JPEG));
        let svg = format!(
            r#"<image href="data:image/png;base64,{png}"/><image href="data:image/png;base64,{png}"/><image x="1" xlink:href="data:image/jpeg;base64,{jpeg}"/><image href="data:image/svg+xml,%3Csvg/%3E"/>"#
        );
        // The JPEG was downloaded from a URL ending in `.jpeg`: same type, so that wins.
        let known = BTreeMap::from([(hash(JPEG), "jpeg".to_owned())]);
        let options = options("move");

        let (out, files) = externalize(&svg, &options, &known).unwrap();

        let (p, j) = (hash(PNG), hash(JPEG));
        assert_eq!(
            out,
            format!(
                r#"<image href="../assets/{p}.png"/><image href="../assets/{p}.png"/><image x="1" xlink:href="../assets/{j}.jpeg"/><image href="data:image/svg+xml,%3Csvg/%3E"/>"#
            )
        );
        assert_eq!(files, [format!("{p}.png"), format!("{j}.jpeg")]);
        assert_eq!(std::fs::read(options.dir.join(&files[0])).unwrap(), PNG);
        std::fs::remove_dir_all(&options.dir).unwrap();
    }

    #[test]
    fn a_page_cannot_place_files_of_its_choosing() {
        let script =
            b64(b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>");
        let png = b64(PNG);
        let svg = format!(
            concat!(
                // The page's own attribute, carried into the SVG: never touched.
                r#"<g data-x="data:text/html;base64,{s}">"#,
                // An image whose bytes are SVG, labelled as PNG: stays inline.
                r#"<image href="data:image/png;base64,{s}"/>"#,
                // A PNG downloaded from `avatar.php`: written as `.png`.
                r#"<image href="data:image/png;base64,{p}"/></g>"#,
            ),
            s = script,
            p = png
        );
        let known = BTreeMap::from([(hash(PNG), "php".to_owned())]);
        let options = options("hostile");

        let (out, files) = externalize(&svg, &options, &known).unwrap();

        assert_eq!(files, [format!("{}.png", hash(PNG))]);
        assert!(out.contains(&format!(r#"data-x="data:text/html;base64,{script}""#)));
        assert!(out.contains(&format!(
            r#"<image href="data:image/png;base64,{script}"/>"#
        )));
        std::fs::remove_dir_all(&options.dir).unwrap();
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
