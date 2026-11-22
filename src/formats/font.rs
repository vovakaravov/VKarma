use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use ttf_parser::name_id;

#[derive(Debug, Clone)]
pub struct FontAxisInfo {
    pub tag: String,
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

#[derive(Debug, Clone)]
pub struct FontGlyphInfo {
    pub ch: char,
    pub codepoint: u32,
    pub glyph_id: u16,
    pub name: Option<String>,
    pub advance: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct FontInfo {
    pub family: String,
    pub subfamily: String,
    pub full_name: String,
    pub version: Option<String>,
    pub designer: Option<String>,
    pub license: Option<String>,
    pub copyright: Option<String>,
    pub num_glyphs: u16,
    pub units_per_em: u16,
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
    pub axes: Vec<FontAxisInfo>,
    pub features: Vec<String>,
    pub glyphs: Vec<FontGlyphInfo>,
    pub raw_bytes: Vec<u8>,
}

fn find_name(face: &ttf_parser::Face<'_>, id: u16) -> Option<String> {
    let mut fallback = None;
    for rec in face.names() {
        if rec.name_id == id
            && let Some(val) = rec.to_string()
        {
            let clean = val.trim().to_string();
            if clean.is_empty() {
                continue;
            }
            if rec.is_unicode() {
                return Some(clean);
            }
            if fallback.is_none() {
                fallback = Some(clean);
            }
        }
    }
    fallback
}

impl FontInfo {
    pub fn load(path: &Path) -> Result<Self> {
        let raw_bytes =
            fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
        let face =
            ttf_parser::Face::parse(&raw_bytes, 0).context("Failed to parse TTF/OTF font")?;

        let fallback_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Font")
            .to_string();

        let family = find_name(&face, name_id::TYPOGRAPHIC_FAMILY)
            .or_else(|| find_name(&face, name_id::FAMILY))
            .or_else(|| find_name(&face, name_id::FULL_NAME))
            .unwrap_or(fallback_stem);

        let subfamily = find_name(&face, name_id::TYPOGRAPHIC_SUBFAMILY)
            .or_else(|| find_name(&face, name_id::SUBFAMILY))
            .unwrap_or_else(|| "Regular".to_string());

        let full_name = find_name(&face, name_id::FULL_NAME)
            .unwrap_or_else(|| format!("{family} {subfamily}"));

        let version = find_name(&face, name_id::VERSION);
        let designer = find_name(&face, name_id::DESIGNER);
        let license = find_name(&face, name_id::LICENSE);
        let copyright = find_name(&face, name_id::COPYRIGHT_NOTICE);

        let axes: Vec<FontAxisInfo> = face
            .variation_axes()
            .into_iter()
            .map(|ax| FontAxisInfo {
                tag: ax.tag.to_string(),
                min: ax.min_value,
                default: ax.def_value,
                max: ax.max_value,
            })
            .collect();

        let mut feat_set = BTreeSet::new();
        if let Some(gsub) = face.tables().gsub {
            for feat in gsub.features {
                feat_set.insert(feat.tag.to_string());
                if feat_set.len() >= 64 {
                    break;
                }
            }
        }
        if feat_set.len() < 64
            && let Some(gpos) = face.tables().gpos
        {
            for feat in gpos.features {
                feat_set.insert(feat.tag.to_string());
                if feat_set.len() >= 64 {
                    break;
                }
            }
        }
        let features: Vec<String> = feat_set.into_iter().collect();

        let mut cp_set = BTreeSet::new();
        if let Some(cmap) = face.tables().cmap {
            for subtable in cmap.subtables {
                if subtable.is_unicode() {
                    subtable.codepoints(|cp| {
                        if cp >= 0x20 && !(0x7f..=0x9f).contains(&cp) {
                            cp_set.insert(cp);
                        }
                    });
                }
            }
        }

        let mut glyphs = Vec::with_capacity(cp_set.len().min(1500));
        for cp in cp_set {
            if glyphs.len() >= 1500 {
                break;
            }
            let Some(ch) = char::from_u32(cp) else {
                continue;
            };
            let Some(gid) = face.glyph_index(ch) else {
                continue;
            };
            let name = face.glyph_name(gid).map(ToString::to_string);
            let advance = face.glyph_hor_advance(gid);
            glyphs.push(FontGlyphInfo {
                ch,
                codepoint: cp,
                glyph_id: gid.0,
                name,
                advance,
            });
        }

        Ok(Self {
            family,
            subfamily,
            full_name,
            version,
            designer,
            license,
            copyright,
            num_glyphs: face.number_of_glyphs(),
            units_per_em: face.units_per_em(),
            ascender: face.ascender(),
            descender: face.descender(),
            line_gap: face.line_gap(),
            axes,
            features,
            glyphs,
            raw_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_system_font(dir: &Path) -> Option<std::path::PathBuf> {
        let entries = fs::read_dir(dir).ok()?;
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_dir() {
                if let Some(found) = find_system_font(&p) {
                    return Some(found);
                }
            } else if p
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
            {
                return Some(p);
            }
        }
        None
    }

    #[test]
    fn loads_system_ttf_metadata_and_glyphs() {
        let Some(font_path) = find_system_font(Path::new("/usr/share/fonts")) else {
            return;
        };
        let info = FontInfo::load(&font_path).unwrap();
        assert!(!info.family.is_empty());
        assert!(info.units_per_em > 0);
        assert!(info.num_glyphs > 0);
        assert!(!info.glyphs.is_empty());
    }
}
