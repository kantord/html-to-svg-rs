//! The fonts Chrome drew the page with, made available to Takumi under names the lowering uses.
//!
//! Capture records, per element with text, the faces Chrome used (`CSS.getPlatformFontsForNode`:
//! family, PostScript name, web or system font, glyph count) in `fonts.json`, and saves every web
//! font Chrome downloaded under `fonts/`. Here each used face is found by its PostScript name
//! (web fonts by reading their name table, system fonts through `fc-list`) and registered in
//! Takumi under the alias [`alias`], one family per face, and again under its family name. The
//! lowering asks for exactly the faces Chrome used, in Chrome's order; text Chrome reported no
//! face for falls back to the page's `font-family` list, which then finds the same files.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use takumi::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontUse {
    pub family: String,
    pub postscript: String,
    pub custom: bool,
    pub glyphs: f64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FontsFile {
    /// Element snapshot index -> faces used for its text, as Chrome reports them.
    pub used: BTreeMap<usize, Vec<FontUse>>,
    /// File name under `fonts/` -> the URL it was downloaded from.
    pub web: BTreeMap<String, String>,
}

/// The Takumi family name a face is registered under: one family per face.
pub fn alias(postscript: &str) -> String {
    format!("cs-{postscript}")
}

/// The `font-family` value for an element's text: the faces Chrome used, most glyphs first.
pub fn family_list(uses: &[FontUse]) -> String {
    let mut sorted: Vec<&FontUse> = uses.iter().collect();
    sorted.sort_by(|a, b| b.glyphs.total_cmp(&a.glyphs));
    sorted
        .iter()
        .map(|u| format!("\"{}\"", alias(&u.postscript)))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, Serialize)]
pub struct Registered {
    pub faces: usize,
    pub missing: Vec<String>,
}

/// Registers every face `fonts.json` names. Faces that cannot be found or loaded are listed.
pub fn register(dir: &Path, file: &FontsFile, fonts: &mut Fonts) -> Registered {
    // A web font split by `unicode-range` is several files with one PostScript name (latin,
    // latin-ext, ...): all of them are kept, and registered, each covering its own letters.
    let mut by_postscript: HashMap<String, Vec<(Vec<u8>, u32)>> = HashMap::new();
    for name in file.web.keys() {
        let Ok(bytes) = std::fs::read(dir.join("fonts").join(name)) else {
            continue;
        };
        let Some(sfnt) = to_sfnt(bytes) else { continue };
        if let Some(ps) = postscript_name(&sfnt, 0) {
            by_postscript.entry(ps).or_default().push((sfnt, 0));
        }
    }
    let system = system_fonts();

    let mut wanted: Vec<&FontUse> = file.used.values().flatten().collect();
    wanted.sort_by(|a, b| a.postscript.cmp(&b.postscript));
    wanted.dedup_by(|a, b| a.postscript == b.postscript);

    let mut result = Registered {
        faces: 0,
        missing: Vec::new(),
    };
    for u in wanted {
        let files = by_postscript.get(&u.postscript).cloned().or_else(|| {
            let (path, index) = system.get(&u.postscript)?;
            Some(vec![(std::fs::read(path).ok()?, *index)])
        });
        let Some(files) = files else {
            result.missing.push(format!(
                "{} ({})",
                u.postscript,
                if u.custom { "web" } else { "system" }
            ));
            continue;
        };
        for (bytes, index) in files {
            if index != 0 {
                // Takumi takes the first face of a collection; a later one is reported, not guessed.
                result
                    .missing
                    .push(format!("{} (face {index} of a collection)", u.postscript));
                continue;
            }
            // Under its alias (asked for by the lowering), and under its own family name, so text
            // Chrome reported no face for (inside some shadow trees) still resolves the page's
            // `font-family` list to the same files.
            let aliased = FontResource::new(bytes.clone()).override_info(FontOverride {
                family_name: Some(alias(&u.postscript).into()),
                ..FontOverride::default()
            });
            match fonts.register(aliased) {
                Ok(_) => result.faces += 1,
                Err(e) => result.missing.push(format!("{}: {e}", u.postscript)),
            }
            let _ = fonts.register(FontResource::new(bytes).override_info(FontOverride {
                family_name: Some(u.family.clone().into()),
                ..FontOverride::default()
            }));
        }
    }
    result
}

/// WOFF and WOFF2 decoded to plain OpenType; OpenType and TrueType as they are.
fn to_sfnt(bytes: Vec<u8>) -> Option<Vec<u8>> {
    match bytes.get(..4)? {
        b"wOF2" => wuff::decompress_woff2(&bytes).ok(),
        b"wOFF" => wuff::decompress_woff1(&bytes).ok(),
        _ => Some(bytes),
    }
}

fn postscript_name(sfnt: &[u8], index: u32) -> Option<String> {
    let face = ttf_parser::Face::parse(sfnt, index).ok()?;
    face.names()
        .into_iter()
        .filter(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
        .find_map(|n| n.to_string())
}

/// PostScript name -> (file, face index) of every installed font, from fontconfig.
fn system_fonts() -> HashMap<String, (PathBuf, u32)> {
    let Ok(out) = Command::new("fc-list")
        .arg("--format")
        .arg("%{postscriptname}\t%{file}\t%{index}\n")
        .output()
    else {
        return HashMap::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let (ps, file, index) = (parts.next()?, parts.next()?, parts.next()?);
            Some((ps.to_owned(), (PathBuf::from(file), index.parse().ok()?)))
        })
        .collect()
}
