use crate::formats::office::image_format_from_name;
use crate::vault::OutlineHeading;
use anyhow::{Context, Result};
use gpui_kit::Image;
use percent_encoding::percent_decode_str;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use zip::ZipArchive;

#[derive(Clone)]
pub enum EpubBlock {
    Heading { level: u8, text: String },
    Paragraph { text: String, is_quote: bool },
    ListItem(String),
    Image { alt: String, image: Arc<Image> },
    Rule,
}

#[derive(Clone)]
pub struct EpubChapter {
    pub title: String,
    pub blocks: Vec<EpubBlock>,
}

#[derive(Clone)]
pub struct EpubTocItem {
    pub title: String,
    pub chapter_idx: usize,
    pub depth: u8,
}

#[derive(Clone)]
pub struct EpubBook {
    pub title: String,
    pub author: Option<String>,
    pub chapters: Vec<EpubChapter>,
    pub toc: Vec<EpubTocItem>,
    pub total_words: usize,
}

fn read_zip_text<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> Option<String> {
    let mut entry = zip.by_name(name).ok()?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).ok()?;
    Some(buf)
}

fn read_zip_image<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    zip_path: &str,
) -> Option<Arc<Image>> {
    let fmt = image_format_from_name(zip_path)?;
    let mut entry = zip.by_name(zip_path).ok()?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(Arc::new(Image::from_bytes(fmt, bytes)))
}

fn resolve_zip_relative(base_dir: &str, href: &str) -> String {
    let clean_href = href.split('#').next().unwrap_or(href);
    let decoded = percent_decode_str(clean_href).decode_utf8_lossy();
    let mut parts: Vec<String> = if decoded.starts_with('/') {
        Vec::new()
    } else {
        base_dir
            .split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .map(ToString::to_string)
            .collect()
    };
    for seg in decoded.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s.to_string()),
        }
    }
    parts.join("/")
}

fn replace_html_entities_numeric(xml: &str) -> String {
    // Strip DOCTYPE declarations (which often trigger DTD errors in roxmltree)
    let mut cleaned = String::with_capacity(xml.len());
    let mut rest = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    while let Some(pos) = rest.find("<!DOCTYPE") {
        cleaned.push_str(&rest[..pos]);
        if let Some(end) = rest[pos..].find('>') {
            rest = &rest[pos + end + 1..];
        } else {
            break;
        }
    }
    cleaned.push_str(rest);

    const ENTITIES: &[(&str, &str)] = &[
        ("&nbsp;", "&#160;"),
        ("&mdash;", "&#8212;"),
        ("&ndash;", "&#8211;"),
        ("&hellip;", "&#8230;"),
        ("&lsquo;", "&#8216;"),
        ("&rsquo;", "&#8217;"),
        ("&ldquo;", "&#8220;"),
        ("&rdquo;", "&#8221;"),
        ("&laquo;", "&#171;"),
        ("&raquo;", "&#187;"),
        ("&copy;", "&#169;"),
        ("&reg;", "&#174;"),
        ("&trade;", "&#8482;"),
        ("&bull;", "&#8226;"),
        ("&middot;", "&#183;"),
        ("&times;", "&#215;"),
        ("&divide;", "&#247;"),
        ("&deg;", "&#176;"),
        ("&eacute;", "&#233;"),
        ("&egrave;", "&#232;"),
        ("&agrave;", "&#224;"),
        ("&uuml;", "&#252;"),
        ("&ouml;", "&#246;"),
        ("&auml;", "&#228;"),
        ("&ccedil;", "&#231;"),
        ("&szlig;", "&#223;"),
        ("&sect;", "&#167;"),
        ("&para;", "&#182;"),
        ("&dagger;", "&#8224;"),
        ("&Dagger;", "&#8225;"),
        ("&prime;", "&#8242;"),
        ("&Prime;", "&#8243;"),
    ];
    for &(from, to) in ENTITIES {
        if cleaned.contains(from) {
            cleaned = cleaned.replace(from, to);
        }
    }
    cleaned
}

fn collect_inline_text(node: roxmltree::Node<'_, '_>, out: &mut String) {
    if node.is_text() {
        if let Some(t) = node.text() {
            out.push_str(t);
        }
        return;
    }
    if node.tag_name().name().eq_ignore_ascii_case("br") {
        out.push('\n');
        return;
    }
    for ch in node.children() {
        collect_inline_text(ch, out);
    }
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_html_tags_fallback(raw: &str) -> Vec<EpubBlock> {
    let mut blocks = Vec::new();
    let mut cur = String::new();
    let mut in_tag = false;
    let mut skip_style_script = false;
    let lower = raw.to_ascii_lowercase();
    let chars: Vec<char> = raw.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        if !in_tag && chars[i] == '<' {
            let tail: String = lower_chars[i..].iter().take(10).collect();
            if tail.starts_with("<style") || tail.starts_with("<script") {
                skip_style_script = true;
            } else if tail.starts_with("</style") || tail.starts_with("</script") {
                skip_style_script = false;
            } else if tail.starts_with("</p") || tail.starts_with("</div") || tail.starts_with("</h")
            {
                let t = normalize_ws(&cur);
                if !t.is_empty() {
                    blocks.push(EpubBlock::Paragraph {
                        text: t,
                        is_quote: false,
                    });
                }
                cur.clear();
            }
            in_tag = true;
        } else if in_tag && chars[i] == '>' {
            in_tag = false;
        } else if !in_tag && !skip_style_script {
            cur.push(chars[i]);
        }
        i += 1;
    }
    let tail = normalize_ws(&cur);
    if !tail.is_empty() {
        blocks.push(EpubBlock::Paragraph {
            text: tail,
            is_quote: false,
        });
    }
    blocks
}

fn walk_xhtml_blocks<R: Read + std::io::Seek>(
    node: roxmltree::Node<'_, '_>,
    chap_dir: &str,
    in_quote: bool,
    zip: &mut ZipArchive<R>,
    blocks: &mut Vec<EpubBlock>,
) {
    if !node.is_element() {
        return;
    }
    let tag = node.tag_name().name().to_ascii_lowercase();
    match tag.as_str() {
        "style" | "script" | "head" => {}
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let lvl = tag[1..].parse::<u8>().unwrap_or(1).clamp(1, 6);
            let mut txt = String::new();
            collect_inline_text(node, &mut txt);
            let clean = normalize_ws(&txt);
            if !clean.is_empty() {
                blocks.push(EpubBlock::Heading {
                    level: lvl,
                    text: clean,
                });
            }
        }
        "hr" => blocks.push(EpubBlock::Rule),
        "img" | "image" => {
            let src = node
                .attributes()
                .find(|a| matches!(a.name(), "src" | "href"))
                .map(|a| a.value());
            if let Some(src_href) = src {
                let zpath = resolve_zip_relative(chap_dir, src_href);
                if let Some(image) = read_zip_image(zip, &zpath) {
                    let alt = node.attribute("alt").unwrap_or("").trim().to_string();
                    blocks.push(EpubBlock::Image { alt, image });
                }
            }
        }
        "li" => {
            let mut txt = String::new();
            collect_inline_text(node, &mut txt);
            let clean = normalize_ws(&txt);
            if !clean.is_empty() {
                blocks.push(EpubBlock::ListItem(clean));
            }
        }
        "blockquote" => {
            let has_inner_blocks = node.children().any(|c| {
                c.is_element()
                    && matches!(
                        c.tag_name().name().to_ascii_lowercase().as_str(),
                        "p" | "div" | "li"
                    )
            });
            if has_inner_blocks {
                for ch in node.children() {
                    walk_xhtml_blocks(ch, chap_dir, true, zip, blocks);
                }
            } else {
                let mut txt = String::new();
                collect_inline_text(node, &mut txt);
                let clean = normalize_ws(&txt);
                if !clean.is_empty() {
                    blocks.push(EpubBlock::Paragraph {
                        text: clean,
                        is_quote: true,
                    });
                }
            }
        }
        "p" => {
            // Check for embedded images inside `<p>` first
            for img_el in node.descendants().filter(|n| {
                n.is_element() && matches!(n.tag_name().name().to_ascii_lowercase().as_str(), "img" | "image")
            }) {
                if let Some(src_href) = img_el
                    .attributes()
                    .find(|a| matches!(a.name(), "src" | "href"))
                    .map(|a| a.value())
                {
                    let zpath = resolve_zip_relative(chap_dir, src_href);
                    if let Some(image) = read_zip_image(zip, &zpath) {
                        let alt = img_el.attribute("alt").unwrap_or("").trim().to_string();
                        blocks.push(EpubBlock::Image { alt, image });
                    }
                }
            }
            let mut txt = String::new();
            collect_inline_text(node, &mut txt);
            let clean = normalize_ws(&txt);
            if !clean.is_empty() {
                blocks.push(EpubBlock::Paragraph {
                    text: clean,
                    is_quote: in_quote,
                });
            }
        }
        _ => {
            for ch in node.children() {
                walk_xhtml_blocks(ch, chap_dir, in_quote, zip, blocks);
            }
        }
    }
}

fn collect_nav_toc(
    ol_node: roxmltree::Node<'_, '_>,
    nav_dir: &str,
    depth: u8,
    chapter_by_path: &HashMap<String, usize>,
    out: &mut Vec<EpubTocItem>,
) {
    for li in ol_node
        .children()
        .filter(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("li"))
    {
        if let Some(a) = li
            .children()
            .find(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("a"))
            && let Some(href) = a.attribute("href")
        {
            let target = resolve_zip_relative(nav_dir, href);
            if let Some(&ch_ix) = chapter_by_path.get(&target) {
                let mut lbl = String::new();
                collect_inline_text(a, &mut lbl);
                let title = normalize_ws(&lbl);
                if !title.is_empty() {
                    out.push(EpubTocItem {
                        title,
                        chapter_idx: ch_ix,
                        depth,
                    });
                }
            }
        }
        for sub_ol in li
            .children()
            .filter(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("ol"))
        {
            collect_nav_toc(sub_ol, nav_dir, (depth + 1).min(6), chapter_by_path, out);
        }
    }
}

fn collect_ncx_toc(
    parent: roxmltree::Node<'_, '_>,
    ncx_dir: &str,
    depth: u8,
    chapter_by_path: &HashMap<String, usize>,
    out: &mut Vec<EpubTocItem>,
) {
    for np in parent
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "navPoint")
    {
        let label = np
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "navLabel")
            .map(|lbl| {
                let mut s = String::new();
                collect_inline_text(lbl, &mut s);
                normalize_ws(&s)
            })
            .unwrap_or_default();
        let src = np
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "content")
            .and_then(|c| c.attribute("src"));
        if let Some(href) = src {
            let target = resolve_zip_relative(ncx_dir, href);
            if let Some(&ch_ix) = chapter_by_path.get(&target)
                && !label.is_empty()
            {
                out.push(EpubTocItem {
                    title: label,
                    chapter_idx: ch_ix,
                    depth,
                });
            }
        }
        collect_ncx_toc(np, ncx_dir, (depth + 1).min(6), chapter_by_path, out);
    }
}

impl EpubBook {
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
        let mut zip = ZipArchive::new(file).context("Invalid EPUB zip archive")?;

        let opf_path = if let Some(cont_xml) = read_zip_text(&mut zip, "META-INF/container.xml")
            && let Ok(cont_doc) = roxmltree::Document::parse(&cont_xml)
            && let Some(full_path) = cont_doc
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "rootfile")
                .and_then(|n| n.attribute("full-path"))
        {
            full_path.trim_start_matches('/').to_string()
        } else {
            let mut found = None;
            for i in 0..zip.len() {
                if let Ok(f) = zip.by_index(i)
                    && let Ok(fname) = f.name()
                    && fname.to_ascii_lowercase().ends_with(".opf")
                {
                    found = Some(fname.into_owned());
                    break;
                }
            }
            found.context("EPUB missing OPF package document")?
        };

        let opf_dir = opf_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let opf_xml = read_zip_text(&mut zip, &opf_path).context("Failed to read EPUB OPF")?;
        let opf_doc = roxmltree::Document::parse(&opf_xml).context("Invalid EPUB OPF XML")?;

        let title = opf_doc
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "title")
            .and_then(|n| n.text())
            .map(normalize_ws)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("EPUB Book")
                    .to_string()
            });

        let author = opf_doc
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "creator")
            .and_then(|n| n.text())
            .map(normalize_ws)
            .filter(|s| !s.is_empty());

        struct ManifestItem {
            path: String,
            media_type: String,
            properties: String,
        }
        let mut manifest: HashMap<String, ManifestItem> = HashMap::new();
        for item in opf_doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "item")
        {
            if let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) {
                manifest.insert(
                    id.to_string(),
                    ManifestItem {
                        path: resolve_zip_relative(opf_dir, href),
                        media_type: item.attribute("media-type").unwrap_or("").to_string(),
                        properties: item.attribute("properties").unwrap_or("").to_string(),
                    },
                );
            }
        }

        let mut spine_paths = Vec::new();
        for itemref in opf_doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "itemref")
        {
            if let Some(idref) = itemref.attribute("idref")
                && let Some(item) = manifest.get(idref)
            {
                spine_paths.push(item.path.clone());
            }
        }

        let mut chapter_by_path: HashMap<String, usize> = HashMap::new();
        let mut chapters = Vec::with_capacity(spine_paths.len());
        let mut total_words = 0usize;

        for (ix, ch_path) in spine_paths.iter().enumerate() {
            chapter_by_path.insert(ch_path.clone(), ix);
            let ch_dir = ch_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let raw_html = read_zip_text(&mut zip, ch_path).unwrap_or_default();
            let pre = replace_html_entities_numeric(&raw_html);
            let mut blocks = Vec::new();
            let mut ch_title: Option<String> = None;

            if let Ok(xhtml) = roxmltree::Document::parse(&pre) {
                ch_title = xhtml
                    .descendants()
                    .find(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("title"))
                    .and_then(|n| n.text())
                    .map(normalize_ws)
                    .filter(|s| !s.is_empty());

                let root_el = xhtml
                    .descendants()
                    .find(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("body"))
                    .unwrap_or_else(|| xhtml.root_element());
                walk_xhtml_blocks(root_el, ch_dir, false, &mut zip, &mut blocks);
            } else {
                blocks = strip_html_tags_fallback(&raw_html);
            }

            if ch_title.is_none() {
                ch_title = blocks.iter().find_map(|b| match b {
                    EpubBlock::Heading { text, .. } => Some(text.clone()),
                    _ => None,
                });
            }
            for blk in &blocks {
                match blk {
                    EpubBlock::Heading { text, .. }
                    | EpubBlock::Paragraph { text, .. }
                    | EpubBlock::ListItem(text) => total_words += text.split_whitespace().count(),
                    _ => {}
                }
            }

            chapters.push(EpubChapter {
                title: ch_title.unwrap_or_else(|| format!("Section {}", ix + 1)),
                blocks,
            });
        }

        // Parse TOC from EPUB3 nav first, else EPUB2 NCX
        let mut toc = Vec::new();
        if let Some(nav_item) = manifest
            .values()
            .find(|m| m.properties.split_whitespace().any(|p| p == "nav"))
            && let Some(nav_raw) = read_zip_text(&mut zip, &nav_item.path)
        {
            let nav_clean = replace_html_entities_numeric(&nav_raw);
            let nav_dir = nav_item.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            if let Ok(nav_doc) = roxmltree::Document::parse(&nav_clean)
                && let Some(nav_el) = nav_doc.descendants().find(|n| {
                    n.is_element()
                        && n.tag_name().name().eq_ignore_ascii_case("nav")
                        && n.attributes()
                            .any(|a| a.name() == "type" && a.value().contains("toc"))
                })
                && let Some(ol) = nav_el
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("ol"))
            {
                collect_nav_toc(ol, nav_dir, 1, &chapter_by_path, &mut toc);
            }
        }

        if toc.is_empty()
            && let Some(ncx_item) = manifest.values().find(|m| {
                m.media_type.contains("dtbncx") || m.path.to_ascii_lowercase().ends_with(".ncx")
            })
            && let Some(ncx_raw) = read_zip_text(&mut zip, &ncx_item.path)
        {
            let ncx_clean = replace_html_entities_numeric(&ncx_raw);
            let ncx_dir = ncx_item.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            if let Ok(ncx_doc) = roxmltree::Document::parse(&ncx_clean)
                && let Some(nav_map) = ncx_doc
                    .descendants()
                    .find(|n| n.is_element() && n.tag_name().name() == "navMap")
            {
                collect_ncx_toc(nav_map, ncx_dir, 1, &chapter_by_path, &mut toc);
            }
        }

        // Sync chapter titles from TOC if a chapter still has a generic Section N title
        for item in &toc {
            if let Some(ch) = chapters.get_mut(item.chapter_idx)
                && ch.title.starts_with("Section ")
            {
                ch.title = item.title.clone();
            }
        }
        if toc.is_empty() {
            for (ix, ch) in chapters.iter().enumerate() {
                toc.push(EpubTocItem {
                    title: ch.title.clone(),
                    chapter_idx: ix,
                    depth: 1,
                });
            }
        }

        Ok(Self {
            title,
            author,
            chapters,
            toc,
            total_words,
        })
    }

    pub fn outline_headings(&self) -> Vec<OutlineHeading> {
        self.toc
            .iter()
            .map(|t| OutlineHeading {
                level: t.depth.clamp(1, 6) as usize,
                title: t.title.clone(),
                line: t.chapter_idx + 1,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn loads_epub_chapters_entities_and_toc() {
        let tmp = tempfile::tempdir().unwrap();
        let epub_path = tmp.path().join("book.epub");
        {
            let file = File::create(&epub_path).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            zw.start_file("META-INF/container.xml", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(
                br#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
                <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
            </container>"#,
            )
            .unwrap();
            zw.start_file("OEBPS/content.opf", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(
                br#"<package xmlns="http://www.idpf.org/2007/opf" version="2.0" xmlns:dc="http://purl.org/dc/elements/1.1/">
                <metadata><dc:title>Pocket Book</dc:title><dc:creator>Ada Lovelace</dc:creator></metadata>
                <manifest>
                    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
                    <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
                    <item id="c2" href="ch2.xhtml" media-type="application/xhtml+xml"/>
                </manifest>
                <spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/></spine>
            </package>"#,
            )
            .unwrap();
            zw.start_file("OEBPS/toc.ncx", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(
                br#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/">
                <navMap>
                    <navPoint id="n1"><navLabel><text>Chapter One</text></navLabel><content src="ch1.xhtml#top"/></navPoint>
                    <navPoint id="n2"><navLabel><text>Chapter Two</text></navLabel><content src="ch2.xhtml"/></navPoint>
                </navMap>
            </ncx>"#,
            )
            .unwrap();
            zw.start_file("OEBPS/ch1.xhtml", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(
                br#"<!DOCTYPE html>
                <html xmlns="http://www.w3.org/1999/xhtml"><body>
                <h1>Arrival</h1>
                <p>Hello&nbsp;world&mdash;indeed.</p>
                <blockquote>A wise quote.</blockquote>
                <ul><li>First point</li></ul>
                </body></html>"#,
            )
            .unwrap();
            zw.start_file("OEBPS/ch2.xhtml", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Ending.</p></body></html>"#,
            )
            .unwrap();
            zw.finish().unwrap();
        }

        let book = EpubBook::load(&epub_path).unwrap();
        assert_eq!(book.title, "Pocket Book");
        assert_eq!(book.author.as_deref(), Some("Ada Lovelace"));
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.toc.len(), 2);
        assert_eq!(book.outline_headings()[1].line, 2);
        assert!(matches!(
            &book.chapters[0].blocks[0],
            EpubBlock::Heading { level: 1, text } if text == "Arrival"
        ));
        assert!(matches!(
            &book.chapters[0].blocks[1],
            EpubBlock::Paragraph { text, is_quote: false } if text.contains('\u{2014}')
        ));
        assert!(matches!(
            &book.chapters[0].blocks[2],
            EpubBlock::Paragraph { text, is_quote: true } if text == "A wise quote."
        ));
        assert!(matches!(
            &book.chapters[0].blocks[3],
            EpubBlock::ListItem(t) if t == "First point"
        ));
    }
}
