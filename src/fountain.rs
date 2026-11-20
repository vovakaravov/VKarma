use crate::vault::OutlineHeading;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitlePageEntry {
    pub key: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FountainElement {
    SceneHeading {
        text: String,
        scene_number: Option<String>,
        line: usize,
    },
    Action {
        text: String,
        line: usize,
    },
    Character {
        name: String,
        dual: bool,
        line: usize,
    },
    Parenthetical {
        text: String,
        line: usize,
    },
    Dialogue {
        text: String,
        line: usize,
    },
    Transition {
        text: String,
        line: usize,
    },
    Centered {
        text: String,
        line: usize,
    },
    Section {
        level: usize,
        text: String,
        line: usize,
    },
    Synopsis {
        text: String,
        line: usize,
    },
    Note {
        text: String,
        line: usize,
    },
    Lyric {
        text: String,
        line: usize,
    },
    PageBreak {
        line: usize,
    },
}

#[derive(Debug, Clone, Default)]
pub struct FountainDocument {
    pub title_page: Vec<TitlePageEntry>,
    pub elements: Vec<FountainElement>,
    pub scene_count: usize,
    pub word_count: usize,
    pub estimated_pages: f32,
}

const KNOWN_TITLE_KEYS: &[&str] = &[
    "title",
    "credit",
    "author",
    "authors",
    "source",
    "notes",
    "draft date",
    "date",
    "contact",
    "copyright",
    "revision",
];

fn strip_boneyard_preserving_lines(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut in_comment = false;

    while i < bytes.len() {
        if !in_comment && i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            in_comment = true;
            i += 2;
            continue;
        }
        if in_comment && i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
            in_comment = false;
            i += 2;
            continue;
        }
        let ch = source[i..].chars().next().unwrap();
        if in_comment {
            if ch == '\n' {
                out.push('\n');
            }
        } else {
            out.push(ch);
        }
        i += ch.len_utf8();
    }
    out
}

fn parse_title_key_line(line: &str) -> Option<(String, String)> {
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == ' ' || c == '-' || c == '_')
    {
        return None;
    }
    let val = line[colon + 1..].trim().to_string();
    Some((key.to_string(), val))
}

fn looks_like_title_page(lines: &[&str]) -> bool {
    let first = match lines.first() {
        Some(l) if !l.trim().is_empty() => l.trim_end_matches('\r'),
        _ => return false,
    };
    let Some((first_key, _)) = parse_title_key_line(first) else {
        return false;
    };
    let is_known = KNOWN_TITLE_KEYS
        .iter()
        .any(|k| k.eq_ignore_ascii_case(&first_key));
    let has_lower = first_key.chars().any(|c| c.is_ascii_lowercase());
    if !is_known && !has_lower {
        return false;
    }

    for line in lines {
        let clean = line.trim_end_matches('\r');
        if clean.trim().is_empty() {
            break;
        }
        if parse_title_key_line(clean).is_some()
            || clean.starts_with("   ")
            || clean.starts_with('\t')
        {
            continue;
        }
        return false;
    }
    true
}

fn is_all_caps_text(s: &str) -> bool {
    let mut has_upper = false;
    for ch in s.chars() {
        if ch.is_lowercase() {
            return false;
        }
        if ch.is_uppercase() {
            has_upper = true;
        }
    }
    has_upper
}

fn parse_scene_heading_parts(raw: &str) -> Option<(String, Option<String>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let heading_body = if let Some(forced) = trimmed.strip_prefix('.') {
        if forced.starts_with('.') {
            return None;
        }
        forced.trim()
    } else {
        let upper = trimmed.to_ascii_uppercase();
        let prefixes = [
            "INT./EXT.",
            "INT/EXT.",
            "INT./EXT ",
            "INT/EXT ",
            "I/E.",
            "I/E ",
            "INT.",
            "EXT.",
            "EST.",
            "INT ",
            "EXT ",
        ];
        if !prefixes.iter().any(|p| upper.starts_with(p)) {
            return None;
        }
        trimmed
    };

    // Optional trailing scene number: `#12A#`
    if let Some(without_end_hash) = heading_body.strip_suffix('#')
        && let Some(open_hash) = without_end_hash.rfind('#')
    {
        let text = without_end_hash[..open_hash].trim().to_string();
        let num = without_end_hash[open_hash + 1..].trim().to_string();
        if !num.is_empty() && !text.is_empty() {
            return Some((text, Some(num)));
        }
    }

    Some((heading_body.to_string(), None))
}

fn is_transition_line(trimmed: &str, prev_blank: bool, next_blank: bool) -> Option<String> {
    if let Some(rest) = trimmed.strip_prefix('>') {
        if !trimmed.ends_with('<') {
            return Some(rest.trim().to_string());
        }
        return None;
    }
    if prev_blank && next_blank && is_all_caps_text(trimmed) {
        let upper = trimmed.to_ascii_uppercase();
        if upper.ends_with("TO:") || upper == "FADE IN:" || upper == "FADE OUT." || upper == "FADE OUT" {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn parse_character_line(trimmed: &str, prev_blank: bool, next_blank: bool) -> Option<(String, bool)> {
    if !prev_blank || next_blank || trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('!') || trimmed.starts_with('>') || trimmed.starts_with('.') {
        return None;
    }

    let (body, forced) = if let Some(rest) = trimmed.strip_prefix('@') {
        (rest.trim(), true)
    } else {
        (trimmed, false)
    };

    let (name_part, dual) = if let Some(no_caret) = body.strip_suffix('^') {
        (no_caret.trim(), true)
    } else {
        (body, false)
    };

    if name_part.is_empty() {
        return None;
    }

    if !forced {
        // Strip parenthetical extensions like `(CONT'D)` or `(V.O.)` when checking uppercase
        if !is_all_caps_text(name_part) {
            return None;
        }
        if parse_scene_heading_parts(name_part).is_some() {
            return None;
        }
    }

    Some((name_part.to_string(), dual))
}

pub fn parse_fountain(source: &str) -> FountainDocument {
    let clean_source = strip_boneyard_preserving_lines(source);
    let word_count = clean_source.split_whitespace().count();
    let raw_lines: Vec<&str> = clean_source.split('\n').collect();

    let mut title_page = Vec::new();
    let mut i = 0usize;

    if looks_like_title_page(&raw_lines) {
        while i < raw_lines.len() {
            let line = raw_lines[i].trim_end_matches('\r');
            if line.trim().is_empty() {
                i += 1;
                break;
            }
            if let Some((key, val)) = parse_title_key_line(line) {
                let mut values = Vec::new();
                if !val.is_empty() {
                    values.push(val);
                }
                title_page.push(TitlePageEntry { key, values });
            } else if let Some(last) = title_page.last_mut() {
                last.values.push(line.trim().to_string());
            }
            i += 1;
        }
    }

    let mut elements = Vec::new();
    let mut scene_count = 0usize;

    while i < raw_lines.len() {
        let line_num = i + 1;
        let raw_line = raw_lines[i].trim_end_matches('\r');
        let trimmed = raw_line.trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        let prev_blank = i == 0 || raw_lines[i - 1].trim_end_matches('\r').trim().is_empty();
        let next_blank =
            i + 1 >= raw_lines.len() || raw_lines[i + 1].trim_end_matches('\r').trim().is_empty();

        // Page break: === or longer
        if trimmed.len() >= 3 && trimmed.chars().all(|c| c == '=') {
            elements.push(FountainElement::PageBreak { line: line_num });
            i += 1;
            continue;
        }

        // Section: # .. ######
        let hash_cnt = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hash_cnt) && trimmed[hash_cnt..].starts_with(char::is_whitespace) {
            elements.push(FountainElement::Section {
                level: hash_cnt,
                text: trimmed[hash_cnt..].trim().to_string(),
                line: line_num,
            });
            i += 1;
            continue;
        }

        // Synopsis: = text
        if let Some(rest) = trimmed.strip_prefix('=')
            && !rest.starts_with('=')
        {
            elements.push(FountainElement::Synopsis {
                text: rest.trim().to_string(),
                line: line_num,
            });
            i += 1;
            continue;
        }

        // Standalone Note: [[text]]
        if trimmed.starts_with("[[") && trimmed.ends_with("]]") && trimmed.len() >= 4 {
            elements.push(FountainElement::Note {
                text: trimmed[2..trimmed.len() - 2].trim().to_string(),
                line: line_num,
            });
            i += 1;
            continue;
        }

        // Centered: > text <
        if let Some(rest) = trimmed.strip_prefix('>')
            && let Some(center_body) = rest.strip_suffix('<')
        {
            elements.push(FountainElement::Centered {
                text: center_body.trim().to_string(),
                line: line_num,
            });
            i += 1;
            continue;
        }

        // Lyrics: ~ text
        if let Some(lyric) = trimmed.strip_prefix('~') {
            elements.push(FountainElement::Lyric {
                text: lyric.trim().to_string(),
                line: line_num,
            });
            i += 1;
            continue;
        }

        // Forced Action: !text
        if let Some(forced_act) = raw_line.strip_prefix('!') {
            let mut text = forced_act.to_string();
            i += 1;
            while i < raw_lines.len() {
                let cont = raw_lines[i].trim_end_matches('\r');
                if cont.trim().is_empty() {
                    break;
                }
                text.push('\n');
                text.push_str(cont.strip_prefix('!').unwrap_or(cont));
                i += 1;
            }
            elements.push(FountainElement::Action {
                text,
                line: line_num,
            });
            continue;
        }

        // Transition
        if let Some(trans_text) = is_transition_line(trimmed, prev_blank, next_blank) {
            elements.push(FountainElement::Transition {
                text: trans_text,
                line: line_num,
            });
            i += 1;
            continue;
        }

        // Scene heading
        if prev_blank
            && let Some((heading_text, scene_number)) = parse_scene_heading_parts(trimmed)
        {
            scene_count += 1;
            elements.push(FountainElement::SceneHeading {
                text: heading_text,
                scene_number,
                line: line_num,
            });
            i += 1;
            continue;
        }

        // Character + Dialogue block
        if let Some((char_name, dual)) = parse_character_line(trimmed, prev_blank, next_blank) {
            elements.push(FountainElement::Character {
                name: char_name,
                dual,
                line: line_num,
            });
            i += 1;
            while i < raw_lines.len() {
                let d_line_num = i + 1;
                let d_raw = raw_lines[i].trim_end_matches('\r');
                let d_trim = d_raw.trim();
                if d_trim.is_empty() {
                    break;
                }
                if d_trim.starts_with('(') && d_trim.ends_with(')') {
                    elements.push(FountainElement::Parenthetical {
                        text: d_trim.to_string(),
                        line: d_line_num,
                    });
                    i += 1;
                } else {
                    let mut dial_buf = String::from(d_raw);
                    i += 1;
                    while i < raw_lines.len() {
                        let nxt = raw_lines[i].trim_end_matches('\r');
                        let nxt_trim = nxt.trim();
                        if nxt_trim.is_empty()
                            || (nxt_trim.starts_with('(') && nxt_trim.ends_with(')'))
                        {
                            break;
                        }
                        dial_buf.push('\n');
                        dial_buf.push_str(nxt);
                        i += 1;
                    }
                    elements.push(FountainElement::Dialogue {
                        text: dial_buf,
                        line: d_line_num,
                    });
                }
            }
            continue;
        }

        // Default: Action paragraph (collect until blank line or structural break)
        let mut action_buf = String::from(raw_line);
        i += 1;
        while i < raw_lines.len() {
            let nxt = raw_lines[i].trim_end_matches('\r');
            let nxt_trim = nxt.trim();
            if nxt_trim.is_empty()
                || nxt_trim.starts_with('#')
                || (nxt_trim.starts_with('=') && !nxt_trim.starts_with("=="))
                || (nxt_trim.len() >= 3 && nxt_trim.chars().all(|c| c == '='))
                || (nxt_trim.starts_with("[[") && nxt_trim.ends_with("]]"))
            {
                break;
            }
            action_buf.push('\n');
            action_buf.push_str(nxt);
            i += 1;
        }
        elements.push(FountainElement::Action {
            text: action_buf,
            line: line_num,
        });
    }

    let est_lines = estimate_screenplay_lines(&elements);
    let estimated_pages = if elements.is_empty() && title_page.is_empty() {
        0.0
    } else {
        (est_lines as f32 / 55.0).max(1.0)
    };

    FountainDocument {
        title_page,
        elements,
        scene_count,
        word_count,
        estimated_pages,
    }
}

fn estimate_screenplay_lines(elements: &[FountainElement]) -> usize {
    let mut lines = 0usize;
    for el in elements {
        match el {
            FountainElement::SceneHeading { text, .. } => {
                lines += 2 + (text.len() / 60).max(1);
            }
            FountainElement::Action { text, .. } => {
                lines += 1;
                for l in text.lines() {
                    lines += (l.len() / 60).max(1);
                }
            }
            FountainElement::Character { .. } => lines += 2,
            FountainElement::Parenthetical { .. } => lines += 1,
            FountainElement::Dialogue { text, .. } => {
                for l in text.lines() {
                    lines += (l.len() / 35).max(1);
                }
            }
            FountainElement::Transition { .. } | FountainElement::Centered { .. } => lines += 2,
            FountainElement::PageBreak { .. } => lines += 10,
            FountainElement::Section { .. }
            | FountainElement::Synopsis { .. }
            | FountainElement::Note { .. } => {}
            FountainElement::Lyric { .. } => lines += 1,
        }
    }
    lines
}

pub fn extract_fountain_headings(doc: &FountainDocument) -> Vec<OutlineHeading> {
    let has_sections = doc
        .elements
        .iter()
        .any(|e| matches!(e, FountainElement::Section { .. }));
    let scene_level = if has_sections { 2 } else { 1 };

    let mut out = Vec::new();
    let mut scene_idx = 0usize;
    for el in &doc.elements {
        match el {
            FountainElement::Section { level, text, line } => {
                out.push(OutlineHeading {
                    level: *level,
                    title: text.clone(),
                    line: *line,
                });
            }
            FountainElement::SceneHeading {
                text,
                scene_number,
                line,
            } => {
                scene_idx += 1;
                let num = scene_number
                    .as_deref()
                    .map(str::to_string)
                    .unwrap_or_else(|| scene_idx.to_string());
                out.push(OutlineHeading {
                    level: scene_level,
                    title: format!("{num}. {text}"),
                    line: *line,
                });
            }
            _ => {}
        }
    }
    out
}

fn escape_typst_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for ch in s.chars() {
        match ch {
            '\\' | '#' | '$' | '*' | '_' | '[' | ']' | '<' | '>' | '@' | '`' | '~' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str(" \\\n"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn fountain_to_typst(doc: &FountainDocument) -> String {
    let mut out = String::with_capacity(4096);
    out.push_str(
        r#"#set page(paper: "us-letter", margin: (left: 1.5in, right: 1in, top: 1in, bottom: 1in), header: align(right, context text(size: 10pt)[#counter(page).display("1.")]))
#set text(font: ("DejaVu Sans Mono", "Liberation Mono"), size: 11pt, lang: "en")
#set par(leading: 0.55em, spacing: 1em)

"#,
    );

    if !doc.title_page.is_empty() {
        let mut title = None;
        let mut credit = None;
        let mut author = None;
        let mut left_meta = Vec::new();
        let mut right_meta = Vec::new();

        for entry in &doc.title_page {
            let k_lower = entry.key.to_ascii_lowercase();
            let joined = entry.values.join("\n");
            match k_lower.as_str() {
                "title" => title = Some(joined),
                "credit" => credit = Some(joined),
                "author" | "authors" => author = Some(joined),
                "draft date" | "date" | "revision" => right_meta.push(joined),
                _ => left_meta.push(joined),
            }
        }

        out.push_str("#page(header: none)[\n  #v(3fr)\n  #align(center)[\n");
        if let Some(t) = title {
            let _ = writeln!(out, "    #text(weight: \"bold\", size: 14pt)[#underline[{}]] \\", escape_typst_text(&t));
        }
        if let Some(c) = credit {
            let _ = writeln!(out, "    #v(1.2em)\n    {} \\", escape_typst_text(&c));
        }
        if let Some(a) = author {
            let _ = writeln!(out, "    #v(0.8em)\n    {} \\", escape_typst_text(&a));
        }
        out.push_str("  ]\n  #v(4fr)\n  #grid(columns: (1fr, 1fr),\n");
        let _ = writeln!(out, "    align(left)[{}],", escape_typst_text(&left_meta.join("\n")));
        let _ = writeln!(out, "    align(right)[{}],", escape_typst_text(&right_meta.join("\n")));
        out.push_str("  )\n]\n\n");
    }

    let mut scene_num = 0usize;
    for el in &doc.elements {
        match el {
            FountainElement::SceneHeading {
                text, scene_number, ..
            } => {
                scene_num += 1;
                let num = scene_number
                    .as_deref()
                    .map(str::to_string)
                    .unwrap_or_else(|| scene_num.to_string());
                let _ = writeln!(
                    out,
                    "#v(1.2em)\n#grid(columns: (auto, 1fr), column-gutter: 0.8em, text(fill: luma(110))[{}], text(weight: \"bold\")[{}])\n",
                    escape_typst_text(&num),
                    escape_typst_text(&text.to_ascii_uppercase())
                );
            }
            FountainElement::Action { text, .. } => {
                let _ = writeln!(out, "{}\n", escape_typst_text(text));
            }
            FountainElement::Character { name, dual, .. } => {
                let label = if *dual {
                    format!("{name} ^")
                } else {
                    name.clone()
                };
                let _ = writeln!(
                    out,
                    "#v(0.6em)\n#pad(left: 2.0in)[#text(weight: \"bold\")[{}]]",
                    escape_typst_text(&label.to_ascii_uppercase())
                );
            }
            FountainElement::Parenthetical { text, .. } => {
                let _ = writeln!(
                    out,
                    "#pad(left: 1.5in, right: 1.4in)[{}]",
                    escape_typst_text(text)
                );
            }
            FountainElement::Dialogue { text, .. } => {
                let _ = writeln!(
                    out,
                    "#pad(left: 1.0in, right: 1.2in)[{}]\n",
                    escape_typst_text(text)
                );
            }
            FountainElement::Transition { text, .. } => {
                let _ = writeln!(
                    out,
                    "#align(right)[#text(weight: \"bold\")[{}]]\n",
                    escape_typst_text(&text.to_ascii_uppercase())
                );
            }
            FountainElement::Centered { text, .. } => {
                let _ = writeln!(out, "#align(center)[{}]\n", escape_typst_text(text));
            }
            FountainElement::Lyric { text, .. } => {
                let _ = writeln!(out, "#emph[{}]\n", escape_typst_text(text));
            }
            FountainElement::PageBreak { .. } => {
                out.push_str("#pagebreak()\n");
            }
            FountainElement::Section { .. }
            | FountainElement::Synopsis { .. }
            | FountainElement::Note { .. } => {}
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typst_engine::export_typst_pdf;
    use tempfile::TempDir;

    #[test]
    fn parses_fountain_elements_and_title_page() {
        let src = "\
Title: MORTAL SOULS
Credit: Written by
Author: Vladimir Karashchuk
Draft date: 2026-04-06
Contact:
    author@example.com

# ACT ONE
= Arena introduction

/* This is a boneyard note
across two lines */
INT. SAND PIT ARENA - EVENING #1A#

Sand swirls across the stone floor.

MONSOON
(leaning on his blade)
Are we really doing this again?

CUT TO:

> THE END <
";
        let doc = parse_fountain(src);
        assert_eq!(doc.title_page.len(), 5);
        assert_eq!(doc.scene_count, 1);

        let heads = extract_fountain_headings(&doc);
        assert_eq!(heads.len(), 2);
        assert_eq!(heads[0].title, "ACT ONE");
        assert_eq!(heads[1].title, "1A. INT. SAND PIT ARENA - EVENING");

        assert!(doc.elements.iter().any(|e| matches!(
            e,
            FountainElement::Character { name, .. } if name == "MONSOON"
        )));
        assert!(doc.elements.iter().any(|e| matches!(
            e,
            FountainElement::Parenthetical { text, .. } if text == "(leaning on his blade)"
        )));
        assert!(doc.elements.iter().any(|e| matches!(
            e,
            FountainElement::Dialogue { text, .. } if text == "Are we really doing this again?"
        )));
        assert!(doc.elements.iter().any(|e| matches!(
            e,
            FountainElement::Transition { text, .. } if text == "CUT TO:"
        )));
        assert!(doc.elements.iter().any(|e| matches!(
            e,
            FountainElement::Centered { text, .. } if text == "THE END"
        )));
    }

    #[test]
    fn fountain_to_typst_compiles_clean_pdf() {
        let tmp = TempDir::new().unwrap();
        let src = "\
Title: TEST SCRIPT
Author: Writer

INT. CABIN - NIGHT

Wind rattles the glass.

ALEX
Did you hear that?
";
        let doc = parse_fountain(src);
        let typst_src = fountain_to_typst(&doc);
        let pdf_out = tmp.path().join("script.pdf");
        let fake_doc = tmp.path().join("script.fountain");
        let compiled = export_typst_pdf(&typst_src, &fake_doc, tmp.path(), &pdf_out).unwrap();
        assert!(std::fs::metadata(compiled).unwrap().len() > 200);
    }
}
