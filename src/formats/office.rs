use crate::vault::OutlineHeading;
use anyhow::{Context, Result};
use gpui_kit::{Image, ImageFormat};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use zip::ZipArchive;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeFormat {
    Docx,
    Odt,
}

impl OfficeFormat {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Docx => "DOCX",
            Self::Odt => "ODT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub code: bool,
}

#[derive(Clone)]
pub enum OfficeBlock {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph {
        runs: Vec<OfficeRun>,
        list_prefix: Option<String>,
    },
    Table {
        rows: Vec<Vec<String>>,
    },
    Image {
        image: Arc<Image>,
    },
}

#[derive(Clone)]
pub struct OfficeDoc {
    pub format: OfficeFormat,
    pub title: Option<String>,
    pub blocks: Vec<OfficeBlock>,
    pub word_count: usize,
    pub table_count: usize,
    pub headings: Vec<OutlineHeading>,
}

fn read_zip_string<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let mut entry = zip.by_name(name).ok()?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).ok()?;
    Some(buf)
}

pub(crate) fn image_format_from_name(name: &str) -> Option<ImageFormat> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        Some(ImageFormat::Png)
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        Some(ImageFormat::Jpeg)
    } else if lower.ends_with(".webp") {
        Some(ImageFormat::Webp)
    } else if lower.ends_with(".gif") {
        Some(ImageFormat::Gif)
    } else if lower.ends_with(".bmp") {
        Some(ImageFormat::Bmp)
    } else if lower.ends_with(".svg") {
        Some(ImageFormat::Svg)
    } else {
        None
    }
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

fn push_run(runs: &mut Vec<OfficeRun>, next: OfficeRun) {
    if next.text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && last.bold == next.bold
        && last.italic == next.italic
        && last.underline == next.underline
        && last.code == next.code
    {
        last.text.push_str(&next.text);
        return;
    }
    runs.push(next);
}

fn collect_plain_text(node: roxmltree::Node<'_, '_>, out: &mut String) {
    if node.is_text() {
        if let Some(t) = node.text() {
            out.push_str(t);
        }
        return;
    }
    match node.tag_name().name() {
        "tab" => out.push('\t'),
        "br" | "cr" | "line-break" => out.push('\n'),
        "s" => out.push(' '),
        _ => {
            for ch in node.children() {
                collect_plain_text(ch, out);
            }
        }
    }
}

fn format_runs_markdown(runs: &[OfficeRun]) -> String {
    let mut out = String::new();
    for r in runs {
        let t = r.text.as_str();
        if t.trim().is_empty() {
            out.push_str(t);
            continue;
        }
        if r.code {
            out.push('`');
            out.push_str(t);
            out.push('`');
        } else if r.bold && r.italic {
            out.push_str("***");
            out.push_str(t);
            out.push_str("***");
        } else if r.bold {
            out.push_str("**");
            out.push_str(t);
            out.push_str("**");
        } else if r.italic {
            out.push('*');
            out.push_str(t);
            out.push('*');
        } else {
            out.push_str(t);
        }
    }
    out
}

fn format_runs_typst(runs: &[OfficeRun]) -> String {
    let mut out = String::new();
    for r in runs {
        let t = r.text.as_str();
        if r.code {
            out.push('`');
            out.push_str(t);
            out.push('`');
        } else if r.bold && r.italic {
            out.push_str("*_");
            out.push_str(t);
            out.push_str("_*");
        } else if r.bold {
            out.push('*');
            out.push_str(t);
            out.push('*');
        } else if r.italic {
            out.push('_');
            out.push_str(t);
            out.push('_');
        } else {
            out.push_str(t);
        }
    }
    out
}

impl OfficeDoc {
    pub fn load(path: &Path) -> Result<Self> {
        let is_odt = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("odt"));
        let file = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
        let mut zip = ZipArchive::new(file).context("Invalid DOCX/ODT archive")?;

        let mut doc = if is_odt {
            Self::parse_odt(&mut zip)?
        } else {
            Self::parse_docx(&mut zip)?
        };

        let mut words = 0usize;
        let mut tables = 0usize;
        let mut headings = Vec::new();
        for (ix, blk) in doc.blocks.iter().enumerate() {
            match blk {
                OfficeBlock::Heading { level, text } => {
                    words += text.split_whitespace().count();
                    headings.push(OutlineHeading {
                        level: (*level) as usize,
                        title: text.clone(),
                        line: ix + 1,
                    });
                }
                OfficeBlock::Paragraph { runs, .. } => {
                    for r in runs {
                        words += r.text.split_whitespace().count();
                    }
                }
                OfficeBlock::Table { rows } => {
                    tables += 1;
                    for row in rows {
                        for cell in row {
                            words += cell.split_whitespace().count();
                        }
                    }
                }
                OfficeBlock::Image { .. } => {}
            }
        }
        doc.word_count = words;
        doc.table_count = tables;
        doc.headings = headings;
        Ok(doc)
    }

    fn parse_docx<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>) -> Result<Self> {
        let title = read_zip_string(zip, "docProps/core.xml").and_then(|xml| {
            let parsed = roxmltree::Document::parse(&xml).ok()?;
            parsed
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "title")
                .and_then(|n| n.text())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });

        let mut rel_targets: HashMap<String, String> = HashMap::new();
        if let Some(rels_xml) = read_zip_string(zip, "word/_rels/document.xml.rels")
            && let Ok(rels_doc) = roxmltree::Document::parse(&rels_xml)
        {
            for rel in rels_doc
                .descendants()
                .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
            {
                if let (Some(id), Some(target)) = (rel.attribute("Id"), rel.attribute("Target")) {
                    let clean = target.trim_start_matches("./").trim_start_matches('/');
                    let full = if clean.starts_with("word/") {
                        clean.to_string()
                    } else {
                        format!("word/{clean}")
                    };
                    rel_targets.insert(id.to_string(), full);
                }
            }
        }

        let xml = read_zip_string(zip, "word/document.xml")
            .context("DOCX missing word/document.xml")?;
        let parsed = roxmltree::Document::parse(&xml).context("Invalid word/document.xml")?;
        let body = parsed
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "body")
            .context("DOCX missing document body")?;

        let mut blocks = Vec::new();
        for child in body.children().filter(|n| n.is_element()) {
            match child.tag_name().name() {
                "p" => {
                    let p_pr = child
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == "pPr");
                    let style_val = p_pr
                        .and_then(|p| {
                            p.children()
                                .find(|n| n.is_element() && n.tag_name().name() == "pStyle")
                        })
                        .and_then(|s| s.attribute("val").or_else(|| s.attributes().next().map(|a| a.value())))
                        .unwrap_or("");
                    let heading_level = heading_level_from_style(style_val);
                    let is_list = p_pr.is_some_and(|p| {
                        p.children()
                            .any(|n| n.is_element() && n.tag_name().name() == "numPr")
                    });

                    // Extract embedded images inside this paragraph
                    for blip in child
                        .descendants()
                        .filter(|n| n.is_element() && n.tag_name().name() == "blip")
                    {
                        let embed_id = blip
                            .attributes()
                            .find(|a| a.name() == "embed" || a.name() == "link")
                            .map(|a| a.value());
                        if let Some(eid) = embed_id
                            && let Some(target_path) = rel_targets.get(eid)
                            && let Some(image) = read_zip_image(zip, target_path)
                        {
                            blocks.push(OfficeBlock::Image { image });
                        }
                    }

                    let mut runs = Vec::new();
                    for node in child.children().filter(|n| n.is_element()) {
                        match node.tag_name().name() {
                            "r" => parse_docx_run(node, &mut runs),
                            "hyperlink" => {
                                for r in node
                                    .children()
                                    .filter(|n| n.is_element() && n.tag_name().name() == "r")
                                {
                                    parse_docx_run(r, &mut runs);
                                }
                            }
                            _ => {}
                        }
                    }

                    let full_text: String = runs.iter().map(|r| r.text.as_str()).collect();
                    if let Some(level) = heading_level {
                        let clean = full_text.trim();
                        if !clean.is_empty() {
                            blocks.push(OfficeBlock::Heading {
                                level,
                                text: clean.to_string(),
                            });
                        }
                    } else if !full_text.trim().is_empty() {
                        blocks.push(OfficeBlock::Paragraph {
                            runs,
                            list_prefix: if is_list {
                                Some("• ".to_string())
                            } else {
                                None
                            },
                        });
                    }
                }
                "tbl" => {
                    let mut rows = Vec::new();
                    for tr in child
                        .children()
                        .filter(|n| n.is_element() && n.tag_name().name() == "tr")
                    {
                        let mut cells = Vec::new();
                        for tc in tr
                            .children()
                            .filter(|n| n.is_element() && n.tag_name().name() == "tc")
                        {
                            let mut cell_parts = Vec::new();
                            for p in tc
                                .children()
                                .filter(|n| n.is_element() && n.tag_name().name() == "p")
                            {
                                let mut pt = String::new();
                                for r in p.descendants().filter(|n| {
                                    n.is_element()
                                        && matches!(n.tag_name().name(), "t" | "tab" | "br")
                                }) {
                                    collect_plain_text(r, &mut pt);
                                }
                                let trimmed = pt.trim();
                                if !trimmed.is_empty() {
                                    cell_parts.push(trimmed.to_string());
                                }
                            }
                            cells.push(cell_parts.join(" "));
                        }
                        if !cells.is_empty() {
                            rows.push(cells);
                        }
                    }
                    if !rows.is_empty() {
                        blocks.push(OfficeBlock::Table { rows });
                    }
                }
                _ => {}
            }
        }

        Ok(Self {
            format: OfficeFormat::Docx,
            title,
            blocks,
            word_count: 0,
            table_count: 0,
            headings: Vec::new(),
        })
    }

    fn parse_odt<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>) -> Result<Self> {
        let title = read_zip_string(zip, "meta.xml").and_then(|xml| {
            let parsed = roxmltree::Document::parse(&xml).ok()?;
            parsed
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "title")
                .and_then(|n| n.text())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });

        let xml = read_zip_string(zip, "content.xml").context("ODT missing content.xml")?;
        let parsed = roxmltree::Document::parse(&xml).context("Invalid ODT content.xml")?;
        let text_body = parsed
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "text")
            .context("ODT missing office:text")?;

        let mut blocks = Vec::new();
        for child in text_body.children().filter(|n| n.is_element()) {
            collect_odt_block(child, None, zip, &mut blocks);
        }

        Ok(Self {
            format: OfficeFormat::Odt,
            title,
            blocks,
            word_count: 0,
            table_count: 0,
            headings: Vec::new(),
        })
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        for (ix, blk) in self.blocks.iter().enumerate() {
            if ix > 0 {
                out.push('\n');
            }
            match blk {
                OfficeBlock::Heading { level, text } => {
                    let hashes = "#".repeat((*level).clamp(1, 6) as usize);
                    out.push_str(&format!("{hashes} {text}\n"));
                }
                OfficeBlock::Paragraph { runs, list_prefix } => {
                    if list_prefix.is_some() {
                        out.push_str("- ");
                    }
                    out.push_str(&format_runs_markdown(runs));
                    out.push('\n');
                }
                OfficeBlock::Table { rows } => {
                    if let Some((first, rest)) = rows.split_first() {
                        out.push_str("| ");
                        out.push_str(&first.join(" | "));
                        out.push_str(" |\n| ");
                        out.push_str(&vec!["---"; first.len()].join(" | "));
                        out.push_str(" |\n");
                        for row in rest {
                            out.push_str("| ");
                            out.push_str(&row.join(" | "));
                            out.push_str(" |\n");
                        }
                    }
                }
                OfficeBlock::Image { .. } => {}
            }
        }
        out
    }

    pub fn to_typst(&self) -> String {
        let mut out = String::from("#set page(paper: \"a4\", margin: 2cm)\n#set text(size: 11pt)\n\n");
        for blk in &self.blocks {
            match blk {
                OfficeBlock::Heading { level, text } => {
                    let eq = "=".repeat((*level).clamp(1, 6) as usize);
                    out.push_str(&format!("{eq} {text}\n\n"));
                }
                OfficeBlock::Paragraph { runs, list_prefix } => {
                    if list_prefix.is_some() {
                        out.push_str("- ");
                    }
                    out.push_str(&format_runs_typst(runs));
                    out.push_str("\n\n");
                }
                OfficeBlock::Table { rows } => {
                    if let Some(first) = rows.first() {
                        out.push_str(&format!("#table(\n  columns: {},\n", first.len().max(1)));
                        for row in rows {
                            for cell in row {
                                let esc = cell.replace('[', "\\[").replace(']', "\\]");
                                out.push_str(&format!("  [{esc}],"));
                            }
                            out.push('\n');
                        }
                        out.push_str(")\n\n");
                    }
                }
                OfficeBlock::Image { .. } => {}
            }
        }
        out
    }
}

fn heading_level_from_style(style_val: &str) -> Option<u8> {
    let lower = style_val.to_ascii_lowercase();
    if lower == "title" {
        return Some(1);
    }
    if lower == "subtitle" {
        return Some(2);
    }
    let digits = lower
        .strip_prefix("heading")
        .or_else(|| lower.strip_prefix("heading "));
    digits.and_then(|d| d.trim().parse::<u8>().ok()).map(|n| n.clamp(1, 6))
}

fn parse_docx_run(run_node: roxmltree::Node<'_, '_>, out: &mut Vec<OfficeRun>) {
    let r_pr = run_node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "rPr");
    let mut bold = false;
    let mut italic = false;
    let mut underline = false;
    let mut code = false;
    if let Some(pr) = r_pr {
        for prop in pr.children().filter(|n| n.is_element()) {
            let off = prop
                .attributes()
                .any(|a| a.name() == "val" && matches!(a.value(), "0" | "false" | "none"));
            match prop.tag_name().name() {
                "b" | "bCs" if !off => bold = true,
                "i" | "iCs" if !off => italic = true,
                "u" if !off => underline = true,
                "rFonts" => {
                    if prop.attributes().any(|a| {
                        let v = a.value().to_ascii_lowercase();
                        v.contains("mono") || v.contains("courier") || v.contains("consolas")
                    }) {
                        code = true;
                    }
                }
                _ => {}
            }
        }
    }

    let mut text = String::new();
    for ch in run_node.children().filter(|n| n.is_element()) {
        match ch.tag_name().name() {
            "t" | "delText" | "instrText" => {
                if let Some(t) = ch.text() {
                    text.push_str(t);
                }
            }
            "tab" => text.push('\t'),
            "br" | "cr" => text.push('\n'),
            _ => {}
        }
    }
    push_run(
        out,
        OfficeRun {
            text,
            bold,
            italic,
            underline,
            code,
        },
    );
}

fn collect_odt_block<R: Read + std::io::Seek>(
    node: roxmltree::Node<'_, '_>,
    list_prefix: Option<&str>,
    zip: &mut ZipArchive<R>,
    out: &mut Vec<OfficeBlock>,
) {
    match node.tag_name().name() {
        "h" => {
            let lvl = node
                .attributes()
                .find(|a| a.name() == "outline-level")
                .and_then(|a| a.value().parse::<u8>().ok())
                .unwrap_or(1)
                .clamp(1, 6);
            let mut txt = String::new();
            collect_plain_text(node, &mut txt);
            let clean = txt.trim();
            if !clean.is_empty() {
                out.push(OfficeBlock::Heading {
                    level: lvl,
                    text: clean.to_string(),
                });
            }
        }
        "p" => {
            for img_el in node
                .descendants()
                .filter(|n| n.is_element() && n.tag_name().name() == "image")
            {
                if let Some(href) = img_el
                    .attributes()
                    .find(|a| a.name() == "href")
                    .map(|a| a.value())
                    && let Some(image) = read_zip_image(zip, href.trim_start_matches("./"))
                {
                    out.push(OfficeBlock::Image { image });
                }
            }
            let mut txt = String::new();
            collect_plain_text(node, &mut txt);
            if !txt.trim().is_empty() {
                out.push(OfficeBlock::Paragraph {
                    runs: vec![OfficeRun {
                        text: txt,
                        bold: false,
                        italic: false,
                        underline: false,
                        code: false,
                    }],
                    list_prefix: list_prefix.map(ToString::to_string),
                });
            }
        }
        "list" => {
            for li in node
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "list-item")
            {
                for ch in li.children().filter(|n| n.is_element()) {
                    collect_odt_block(ch, Some("• "), zip, out);
                }
            }
        }
        "table" => {
            let mut rows = Vec::new();
            for tr in node
                .children()
                .filter(|n| n.is_element() && n.tag_name().name() == "table-row")
            {
                let mut cells = Vec::new();
                for tc in tr
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "table-cell")
                {
                    let mut txt = String::new();
                    collect_plain_text(tc, &mut txt);
                    cells.push(txt.split_whitespace().collect::<Vec<_>>().join(" "));
                }
                if !cells.is_empty() {
                    rows.push(cells);
                }
            }
            if !rows.is_empty() {
                out.push(OfficeBlock::Table { rows });
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    #[test]
    fn loads_docx_and_odt_and_exports_markdown_typst() {
        let tmp = tempfile::tempdir().unwrap();
        let docx_path = tmp.path().join("sample.docx");
        {
            let file = File::create(&docx_path).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            zw.start_file("docProps/core.xml", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(b"<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>Spec Doc</dc:title></cp:coreProperties>")
                .unwrap();
            zw.start_file("word/document.xml", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:body>
                    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Overview</w:t></w:r></w:p>
                    <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>VKarma</w:t></w:r><w:r><w:t> rules.</w:t></w:r></w:p>
                    <w:tbl><w:tr><w:tc><w:p><w:r><w:t>ColA</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>ColB</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
                </w:body>
            </w:document>"#,
            )
            .unwrap();
            zw.finish().unwrap();
        }

        let docx = OfficeDoc::load(&docx_path).unwrap();
        assert_eq!(docx.format, OfficeFormat::Docx);
        assert_eq!(docx.title.as_deref(), Some("Spec Doc"));
        assert_eq!(docx.headings.len(), 1);
        assert_eq!(docx.table_count, 1);
        let md = docx.to_markdown();
        assert!(md.contains("# Overview"));
        assert!(md.contains("**VKarma** rules."));
        assert!(md.contains("| ColA | ColB |"));
        let typ = docx.to_typst();
        assert!(typ.contains("= Overview"));
        assert!(typ.contains("*VKarma* rules."));

        let odt_path = tmp.path().join("sample.odt");
        {
            let file = File::create(&odt_path).unwrap();
            let mut zw = zip::ZipWriter::new(file);
            zw.start_file("content.xml", SimpleFileOptions::default())
                .unwrap();
            zw.write_all(
                br#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0">
                <office:body><office:text>
                    <text:h text:outline-level="2">Chapter</text:h>
                    <text:p>Hello ODT</text:p>
                </office:text></office:body>
            </office:document-content>"#,
            )
            .unwrap();
            zw.finish().unwrap();
        }

        let odt = OfficeDoc::load(&odt_path).unwrap();
        assert_eq!(odt.format, OfficeFormat::Odt);
        assert_eq!(odt.headings[0].level, 2);
        assert!(odt.to_markdown().contains("## Chapter"));
    }
}
