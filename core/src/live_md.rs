pub use crate::image_spec::{
    ImageAlign, StyledImageSpec, compute_image_display_size, format_image_spec, parse_image_spec,
};
use std::ops::Range;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownFormatAction {
    Heading,
    Bold,
    Italic,
    Checkbox,
    Bullet,
    Quote,
    Code,
    Wikilink,
    HorizontalRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineBlockContext {
    Frontmatter {
        is_delim: bool,
        is_first: bool,
        is_last: bool,
    },
    CodeFence {
        is_open: bool,
        is_close: bool,
        lang: Option<String>,
        is_first: bool,
        is_last: bool,
    },
    Table {
        is_header: bool,
        is_delim: bool,
        is_first: bool,
        is_last: bool,
        table_start_line: usize,
        table_end_line: usize,
        body_row_index: usize,
    },
    Callout {
        kind: String,
        is_header: bool,
        is_first: bool,
        is_last: bool,
    },
    Blockquote {
        depth: u8,
        prefix_bytes: usize,
        is_first: bool,
        is_last: bool,
    },
    Heading {
        level: u8,
        prefix_bytes: usize,
    },
    SceneBreak,
    ImageEmbed {
        target: String,
        alt: String,
    },
    TaskItem {
        indent_cols: usize,
        checked: bool,
        prefix_bytes: usize,
        checkbox_byte: usize,
    },
    ListItem {
        indent_cols: usize,
        ordered: bool,
        marker: String,
        prefix_bytes: usize,
    },
    MdxBlock {
        is_first: bool,
        is_last: bool,
    },
    Blank,
    Paragraph,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocLine {
    pub byte_start: usize,
    pub byte_end: usize,
    pub ctx: LineBlockContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub highlight: bool,
    pub link: bool,
    pub wikilink: bool,
    pub syntax_muted: bool,
    pub heading_level: u8,
    pub monospace: bool,
    pub dim: bool,
    pub callout_title: bool,
    pub checked_task: bool,
    pub bullet_marker: bool,
    pub table_pipe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSpan {
    pub range: Range<usize>,
    pub style: InlineStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OffsetSegment {
    Verbatim { raw: Range<usize>, disp: Range<usize> },
    Hidden { raw: Range<usize>, disp_pos: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    Wikilink(String),
    Url(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedLink {
    pub disp_range: Range<usize>,
    pub raw_range: Range<usize>,
    pub target: LinkTarget,
    pub concealed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixVisual {
    None,
    Bullet {
        indent_cols: usize,
    },
    Checkbox {
        indent_cols: usize,
        checked: bool,
        raw_checkbox_byte: usize,
    },
    SceneBreakRule,
    CodeFenceBar {
        lang: Option<String>,
        is_open: bool,
    },
    CalloutBadge {
        kind: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedLine {
    pub display_text: String,
    pub spans: Vec<StyledSpan>,
    pub segments: Vec<OffsetSegment>,
    pub links: Vec<ProjectedLink>,
    pub prefix_visual: PrefixVisual,
    pub raw_len: usize,
}

impl ProjectedLine {
    /// Maps a line-local UTF-8 byte offset in the raw line to the projected display text.
    pub fn raw_to_display(&self, raw_byte: usize) -> usize {
        let rb = raw_byte.min(self.raw_len);
        for seg in &self.segments {
            match seg {
                OffsetSegment::Verbatim { raw, disp } => {
                    if rb >= raw.start && rb <= raw.end {
                        let d = disp.start + (rb - raw.start);
                        return clamp_char_boundary(&self.display_text, d);
                    }
                }
                OffsetSegment::Hidden { raw, disp_pos } => {
                    if rb >= raw.start && rb <= raw.end {
                        return clamp_char_boundary(&self.display_text, *disp_pos);
                    }
                }
            }
        }
        self.display_text.len()
    }

    /// Maps a UTF-8 byte offset in `display_text` back to the raw line.
    pub fn display_to_raw(&self, raw_line: &str, disp_byte: usize) -> usize {
        let db = disp_byte.min(self.display_text.len());
        for seg in &self.segments {
            if let OffsetSegment::Verbatim { raw, disp } = seg
                && db >= disp.start
                && db <= disp.end
            {
                let r = raw.start + (db - disp.start);
                return clamp_char_boundary(raw_line, r);
            }
        }
        for seg in &self.segments {
            if let OffsetSegment::Hidden { raw, disp_pos } = seg
                && db <= *disp_pos
            {
                return clamp_char_boundary(raw_line, raw.end);
            }
        }
        raw_line.len()
    }
}

pub fn clamp_char_boundary(s: &str, byte_idx: usize) -> usize {
    let mut idx = byte_idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

pub fn utf8_to_utf16_offset(s: &str, utf8_byte: usize) -> usize {
    let clamped = clamp_char_boundary(s, utf8_byte);
    s[..clamped].encode_utf16().count()
}

pub fn utf16_to_utf8_offset(s: &str, utf16_units: usize) -> usize {
    let mut cur_utf16 = 0usize;
    for (byte_ix, ch) in s.char_indices() {
        if cur_utf16 >= utf16_units {
            return byte_ix;
        }
        cur_utf16 += ch.len_utf16();
    }
    s.len()
}

/// Classifies every line of a Markdown/MDX document into block contexts with exact raw byte spans.
pub fn classify_document_lines(raw: &str, is_mdx: bool) -> Vec<DocLine> {
    struct Span<'a> {
        start: usize,
        end: usize,
        text: &'a str,
    }

    let mut lines = Vec::new();
    let mut cursor = 0usize;
    for chunk in raw.split('\n') {
        let clean = chunk.trim_end_matches('\r');
        lines.push(Span {
            start: cursor,
            end: cursor + clean.len(),
            text: clean,
        });
        cursor += chunk.len() + 1;
    }

    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0usize;

    while i < lines.len() {
        let trimmed = lines[i].text.trim();

        // 1. YAML Frontmatter at line 0 when closed later.
        if i == 0 && trimmed == "---" {
            if let Some(close_idx) = (1..lines.len()).find(|&j| {
                let t = lines[j].text.trim();
                t == "---" || t == "..."
            }) {
                for j in 0..=close_idx {
                    out.push(DocLine {
                        byte_start: lines[j].start,
                        byte_end: lines[j].end,
                        ctx: LineBlockContext::Frontmatter {
                            is_delim: j == 0 || j == close_idx,
                            is_first: j == 0,
                            is_last: j == close_idx,
                        },
                    });
                }
                i = close_idx + 1;
                continue;
            }
        }

        // 2. Fenced code block (``` or ~~~).
        if let Some((fence_ch, fence_len, lang)) = parse_fence_open(trimmed) {
            let close_idx = ((i + 1)..lines.len())
                .find(|&j| is_fence_close(lines[j].text.trim(), fence_ch, fence_len))
                .unwrap_or(lines.len() - 1);
            for j in i..=close_idx {
                let is_open = j == i;
                let is_close = j == close_idx
                    && j > i
                    && is_fence_close(lines[j].text.trim(), fence_ch, fence_len);
                out.push(DocLine {
                    byte_start: lines[j].start,
                    byte_end: lines[j].end,
                    ctx: LineBlockContext::CodeFence {
                        is_open,
                        is_close,
                        lang: if is_open { lang.clone() } else { None },
                        is_first: j == i,
                        is_last: j == close_idx,
                    },
                });
            }
            i = close_idx + 1;
            continue;
        }

        // 3. Markdown GFM table.
        if has_table_pipe(trimmed)
            && !trimmed.is_empty()
            && i + 1 < lines.len()
            && is_table_delimiter(lines[i].text, lines[i + 1].text)
        {
            let mut end_idx = i + 1;
            while end_idx + 1 < lines.len()
                && is_table_continuation_line(lines[end_idx + 1].text, is_mdx)
            {
                end_idx += 1;
            }
            for j in i..=end_idx {
                out.push(DocLine {
                    byte_start: lines[j].start,
                    byte_end: lines[j].end,
                    ctx: LineBlockContext::Table {
                        is_header: j == i,
                        is_delim: j == i + 1,
                        is_first: j == i,
                        is_last: j == end_idx,
                        table_start_line: i,
                        table_end_line: end_idx,
                        body_row_index: j.saturating_sub(i + 2),
                    },
                });
            }
            i = end_idx + 1;
            continue;
        }

        // 4. Callouts and blockquotes (contiguous > lines).
        if trimmed.starts_with('>') {
            let mut end_idx = i;
            while end_idx + 1 < lines.len() && lines[end_idx + 1].text.trim_start().starts_with('>')
            {
                end_idx += 1;
            }
            let callout_kind = parse_callout_kind(trimmed);
            for j in i..=end_idx {
                let (depth, prefix_bytes) = parse_quote_prefix(lines[j].text);
                let ctx = if let Some(ref kind) = callout_kind {
                    LineBlockContext::Callout {
                        kind: kind.clone(),
                        is_header: j == i,
                        is_first: j == i,
                        is_last: j == end_idx,
                    }
                } else {
                    LineBlockContext::Blockquote {
                        depth,
                        prefix_bytes,
                        is_first: j == i,
                        is_last: j == end_idx,
                    }
                };
                out.push(DocLine {
                    byte_start: lines[j].start,
                    byte_end: lines[j].end,
                    ctx,
                });
            }
            i = end_idx + 1;
            continue;
        }

        // 5. MDX imports, exports, or JSX blocks.
        if is_mdx && starts_mdx_block(trimmed) {
            let line_strs: Vec<&str> = lines.iter().map(|l| l.text).collect();
            let end_idx = find_mdx_block_end(&line_strs, i);
            for j in i..=end_idx {
                out.push(DocLine {
                    byte_start: lines[j].start,
                    byte_end: lines[j].end,
                    ctx: LineBlockContext::MdxBlock {
                        is_first: j == i,
                        is_last: j == end_idx,
                    },
                });
            }
            i = end_idx + 1;
            continue;
        }

        // 6. Single line.
        let ctx = classify_single_line(lines[i].text);
        out.push(DocLine {
            byte_start: lines[i].start,
            byte_end: lines[i].end,
            ctx,
        });
        i += 1;
    }

    out
}

fn classify_single_line(line: &str) -> LineBlockContext {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return LineBlockContext::Blank;
    }
    let leading_ws = line.len() - line.trim_start_matches([' ', '\t']).len();
    if let Some((level, after_prefix)) = parse_atx_heading_with_len(&line[leading_ws..]) {
        return LineBlockContext::Heading {
            level,
            prefix_bytes: leading_ws + after_prefix,
        };
    }
    if is_scene_break(trimmed) {
        return LineBlockContext::SceneBreak;
    }
    if let Some((target, alt)) = parse_image_embed(trimmed) {
        return LineBlockContext::ImageEmbed { target, alt };
    }
    if let Some(prefix) = parse_list_or_task_prefix(line) {
        if let Some(cb_off) = prefix.checkbox_byte {
            return LineBlockContext::TaskItem {
                indent_cols: indent_columns(line),
                checked: prefix.checked,
                prefix_bytes: prefix.prefix_len,
                checkbox_byte: cb_off,
            };
        }
        return LineBlockContext::ListItem {
            indent_cols: indent_columns(line),
            ordered: prefix.ordered,
            marker: prefix.marker_display,
            prefix_bytes: prefix.prefix_len,
        };
    }
    LineBlockContext::Paragraph
}

fn indent_columns(line: &str) -> usize {
    let mut cols = 0usize;
    for b in line.bytes() {
        match b {
            b' ' => cols += 1,
            b'\t' => cols += 4,
            _ => break,
        }
    }
    cols / 2
}

pub fn list_indent_px_from_cols(indent_cols: usize) -> f32 {
    indent_cols as f32 * 20.0
}

fn parse_callout_kind(trimmed: &str) -> Option<String> {
    let after = trimmed.strip_prefix('>')?.trim_start();
    let rest = after.strip_prefix("[!")?;
    let end = rest.find(']')?;
    let kind = rest[..end].trim();
    if kind.is_empty() {
        None
    } else {
        Some(kind.to_ascii_lowercase())
    }
}

fn parse_quote_prefix(line: &str) -> (u8, usize) {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    let mut depth = 0u8;
    while i < bytes.len() {
        let mut ws = i;
        while ws < bytes.len() && matches!(bytes[ws], b' ' | b'\t') {
            ws += 1;
        }
        if bytes.get(ws) == Some(&b'>') {
            depth = depth.saturating_add(1);
            i = ws + 1;
            if bytes.get(i) == Some(&b' ') {
                i += 1;
            }
        } else {
            break;
        }
    }
    (depth.max(1), i)
}

struct ProjectionBuilder<'a> {
    raw: &'a str,
    raw_pos: usize,
    display_text: String,
    spans: Vec<StyledSpan>,
    segments: Vec<OffsetSegment>,
    links: Vec<ProjectedLink>,
}

impl<'a> ProjectionBuilder<'a> {
    fn new(raw: &'a str) -> Self {
        Self {
            raw,
            raw_pos: 0,
            display_text: String::with_capacity(raw.len()),
            spans: Vec::new(),
            segments: Vec::new(),
            links: Vec::new(),
        }
    }

    fn push_hidden(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        let end = (self.raw_pos + len).min(self.raw.len());
        let disp_pos = self.display_text.len();
        self.segments.push(OffsetSegment::Hidden {
            raw: self.raw_pos..end,
            disp_pos,
        });
        self.raw_pos = end;
    }

    fn push_verbatim(&mut self, len: usize, style: InlineStyle) {
        if len == 0 {
            return;
        }
        let end = clamp_char_boundary(self.raw, self.raw_pos + len);
        if end <= self.raw_pos {
            return;
        }
        let slice = &self.raw[self.raw_pos..end];
        let disp_start = self.display_text.len();
        self.display_text.push_str(slice);
        let disp_end = self.display_text.len();

        if let Some(last) = self.spans.last_mut()
            && last.style == style
            && last.range.end == disp_start
        {
            last.range.end = disp_end;
        } else {
            self.spans.push(StyledSpan {
                range: disp_start..disp_end,
                style,
            });
        }

        if let Some(OffsetSegment::Verbatim { raw, disp }) = self.segments.last_mut()
            && raw.end == self.raw_pos
            && disp.end == disp_start
        {
            raw.end = end;
            disp.end = disp_end;
        } else {
            self.segments.push(OffsetSegment::Verbatim {
                raw: self.raw_pos..end,
                disp: disp_start..disp_end,
            });
        }

        self.raw_pos = end;
    }
}

fn range_touched(cursor: &Option<Range<usize>>, start: usize, end: usize) -> bool {
    let Some(c) = cursor else {
        return false;
    };
    c.start <= end && c.end >= start
}

/// Projects a single raw document line into display text, styles, offset map, and prefix decorations.
pub fn project_line(
    raw_line: &str,
    ctx: &LineBlockContext,
    cursor_on_line: Option<Range<usize>>,
) -> ProjectedLine {
    let mut b = ProjectionBuilder::new(raw_line);
    let mut prefix_visual = PrefixVisual::None;
    let cursor_present = cursor_on_line.is_some();

    match ctx {
        LineBlockContext::Blank => {}
        LineBlockContext::Frontmatter { is_delim, .. } => {
            let style = InlineStyle {
                monospace: true,
                dim: !*is_delim,
                syntax_muted: *is_delim,
                ..Default::default()
            };
            b.push_verbatim(raw_line.len(), style);
        }
        LineBlockContext::CodeFence {
            is_open,
            is_close,
            lang,
            ..
        } => {
            if (*is_open || *is_close) && !cursor_present {
                b.push_hidden(raw_line.len());
                prefix_visual = PrefixVisual::CodeFenceBar {
                    lang: lang.clone(),
                    is_open: *is_open,
                };
            } else {
                let style = InlineStyle {
                    monospace: true,
                    syntax_muted: *is_open || *is_close,
                    ..Default::default()
                };
                b.push_verbatim(raw_line.len(), style);
            }
        }
        LineBlockContext::MdxBlock { .. } => {
            let style = InlineStyle {
                monospace: true,
                syntax_muted: !cursor_present,
                ..Default::default()
            };
            b.push_verbatim(raw_line.len(), style);
        }
        LineBlockContext::SceneBreak => {
            if cursor_present {
                b.push_verbatim(
                    raw_line.len(),
                    InlineStyle {
                        syntax_muted: true,
                        ..Default::default()
                    },
                );
            } else {
                b.push_hidden(raw_line.len());
                prefix_visual = PrefixVisual::SceneBreakRule;
            }
        }
        LineBlockContext::ImageEmbed { .. } => {
            if cursor_present {
                b.push_verbatim(
                    raw_line.len(),
                    InlineStyle {
                        monospace: true,
                        syntax_muted: true,
                        ..Default::default()
                    },
                );
            } else {
                b.push_hidden(raw_line.len());
            }
        }
        LineBlockContext::Table {
            is_header,
            is_delim,
            ..
        } => {
            let base = InlineStyle {
                monospace: true,
                bold: *is_header,
                dim: *is_delim,
                ..Default::default()
            };
            if *is_delim {
                b.push_verbatim(raw_line.len(), base);
            } else {
                emit_table_row(&mut b, base, &cursor_on_line);
            }
        }
        LineBlockContext::Heading {
            level,
            prefix_bytes,
        } => {
            let base = InlineStyle {
                heading_level: *level,
                bold: true,
                ..Default::default()
            };
            if cursor_present {
                b.push_verbatim(
                    *prefix_bytes,
                    InlineStyle {
                        heading_level: *level,
                        syntax_muted: true,
                        ..Default::default()
                    },
                );
            } else {
                b.push_hidden(*prefix_bytes);
            }
            emit_inline(&mut b, raw_line.len(), base, &cursor_on_line);
        }
        LineBlockContext::TaskItem {
            indent_cols,
            checked,
            prefix_bytes,
            checkbox_byte,
        } => {
            let reveal_prefix = range_touched(&cursor_on_line, 0, *prefix_bytes);
            if reveal_prefix {
                b.push_verbatim(
                    *prefix_bytes,
                    InlineStyle {
                        syntax_muted: true,
                        ..Default::default()
                    },
                );
            } else {
                b.push_hidden(*prefix_bytes);
                prefix_visual = PrefixVisual::Checkbox {
                    indent_cols: *indent_cols,
                    checked: *checked,
                    raw_checkbox_byte: *checkbox_byte,
                };
            }
            let base = InlineStyle {
                checked_task: *checked && !cursor_present,
                strikethrough: *checked && !cursor_present,
                ..Default::default()
            };
            emit_inline(&mut b, raw_line.len(), base, &cursor_on_line);
        }
        LineBlockContext::ListItem {
            indent_cols,
            ordered,
            prefix_bytes,
            ..
        } => {
            if *ordered {
                let ws_len = raw_line.len() - raw_line.trim_start_matches([' ', '\t']).len();
                if range_touched(&cursor_on_line, 0, *prefix_bytes) {
                    b.push_verbatim(
                        *prefix_bytes,
                        InlineStyle {
                            bullet_marker: true,
                            ..Default::default()
                        },
                    );
                } else {
                    b.push_hidden(ws_len);
                    b.push_verbatim(
                        prefix_bytes.saturating_sub(ws_len),
                        InlineStyle {
                            bullet_marker: true,
                            ..Default::default()
                        },
                    );
                }
            } else if range_touched(&cursor_on_line, 0, *prefix_bytes) {
                b.push_verbatim(
                    *prefix_bytes,
                    InlineStyle {
                        syntax_muted: true,
                        ..Default::default()
                    },
                );
            } else {
                b.push_hidden(*prefix_bytes);
                prefix_visual = PrefixVisual::Bullet {
                    indent_cols: *indent_cols,
                };
            }
            emit_inline(&mut b, raw_line.len(), InlineStyle::default(), &cursor_on_line);
        }
        LineBlockContext::Callout {
            kind, is_header, ..
        } => {
            let (_, quote_len) = parse_quote_prefix(raw_line);
            if *is_header {
                if let Some(rel_end) = raw_line[quote_len..].find(']') {
                    let marker_end = (quote_len + rel_end + 1
                        + usize::from(raw_line.as_bytes().get(quote_len + rel_end + 1) == Some(&b' ')))
                    .min(raw_line.len());
                    if cursor_present {
                        b.push_verbatim(
                            marker_end,
                            InlineStyle {
                                syntax_muted: true,
                                callout_title: true,
                                bold: true,
                                ..Default::default()
                            },
                        );
                    } else {
                        b.push_hidden(marker_end);
                        prefix_visual = PrefixVisual::CalloutBadge { kind: kind.clone() };
                    }
                    emit_inline(
                        &mut b,
                        raw_line.len(),
                        InlineStyle {
                            callout_title: true,
                            bold: true,
                            ..Default::default()
                        },
                        &cursor_on_line,
                    );
                } else {
                    emit_inline(&mut b, raw_line.len(), InlineStyle::default(), &cursor_on_line);
                }
            } else {
                if range_touched(&cursor_on_line, 0, quote_len) {
                    b.push_verbatim(
                        quote_len,
                        InlineStyle {
                            syntax_muted: true,
                            ..Default::default()
                        },
                    );
                } else {
                    b.push_hidden(quote_len);
                }
                emit_inline(&mut b, raw_line.len(), InlineStyle::default(), &cursor_on_line);
            }
        }
        LineBlockContext::Blockquote { prefix_bytes, .. } => {
            if range_touched(&cursor_on_line, 0, *prefix_bytes) {
                b.push_verbatim(
                    *prefix_bytes,
                    InlineStyle {
                        syntax_muted: true,
                        italic: true,
                        ..Default::default()
                    },
                );
            } else {
                b.push_hidden(*prefix_bytes);
            }
            emit_inline(
                &mut b,
                raw_line.len(),
                InlineStyle {
                    italic: true,
                    dim: true,
                    ..Default::default()
                },
                &cursor_on_line,
            );
        }
        LineBlockContext::Paragraph => {
            emit_inline(&mut b, raw_line.len(), InlineStyle::default(), &cursor_on_line);
        }
    }

    if b.raw_pos < raw_line.len() {
        b.push_verbatim(raw_line.len() - b.raw_pos, InlineStyle::default());
    }

    ProjectedLine {
        display_text: b.display_text,
        spans: b.spans,
        segments: b.segments,
        links: b.links,
        prefix_visual,
        raw_len: raw_line.len(),
    }
}

fn emit_table_row(
    b: &mut ProjectionBuilder<'_>,
    base: InlineStyle,
    cursor: &Option<Range<usize>>,
) {
    let raw = b.raw;
    let pipes = collect_top_level_pipes(raw);
    for pipe_pos in pipes {
        if pipe_pos > b.raw_pos {
            emit_inline(b, pipe_pos, base, cursor);
        }
        if b.raw_pos == pipe_pos {
            b.push_verbatim(
                1,
                InlineStyle {
                    table_pipe: true,
                    syntax_muted: true,
                    ..base
                },
            );
        }
    }
    if b.raw_pos < raw.len() {
        emit_inline(b, raw.len(), base, cursor);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTableCell {
    pub inner_range: Range<usize>,
    pub content_range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedTableCell {
    pub inner_range: Option<Range<usize>>,
    pub raw_offset: usize,
    pub projected: ProjectedLine,
}

pub fn parse_table_row_cells(raw_line: &str) -> Vec<RawTableCell> {
    let pipes = collect_top_level_pipes(raw_line);
    if pipes.is_empty() {
        return vec![make_raw_cell(raw_line, 0, raw_line.len())];
    }

    let first_pipe = pipes[0];
    let last_pipe = *pipes.last().unwrap();
    let has_leading_pipe = raw_line[..first_pipe].trim().is_empty();
    let has_trailing_pipe = (pipes.len() >= 2 || !has_leading_pipe)
        && raw_line[last_pipe + 1..].trim().is_empty();

    let mut raw_spans: Vec<(usize, usize)> = Vec::new();
    if !has_leading_pipe {
        raw_spans.push((0, first_pipe));
    }
    for win in pipes.windows(2) {
        raw_spans.push((win[0] + 1, win[1]));
    }
    if !has_trailing_pipe {
        raw_spans.push((last_pipe + 1, raw_line.len()));
    }

    if raw_spans.is_empty() {
        raw_spans.push((first_pipe + 1, raw_line.len()));
    }

    raw_spans
        .into_iter()
        .map(|(s, e)| make_raw_cell(raw_line, s, e))
        .collect()
}

fn make_raw_cell(raw_line: &str, start: usize, end: usize) -> RawTableCell {
    let slice = &raw_line[start..end];
    let trimmed = slice.trim();
    let content_range = if trimmed.is_empty() {
        let pos = if slice.starts_with(' ') {
            (start + 1).min(end)
        } else {
            start
        };
        pos..pos
    } else {
        let leading = slice.len() - slice.trim_start().len();
        let trailing = slice.len() - slice.trim_end().len();
        (start + leading)..(end - trailing)
    };
    RawTableCell {
        inner_range: start..end,
        content_range,
    }
}

pub fn parse_table_alignments(delim_line: &str) -> Vec<TableAlign> {
    parse_table_row_cells(delim_line)
        .into_iter()
        .map(|c| {
            let t = delim_line[c.content_range].trim();
            let left = t.starts_with(':');
            let right = t.ends_with(':') && t.len() >= 2;
            match (left, right) {
                (true, true) => TableAlign::Center,
                (false, true) => TableAlign::Right,
                _ => TableAlign::Left,
            }
        })
        .collect()
}

pub fn table_cell_at_local_byte(raw_line: &str, local_byte: usize) -> (Vec<RawTableCell>, usize) {
    let cells = parse_table_row_cells(raw_line);
    if cells.is_empty() {
        return (cells, 0);
    }
    let lb = local_byte.min(raw_line.len());
    for (ix, cell) in cells.iter().enumerate() {
        if (lb >= cell.inner_range.start && lb <= cell.inner_range.end)
            || lb < cell.inner_range.start
        {
            return (cells, ix);
        }
    }
    let last = cells.len() - 1;
    (cells, last)
}

pub fn project_table_cells(
    raw_line: &str,
    is_header: bool,
    min_cols: usize,
    cursor_on_line: Option<Range<usize>>,
) -> (Vec<ProjectedTableCell>, Option<usize>) {
    let raw_cells = parse_table_row_cells(raw_line);
    let active_col = cursor_on_line.as_ref().and_then(|c| {
        if raw_cells.is_empty() {
            None
        } else {
            let (_, ix) = table_cell_at_local_byte(raw_line, c.end);
            Some(ix)
        }
    });

    let base = InlineStyle {
        bold: is_header,
        ..Default::default()
    };

    let mut out = Vec::with_capacity(raw_cells.len().max(min_cols));
    for cell in &raw_cells {
        let mut vis_start = cell.content_range.start;
        let mut vis_end = cell.content_range.end;
        let mut cell_cursor = None;

        if let Some(ref c) = cursor_on_line
            && c.start <= cell.inner_range.end
            && c.end >= cell.inner_range.start
        {
            let ov_s = c.start.max(cell.inner_range.start);
            let ov_e = c.end.min(cell.inner_range.end);
            vis_start = clamp_char_boundary(raw_line, vis_start.min(ov_s));
            vis_end = clamp_char_boundary(raw_line, vis_end.max(ov_e));
            cell_cursor = Some((ov_s, ov_e));
        }

        let cell_slice = &raw_line[vis_start..vis_end];
        let local_cursor = cell_cursor.map(|(cs, ce)| {
            let ls = clamp_char_boundary(
                cell_slice,
                cs.saturating_sub(vis_start).min(cell_slice.len()),
            );
            let le = clamp_char_boundary(
                cell_slice,
                ce.saturating_sub(vis_start).min(cell_slice.len()),
            );
            ls..le
        });

        let mut b = ProjectionBuilder::new(cell_slice);
        emit_inline(&mut b, cell_slice.len(), base, &local_cursor);
        if b.raw_pos < cell_slice.len() {
            b.push_verbatim(cell_slice.len() - b.raw_pos, base);
        }

        out.push(ProjectedTableCell {
            inner_range: Some(cell.inner_range.clone()),
            raw_offset: vis_start,
            projected: ProjectedLine {
                display_text: b.display_text,
                spans: b.spans,
                segments: b.segments,
                links: b.links,
                prefix_visual: PrefixVisual::None,
                raw_len: cell_slice.len(),
            },
        });
    }

    while out.len() < min_cols {
        out.push(ProjectedTableCell {
            inner_range: None,
            raw_offset: raw_line.len(),
            projected: ProjectedLine {
                display_text: String::new(),
                spans: Vec::new(),
                segments: Vec::new(),
                links: Vec::new(),
                prefix_visual: PrefixVisual::None,
                raw_len: 0,
            },
        });
    }

    (out, active_col)
}

fn emit_inline(
    b: &mut ProjectionBuilder<'_>,
    end_limit: usize,
    base: InlineStyle,
    cursor: &Option<Range<usize>>,
) {
    let raw = b.raw;
    let limit = end_limit.min(raw.len());

    while b.raw_pos < limit {
        let i = b.raw_pos;
        let rem = &raw[i..limit];
        let bytes = rem.as_bytes();

        // Escaped ASCII punctuation: \* \[ \` etc.
        if bytes[0] == b'\\' && rem.len() >= 2 && bytes[1].is_ascii_punctuation() {
            if range_touched(cursor, i, i + 2) {
                b.push_verbatim(
                    1,
                    InlineStyle {
                        syntax_muted: true,
                        ..base
                    },
                );
                b.push_verbatim(1, base);
            } else {
                b.push_hidden(1);
                b.push_verbatim(1, base);
            }
            continue;
        }

        // 1. Inline code: `...`
        if bytes[0] == b'`' {
            let tick_len = bytes.iter().take_while(|&&c| c == b'`').count();
            let delim = &rem[..tick_len];
            if let Some(close_rel) = rem[tick_len..].find(delim) {
                let inner_len = close_rel;
                let tok_end = i + tick_len + inner_len + tick_len;
                let touched = range_touched(cursor, i, tok_end);
                let code_style = InlineStyle {
                    code: true,
                    monospace: true,
                    ..base
                };
                if touched {
                    b.push_verbatim(
                        tick_len,
                        InlineStyle {
                            syntax_muted: true,
                            monospace: true,
                            ..base
                        },
                    );
                    b.push_verbatim(inner_len, code_style);
                    b.push_verbatim(
                        tick_len,
                        InlineStyle {
                            syntax_muted: true,
                            monospace: true,
                            ..base
                        },
                    );
                } else {
                    b.push_hidden(tick_len);
                    b.push_verbatim(inner_len, code_style);
                    b.push_hidden(tick_len);
                }
                continue;
            }
        }

        // 2. Wikilink: [[target|alias]] or [[target]]
        if rem.starts_with("[[")
            && let Some(close_rel) = rem[2..].find("]]")
        {
            let inner = &rem[2..2 + close_rel];
            if !inner.is_empty() && !inner.contains("[[") {
                let tok_end = i + 2 + close_rel + 2;
                let touched = range_touched(cursor, i, tok_end);
                let pipe_rel = inner.rfind('|');
                let target_raw = pipe_rel.map(|p| &inner[..p]).unwrap_or(inner);
                let target = LinkTarget::Wikilink(target_raw.trim().to_string());
                let link_style = InlineStyle {
                    wikilink: true,
                    link: true,
                    ..base
                };

                if touched {
                    let disp_start = b.display_text.len();
                    b.push_verbatim(
                        2,
                        InlineStyle {
                            syntax_muted: true,
                            ..base
                        },
                    );
                    if let Some(p) = pipe_rel {
                        b.push_verbatim(
                            p + 1,
                            InlineStyle {
                                syntax_muted: true,
                                ..base
                            },
                        );
                        b.push_verbatim(inner.len() - (p + 1), link_style);
                    } else {
                        b.push_verbatim(inner.len(), link_style);
                    }
                    b.push_verbatim(
                        2,
                        InlineStyle {
                            syntax_muted: true,
                            ..base
                        },
                    );
                    let disp_end = b.display_text.len();
                    b.links.push(ProjectedLink {
                        disp_range: disp_start..disp_end,
                        raw_range: i..tok_end,
                        target,
                        concealed: false,
                    });
                } else {
                    if let Some(p) = pipe_rel {
                        b.push_hidden(2 + p + 1);
                        let disp_start = b.display_text.len();
                        b.push_verbatim(inner.len() - (p + 1), link_style);
                        let disp_end = b.display_text.len();
                        b.push_hidden(2);
                        b.links.push(ProjectedLink {
                            disp_range: disp_start..disp_end,
                            raw_range: i..tok_end,
                            target,
                            concealed: true,
                        });
                    } else {
                        b.push_hidden(2);
                        let disp_start = b.display_text.len();
                        b.push_verbatim(inner.len(), link_style);
                        let disp_end = b.display_text.len();
                        b.push_hidden(2);
                        b.links.push(ProjectedLink {
                            disp_range: disp_start..disp_end,
                            raw_range: i..tok_end,
                            target,
                            concealed: true,
                        });
                    }
                }
                continue;
            }
        }

        // 3. Inline Markdown link: [label](url)
        if bytes[0] == b'['
            && !rem.starts_with("[[")
            && let Some(mid_rel) = rem[1..].find("](")
        {
            let after_mid = &rem[1 + mid_rel + 2..];
            if let Some(url_len) = find_closing_paren(after_mid) {
                let label_len = mid_rel;
                let url_str = &after_mid[..url_len];
                let tok_end = i + 1 + label_len + 2 + url_len + 1;
                let touched = range_touched(cursor, i, tok_end);
                let link_style = InlineStyle { link: true, ..base };
                let target = LinkTarget::Url(url_str.trim().to_string());

                if touched {
                    let disp_start = b.display_text.len();
                    b.push_verbatim(
                        1,
                        InlineStyle {
                            syntax_muted: true,
                            ..base
                        },
                    );
                    b.push_verbatim(label_len, link_style);
                    b.push_verbatim(
                        2 + url_len + 1,
                        InlineStyle {
                            syntax_muted: true,
                            ..base
                        },
                    );
                    let disp_end = b.display_text.len();
                    b.links.push(ProjectedLink {
                        disp_range: disp_start..disp_end,
                        raw_range: i..tok_end,
                        target,
                        concealed: false,
                    });
                } else {
                    b.push_hidden(1);
                    let disp_start = b.display_text.len();
                    emit_inline(b, i + 1 + label_len, link_style, cursor);
                    let disp_end = b.display_text.len();
                    b.push_hidden(2 + url_len + 1);
                    b.links.push(ProjectedLink {
                        disp_range: disp_start..disp_end,
                        raw_range: i..tok_end,
                        target,
                        concealed: true,
                    });
                }
                continue;
            }
        }

        // 4. ***Bold+Italic***
        if rem.starts_with("***")
            && let Some(inner_len) = find_delim_close(rem, "***", false)
        {
            emit_delim_span(
                b,
                3,
                inner_len,
                InlineStyle {
                    bold: true,
                    italic: true,
                    ..base
                },
                cursor,
            );
            continue;
        }

        // 5. **Bold** / __Bold__
        if rem.starts_with("**")
            && let Some(inner_len) = find_delim_close(rem, "**", false)
        {
            emit_delim_span(
                b,
                2,
                inner_len,
                InlineStyle { bold: true, ..base },
                cursor,
            );
            continue;
        }
        if rem.starts_with("__")
            && left_word_ok(raw, i)
            && let Some(inner_len) = find_delim_close(rem, "__", true)
        {
            emit_delim_span(
                b,
                2,
                inner_len,
                InlineStyle { bold: true, ..base },
                cursor,
            );
            continue;
        }

        // 6. *Italic* / _Italic_
        if rem.starts_with('*')
            && !rem.starts_with("**")
            && let Some(inner_len) = find_delim_close(rem, "*", false)
        {
            emit_delim_span(
                b,
                1,
                inner_len,
                InlineStyle {
                    italic: true,
                    ..base
                },
                cursor,
            );
            continue;
        }
        if rem.starts_with('_')
            && !rem.starts_with("__")
            && left_word_ok(raw, i)
            && let Some(inner_len) = find_delim_close(rem, "_", true)
        {
            emit_delim_span(
                b,
                1,
                inner_len,
                InlineStyle {
                    italic: true,
                    ..base
                },
                cursor,
            );
            continue;
        }

        // 7. ~~Strikethrough~~
        if rem.starts_with("~~")
            && let Some(inner_len) = find_delim_close(rem, "~~", false)
        {
            emit_delim_span(
                b,
                2,
                inner_len,
                InlineStyle {
                    strikethrough: true,
                    ..base
                },
                cursor,
            );
            continue;
        }

        // 8. ==Highlight==
        if rem.starts_with("==")
            && let Some(inner_len) = find_delim_close(rem, "==", false)
        {
            emit_delim_span(
                b,
                2,
                inner_len,
                InlineStyle {
                    highlight: true,
                    ..base
                },
                cursor,
            );
            continue;
        }

        // Plain character run up to next interesting ASCII marker.
        let ch = rem.chars().next().unwrap();
        let mut step = ch.len_utf8();
        while i + step < limit {
            let b0 = raw.as_bytes()[i + step];
            if matches!(
                b0,
                b'\\' | b'`' | b'[' | b'*' | b'_' | b'~' | b'='
            ) {
                break;
            }
            let next_ch = raw[i + step..limit].chars().next().unwrap();
            step += next_ch.len_utf8();
        }
        b.push_verbatim(step, base);
    }
}

fn emit_delim_span(
    b: &mut ProjectionBuilder<'_>,
    delim_len: usize,
    inner_len: usize,
    inner_style: InlineStyle,
    cursor: &Option<Range<usize>>,
) {
    let start = b.raw_pos;
    let tok_end = start + delim_len + inner_len + delim_len;
    let touched = range_touched(cursor, start, tok_end);
    if touched {
        b.push_verbatim(
            delim_len,
            InlineStyle {
                syntax_muted: true,
                ..inner_style
            },
        );
        emit_inline(b, start + delim_len + inner_len, inner_style, cursor);
        b.push_verbatim(
            delim_len,
            InlineStyle {
                syntax_muted: true,
                ..inner_style
            },
        );
    } else {
        b.push_hidden(delim_len);
        emit_inline(b, start + delim_len + inner_len, inner_style, cursor);
        b.push_hidden(delim_len);
    }
}

fn find_closing_paren(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return Some(idx);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

fn left_word_ok(raw: &str, idx: usize) -> bool {
    raw[..idx]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric())
}

fn right_word_ok(s: &str, after_idx: usize) -> bool {
    s[after_idx..]
        .chars()
        .next()
        .is_none_or(|c| !c.is_alphanumeric())
}

fn find_delim_close(rem: &str, delim: &str, word_guard: bool) -> Option<usize> {
    let dlen = delim.len();
    let after = &rem[dlen..];
    if after.is_empty() || after.starts_with(char::is_whitespace) {
        return None;
    }
    let mut search_from = 0usize;
    while let Some(rel) = after[search_from..].find(delim) {
        let close_start = search_from + rel;
        if close_start == 0 {
            search_from = close_start + dlen;
            continue;
        }
        let inner = &after[..close_start];
        if inner.chars().next_back().is_some_and(char::is_whitespace) {
            search_from = close_start + 1;
            continue;
        }
        let after_close = close_start + dlen;
        if delim == "*" && after[after_close..].starts_with('*') {
            search_from = close_start + 1;
            continue;
        }
        if delim == "_" && after[after_close..].starts_with('_') {
            search_from = close_start + 1;
            continue;
        }
        if word_guard && !right_word_ok(after, after_close) {
            search_from = close_start + 1;
            continue;
        }
        return Some(close_start);
    }
    None
}

fn parse_fence_open(trimmed: &str) -> Option<(char, usize, Option<String>)> {
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == first).count();
    if count < 3 {
        return None;
    }
    let info = trimmed[count..].trim();
    if first == '`' && info.contains('`') {
        return None;
    }
    let lang = if info.is_empty() {
        None
    } else {
        info.split_whitespace().next().map(str::to_string)
    };
    Some((first, count, lang))
}

fn is_fence_close(trimmed: &str, fence_ch: char, min_len: usize) -> bool {
    let count = trimmed.chars().take_while(|&c| c == fence_ch).count();
    count >= min_len && trimmed[count..].trim().is_empty()
}

fn is_delim_cell(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    let inner = t.strip_prefix(':').unwrap_or(t);
    let inner = inner.strip_suffix(':').unwrap_or(inner);
    !inner.is_empty() && inner.bytes().all(|b| b == b'-')
}

fn is_table_delimiter(header: &str, line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || !t.contains('-') {
        return false;
    }
    if !t.contains('|') && parse_table_row_cells(header).len() > 1 {
        return false;
    }
    let cells = parse_table_row_cells(line);
    !cells.is_empty() && cells.iter().all(|c| is_delim_cell(&line[c.inner_range.clone()]))
}

fn collect_top_level_pipes(s: &str) -> Vec<usize> {
    let bytes = s.as_bytes();
    let mut pipes = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 1;
            if let Some(ch) = s[i..].chars().next() {
                i += ch.len_utf8();
            }
        } else if bytes[i] == b'`' {
            let tick_len = bytes[i..].iter().take_while(|&&b| b == b'`').count();
            let delim = &s[i..i + tick_len];
            if let Some(close_rel) = s[i + tick_len..].find(delim) {
                i += tick_len + close_rel + tick_len;
            } else {
                i += tick_len;
            }
        } else if bytes[i] >= 0x80 {
            if let Some(ch) = s[i..].chars().next() {
                i += ch.len_utf8();
            } else {
                i += 1;
            }
        } else if s[i..].starts_with("[[")
            && let Some(close) = s[i + 2..].find("]]")
        {
            i += 2 + close + 2;
        } else if bytes[i] == b'|' {
            pipes.push(i);
            i += 1;
        } else {
            i += 1;
        }
    }
    pipes
}

fn has_table_pipe(s: &str) -> bool {
    !collect_top_level_pipes(s).is_empty()
}

fn is_table_continuation_line(line: &str, is_mdx: bool) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || parse_fence_open(trimmed).is_some()
        || trimmed.starts_with('>')
        || parse_atx_heading_with_len(trimmed).is_some()
        || is_scene_break(trimmed)
        || parse_image_embed(trimmed).is_some()
        || parse_list_or_task_prefix(line).is_some()
        || (is_mdx && starts_mdx_block(trimmed))
    {
        return false;
    }
    has_table_pipe(trimmed)
}

fn parse_atx_heading_with_len(trimmed: &str) -> Option<(u8, usize)> {
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let after = &trimmed[hashes..];
    let ws = after.bytes().take_while(|b| matches!(b, b' ' | b'\t')).count();
    if ws == 0 {
        return None;
    }
    Some((hashes as u8, hashes + ws))
}

fn is_scene_break(trimmed: &str) -> bool {
    let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() < 3 {
        return false;
    }
    let mut chars = compact.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    matches!(first, '*' | '-' | '_') && chars.all(|c| c == first)
}

fn parse_image_embed(trimmed: &str) -> Option<(String, String)> {
    if let Some(inner) = trimmed.strip_prefix("![[").and_then(|s| s.strip_suffix("]]"))
        && !inner.is_empty()
        && !inner.contains("[[")
        && !inner.contains("]]")
    {
        let (target, alt) = inner.split_once('|').unwrap_or((inner, ""));
        return Some((target.trim().to_string(), alt.trim().to_string()));
    }
    if let Some(after) = trimmed.strip_prefix("![")
        && let Some(close_bracket) = after.find("](")
    {
        let alt = &after[..close_bracket];
        let after_paren = &after[close_bracket + 2..];
        if let Some(close_paren) = after_paren.rfind(')')
            && close_paren == after_paren.len() - 1
        {
            return Some((
                after_paren[..close_paren].trim().to_string(),
                alt.trim().to_string(),
            ));
        }
    }
    None
}

/// Rewrite a raw Obsidian `![[target|...]]` or standard `![alt](url)` image line
/// using the updated `StyledImageSpec`, preserving indentation and target path.
pub fn rewrite_image_embed_line(raw_line: &str, spec: &StyledImageSpec) -> Option<String> {
    let ws_len = raw_line.len() - raw_line.trim_start().len();
    let indent = &raw_line[..ws_len];
    let trimmed = raw_line.trim();
    let formatted = format_image_spec(spec);

    if let Some(inner) = trimmed.strip_prefix("![[").and_then(|s| s.strip_suffix("]]"))
        && !inner.is_empty()
        && !inner.contains("[[")
        && !inner.contains("]]")
    {
        let target = inner
            .split_once('|')
            .map(|(t, _)| t.trim())
            .unwrap_or_else(|| inner.trim());
        if formatted.is_empty() {
            return Some(format!("{indent}![[{target}]]"));
        }
        return Some(format!("{indent}![[{target}|{formatted}]]"));
    }

    if let Some(after) = trimmed.strip_prefix("![")
        && let Some(close_bracket) = after.find("](")
    {
        let after_paren = &after[close_bracket + 2..];
        if let Some(close_paren) = after_paren.rfind(')')
            && close_paren == after_paren.len() - 1
        {
            let url = after_paren[..close_paren].trim();
            return Some(format!("{indent}![{formatted}]({url})"));
        }
    }

    None
}

fn parse_jsx_tag_name(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('<')?;
    if rest.starts_with('/') || rest.starts_with('!') || rest.starts_with('?') {
        return None;
    }
    let name_len = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(*b, b'_' | b'-' | b'.'))
        .count();
    if name_len == 0 {
        return None;
    }
    let name = &rest[..name_len];
    let next_ok = matches!(
        rest.as_bytes().get(name_len),
        None | Some(b' ' | b'\t' | b'/' | b'>')
    );
    if !next_ok {
        return None;
    }
    let first = name.as_bytes()[0];
    if first.is_ascii_uppercase()
        || name.eq_ignore_ascii_case("video")
        || name.eq_ignore_ascii_case("iframe")
    {
        Some(name)
    } else {
        None
    }
}

fn starts_mdx_block(trimmed: &str) -> bool {
    trimmed.starts_with("import ")
        || trimmed.starts_with("export ")
        || parse_jsx_tag_name(trimmed).is_some()
}

fn find_mdx_block_end(lines: &[&str], start: usize) -> usize {
    let trimmed = lines[start].trim();
    if trimmed.starts_with("import ") || trimmed.starts_with("export ") {
        return start;
    }
    let Some(tag) = parse_jsx_tag_name(trimmed) else {
        return start;
    };
    let closing = format!("</{tag}>");
    if trimmed.ends_with("/>") || trimmed.contains(&closing) {
        return start;
    }
    let limit = (start + 40).min(lines.len() - 1);
    let mut saw_blank = false;
    for j in (start + 1)..=limit {
        let t = lines[j].trim();
        if t.is_empty() {
            saw_blank = true;
            continue;
        }
        if saw_blank && parse_atx_heading_with_len(t).is_some() {
            return start;
        }
        if t.ends_with("/>") || t.contains(&closing) {
            return j;
        }
    }
    start
}

struct ListOrTaskPrefix {
    prefix_len: usize,
    ordered: bool,
    marker_display: String,
    checked: bool,
    checkbox_byte: Option<usize>,
    next_prefix: String,
}

fn parse_list_or_task_prefix(line: &str) -> Option<ListOrTaskPrefix> {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];
    let bytes = rest.as_bytes();

    if bytes.len() >= 2 && matches!(bytes[0], b'-' | b'*' | b'+') && bytes[1] == b' ' {
        let marker = &rest[..1];
        let after_marker = &rest[2..];
        if let Some((task_len, checked)) = parse_task_marker(after_marker) {
            return Some(ListOrTaskPrefix {
                prefix_len: indent_len + 2 + task_len,
                ordered: false,
                marker_display: marker.to_string(),
                checked,
                checkbox_byte: Some(indent_len + 3),
                next_prefix: format!("{indent}{marker} [ ] "),
            });
        }
        return Some(ListOrTaskPrefix {
            prefix_len: indent_len + 2,
            ordered: false,
            marker_display: marker.to_string(),
            checked: false,
            checkbox_byte: None,
            next_prefix: format!("{indent}{marker} "),
        });
    }

    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0
        && digits + 1 < bytes.len()
        && matches!(bytes[digits], b'.' | b')')
        && bytes[digits + 1] == b' '
    {
        let num: usize = rest[..digits].parse().unwrap_or(1);
        let delim = bytes[digits] as char;
        let base_len = digits + 2;
        let after_marker = &rest[base_len..];
        if let Some((task_len, checked)) = parse_task_marker(after_marker) {
            return Some(ListOrTaskPrefix {
                prefix_len: indent_len + base_len + task_len,
                ordered: true,
                marker_display: rest[..base_len].to_string(),
                checked,
                checkbox_byte: Some(indent_len + base_len + 1),
                next_prefix: format!("{indent}{}{delim} [ ] ", num + 1),
            });
        }
        return Some(ListOrTaskPrefix {
            prefix_len: indent_len + base_len,
            ordered: true,
            marker_display: rest[..base_len].to_string(),
            checked: false,
            checkbox_byte: None,
            next_prefix: format!("{indent}{}{delim} ", num + 1),
        });
    }

    None
}

fn parse_task_marker(after_marker: &str) -> Option<(usize, bool)> {
    let bytes = after_marker.as_bytes();
    if bytes.len() >= 3
        && bytes[0] == b'['
        && matches!(bytes[1], b' ' | b'x' | b'X')
        && bytes[2] == b']'
    {
        let checked = matches!(bytes[1], b'x' | b'X');
        match bytes.get(3) {
            Some(b' ') => Some((4, checked)),
            None => Some((3, checked)),
            _ => None,
        }
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    InsertChar,
    Delete,
    Atomic,
    Ime,
}

const MAX_UNDO_SNAPSHOTS: usize = 250;
const MAX_UNDO_BYTES: usize = 6 * 1024 * 1024;

#[derive(Debug, Clone)]
struct UndoSnapshot {
    text: String,
    anchor: usize,
    head: usize,
}

/// Unified Markdown document buffer with continuous cursor/selection, undo/redo coalescing, and smart edits.
pub struct LiveDocBuffer {
    pub text: String,
    pub lines: Vec<DocLine>,
    pub is_mdx: bool,
    pub version: u64,
    pub anchor: usize,
    pub head: usize,
    pub marked_range: Option<Range<usize>>,
    undo_stack: Vec<UndoSnapshot>,
    undo_bytes: usize,
    redo_stack: Vec<UndoSnapshot>,
    last_edit: Option<(EditKind, Instant)>,
}

impl LiveDocBuffer {
    pub fn new(raw: String, is_mdx: bool) -> Self {
        let lines = classify_document_lines(&raw, is_mdx);
        Self {
            text: raw,
            lines,
            is_mdx,
            version: 1,
            anchor: 0,
            head: 0,
            marked_range: None,
            undo_stack: Vec::new(),
            undo_bytes: 0,
            redo_stack: Vec::new(),
            last_edit: None,
        }
    }

    pub fn selection_range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn has_selection(&self) -> bool {
        self.anchor != self.head
    }

    pub fn selected_text(&self) -> &str {
        let r = self.selection_range();
        &self.text[r]
    }

    pub fn set_cursor(&mut self, byte: usize, extend: bool) {
        let pos = clamp_char_boundary(&self.text, byte);
        self.head = pos;
        if !extend {
            self.anchor = pos;
        }
    }

    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        self.anchor = clamp_char_boundary(&self.text, anchor);
        self.head = clamp_char_boundary(&self.text, head);
    }

    pub fn line_index_for_byte(&self, byte: usize) -> usize {
        let b = byte.min(self.text.len());
        match self.lines.binary_search_by(|l| {
            if b < l.byte_start {
                std::cmp::Ordering::Greater
            } else if b > l.byte_end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(ix) => ix,
            Err(ix) => ix.min(self.lines.len().saturating_sub(1)),
        }
    }

    pub fn line_str(&self, line_ix: usize) -> &str {
        if let Some(l) = self.lines.get(line_ix) {
            &self.text[l.byte_start..l.byte_end]
        } else {
            ""
        }
    }

    pub fn cursor_line_col(&self) -> (usize, usize) {
        let li = self.line_index_for_byte(self.head);
        let col_bytes = self
            .head
            .saturating_sub(self.lines.get(li).map_or(0, |l| l.byte_start));
        let col_chars = self.line_str(li)[..col_bytes.min(self.line_str(li).len())]
            .chars()
            .count();
        (li, col_chars)
    }

    pub fn byte_from_line_col(&self, line_0based: usize, col_chars: usize) -> usize {
        let li = line_0based.min(self.lines.len().saturating_sub(1));
        let Some(dl) = self.lines.get(li) else {
            return 0;
        };
        let slice = &self.text[dl.byte_start..dl.byte_end];
        let rel = slice
            .char_indices()
            .nth(col_chars)
            .map(|(idx, _)| idx)
            .unwrap_or(slice.len());
        dl.byte_start + rel
    }

    fn push_undo_snapshot(&mut self, snap: UndoSnapshot) {
        let snap_bytes = snap.text.len();
        while self.undo_stack.len() >= MAX_UNDO_SNAPSHOTS
            || (self.undo_stack.len() >= 2 && self.undo_bytes + snap_bytes > MAX_UNDO_BYTES)
        {
            let evicted = self.undo_stack.remove(0);
            self.undo_bytes = self.undo_bytes.saturating_sub(evicted.text.len());
        }
        self.undo_bytes += snap_bytes;
        self.undo_stack.push(snap);
    }

    fn record_undo(&mut self, kind: EditKind) {
        let now = Instant::now();
        let coalesce = match (kind, self.last_edit) {
            (EditKind::InsertChar, Some((EditKind::InsertChar, prev)))
            | (EditKind::Delete, Some((EditKind::Delete, prev))) => {
                now.duration_since(prev) < Duration::from_millis(650)
            }
            (EditKind::Ime, Some((EditKind::Ime, _))) => true,
            _ => false,
        };
        if !coalesce || self.undo_stack.is_empty() {
            self.push_undo_snapshot(UndoSnapshot {
                text: self.text.clone(),
                anchor: self.anchor,
                head: self.head,
            });
        }
        self.redo_stack.clear();
        self.last_edit = Some((kind, now));
    }

    pub fn break_undo_coalescing(&mut self) {
        self.last_edit = None;
    }

    pub fn reset_text(&mut self, raw: String, line_0based: usize, col_chars: usize) {
        self.text = raw;
        self.lines = classify_document_lines(&self.text, self.is_mdx);
        self.version = self.version.wrapping_add(1);
        self.marked_range = None;
        self.undo_stack.clear();
        self.undo_bytes = 0;
        self.redo_stack.clear();
        self.last_edit = None;
        let b = self.byte_from_line_col(line_0based, col_chars);
        self.anchor = b;
        self.head = b;
    }

    pub fn replace_range(&mut self, range: Range<usize>, replacement: &str, kind: EditKind) {
        let start = clamp_char_boundary(&self.text, range.start.min(range.end));
        let end = clamp_char_boundary(&self.text, range.start.max(range.end));
        self.record_undo(kind);
        self.text.replace_range(start..end, replacement);
        let new_caret = start + replacement.len();
        self.anchor = new_caret;
        self.head = new_caret;
        self.lines = classify_document_lines(&self.text, self.is_mdx);
        self.version = self.version.wrapping_add(1);
    }

    /// Rewrite an image embed line with a new `StyledImageSpec` as an atomic undoable edit,
    /// keeping selection/cursor on unrelated lines undisturbed.
    pub fn update_image_embed_spec(&mut self, line_ix: usize, spec: &StyledImageSpec) -> bool {
        let Some(dl) = self.lines.get(line_ix) else {
            return false;
        };
        let start = dl.byte_start;
        let end = dl.byte_end;
        let raw_line = &self.text[start..end];
        let Some(rewritten) = rewrite_image_embed_line(raw_line, spec) else {
            return false;
        };
        if rewritten == raw_line {
            return false;
        }

        let prev_anchor = self.anchor;
        let prev_head = self.head;
        let delta = rewritten.len() as isize - (end - start) as isize;
        let new_line_end = start + rewritten.len();

        self.replace_range(start..end, &rewritten, EditKind::Atomic);

        let remap_pos = |pos: usize| -> usize {
            if pos <= start {
                pos
            } else if pos >= end {
                ((pos as isize + delta).max(0) as usize).min(self.text.len())
            } else {
                new_line_end
            }
        };
        self.anchor = clamp_char_boundary(&self.text, remap_pos(prev_anchor));
        self.head = clamp_char_boundary(&self.text, remap_pos(prev_head));
        true
    }

    /// Delete an entire line (including its line break) as an atomic undoable edit.
    pub fn delete_line(&mut self, line_ix: usize) -> bool {
        let Some(dl) = self.lines.get(line_ix) else {
            return false;
        };
        let mut start = dl.byte_start;
        let mut end = dl.byte_end;
        if self.text.as_bytes().get(end) == Some(&b'\n') {
            end += 1;
        } else if start > 0 && self.text.as_bytes().get(start - 1) == Some(&b'\n') {
            start -= 1;
        }
        self.replace_range(start..end, "", EditKind::Atomic);
        true
    }

    pub fn insert_text(&mut self, s: &str) {
        if self.has_selection() {
            let wrap = match s {
                "*" => Some(("*", "*")),
                "_" => Some(("_", "_")),
                "`" => Some(("`", "`")),
                "~" => Some(("~", "~")),
                "=" => Some(("=", "=")),
                "[" => Some(("[", "]")),
                "(" => Some(("(", ")")),
                "\"" => Some(("\"", "\"")),
                _ => None,
            };
            if let Some((open, close)) = wrap {
                let sel = self.selection_range();
                let inner = self.text[sel.clone()].to_string();
                let wrapped = format!("{open}{inner}{close}");
                let inner_start = sel.start + open.len();
                let inner_end = inner_start + inner.len();
                self.replace_range(sel, &wrapped, EditKind::Atomic);
                self.anchor = inner_start;
                self.head = inner_end;
                return;
            }
        }
        let kind = if s.chars().count() == 1 && !s.contains('\n') {
            EditKind::InsertChar
        } else {
            EditKind::Atomic
        };
        let sel = self.selection_range();
        self.replace_range(sel, s, kind);
    }

    pub fn smart_enter(&mut self) {
        if self.has_selection() {
            let sel = self.selection_range();
            self.replace_range(sel, "\n", EditKind::Atomic);
            return;
        }
        let li = self.line_index_for_byte(self.head);
        let Some(dl) = self.lines.get(li).cloned() else {
            self.replace_range(self.head..self.head, "\n", EditKind::Atomic);
            return;
        };
        let line_text = &self.text[dl.byte_start..dl.byte_end];
        let local_pos = self.head.saturating_sub(dl.byte_start).min(line_text.len());
        let before = &line_text[..local_pos];
        let after = &line_text[local_pos..];

        // Handle Markdown GFM table rows.
        if let LineBlockContext::Table {
            is_header,
            is_delim,
            table_start_line,
            table_end_line,
            ..
        } = dl.ctx
        {
            if !is_header && !is_delim && li == table_end_line {
                let raw = self.line_str(li);
                let all_empty = parse_table_row_cells(raw)
                    .iter()
                    .all(|c| raw[c.content_range.clone()].is_empty());
                if all_empty {
                    // Replace empty trailing row with a blank line below the table.
                    self.replace_range(dl.byte_start..dl.byte_end, "", EditKind::Atomic);
                    return;
                }
            }
            if is_delim {
                self.table_insert_row_below(table_start_line);
            } else {
                self.table_insert_row_below(li);
            }
            return;
        }

        // Auto-expand opening code fence if Enter pressed at end of ```lang line.
        if after.trim().is_empty()
            && let Some((fence_ch, fence_len, _)) = parse_fence_open(before.trim())
            && !matches!(
                dl.ctx,
                LineBlockContext::CodeFence { is_open: false, .. }
            )
        {
            let indent_len = before.len() - before.trim_start().len();
            let indent = &before[..indent_len];
            let closing: String = std::iter::repeat_n(fence_ch, fence_len).collect();
            let inserted = format!("\n\n{indent}{closing}");
            let caret = self.head + 1;
            self.replace_range(self.head..self.head, &inserted, EditKind::Atomic);
            self.set_cursor(caret, false);
            return;
        }

        // Continue or terminate list / task prefix.
        if let Some(prefix) = parse_list_or_task_prefix(line_text) {
            let remainder = line_text[prefix.prefix_len..].trim();
            if remainder.is_empty() && after.trim().is_empty() && local_pos >= prefix.prefix_len {
                self.replace_range(dl.byte_start..dl.byte_end, "", EditKind::Atomic);
                return;
            }
            if local_pos >= prefix.prefix_len {
                let inserted = format!("\n{}", prefix.next_prefix);
                self.replace_range(self.head..self.head, &inserted, EditKind::Atomic);
                return;
            }
        }

        // Continue blockquote if line starts with > and cursor is after quote prefix.
        if line_text.trim_start().starts_with('>') {
            let (_, qlen) = parse_quote_prefix(line_text);
            let remainder = line_text[qlen..].trim();
            if remainder.is_empty() && after.trim().is_empty() && local_pos >= qlen {
                self.replace_range(dl.byte_start..dl.byte_end, "", EditKind::Atomic);
                return;
            }
            if local_pos >= qlen {
                let qprefix = &line_text[..qlen];
                let inserted = format!("\n{qprefix}");
                self.replace_range(self.head..self.head, &inserted, EditKind::Atomic);
                return;
            }
        }

        self.replace_range(self.head..self.head, "\n", EditKind::Atomic);
    }

    pub fn backspace(&mut self, by_word: bool) {
        if self.has_selection() {
            let sel = self.selection_range();
            self.replace_range(sel, "", EditKind::Delete);
            return;
        }
        if self.head == 0 {
            return;
        }
        let li = self.line_index_for_byte(self.head);
        if let Some(dl) = self.lines.get(li).cloned()
            && let LineBlockContext::Table {
                is_header,
                is_delim: false,
                table_end_line,
                ..
            } = dl.ctx
        {
            let raw = self.line_str(li);
            let local = self.head.saturating_sub(dl.byte_start).min(raw.len());
            let (cells, col_ix) = table_cell_at_local_byte(raw, local);
            if let Some(cell) = cells.get(col_ix) {
                if local > cell.inner_range.end {
                    self.set_cursor(dl.byte_start + cell.content_range.end, false);
                    return;
                }
                let min_byte = dl.byte_start + cell.content_range.start;
                if self.head <= min_byte {
                    if !is_header
                        && li == table_end_line
                        && col_ix == 0
                        && cells.iter().all(|c| raw[c.content_range.clone()].is_empty())
                    {
                        self.table_delete_row(li);
                    }
                    return;
                }
                let mut target = if by_word {
                    prev_word_boundary(&self.text, self.head)
                } else {
                    prev_char_boundary(&self.text, self.head)
                };
                if target < min_byte {
                    target = min_byte;
                }
                if target < self.head {
                    self.replace_range(target..self.head, "", EditKind::Delete);
                }
                return;
            }
        }
        let target = if by_word {
            prev_word_boundary(&self.text, self.head)
        } else {
            let mut prev = prev_char_boundary(&self.text, self.head);
            if self.text.as_bytes()[prev] == b'\n'
                && prev > 0
                && self.text.as_bytes()[prev - 1] == b'\r'
            {
                prev -= 1;
            }
            prev
        };
        self.replace_range(target..self.head, "", EditKind::Delete);
    }

    pub fn delete(&mut self, by_word: bool) {
        if self.has_selection() {
            let sel = self.selection_range();
            self.replace_range(sel, "", EditKind::Delete);
            return;
        }
        if self.head >= self.text.len() {
            return;
        }
        let li = self.line_index_for_byte(self.head);
        if let Some(dl) = self.lines.get(li).cloned()
            && let LineBlockContext::Table {
                is_delim: false, ..
            } = dl.ctx
        {
            let raw = self.line_str(li);
            let local = self.head.saturating_sub(dl.byte_start).min(raw.len());
            let (cells, col_ix) = table_cell_at_local_byte(raw, local);
            if let Some(cell) = cells.get(col_ix) {
                let trailing_pad = usize::from(
                    raw.as_bytes().get(cell.inner_range.end) == Some(&b'|')
                        && cell.inner_range.end > cell.content_range.end
                        && raw.as_bytes().get(cell.inner_range.end - 1) == Some(&b' '),
                );
                let max_local = cell
                    .content_range
                    .end
                    .max(cell.inner_range.end.saturating_sub(trailing_pad));
                let max_byte = dl.byte_start + max_local;
                if self.head >= max_byte {
                    return;
                }
                let mut target = if by_word {
                    next_word_boundary(&self.text, self.head)
                } else {
                    next_char_boundary(&self.text, self.head)
                };
                if target > max_byte {
                    target = max_byte;
                }
                if target > self.head {
                    self.replace_range(self.head..target, "", EditKind::Delete);
                }
                return;
            }
        }
        let target = if by_word {
            next_word_boundary(&self.text, self.head)
        } else {
            let mut next = next_char_boundary(&self.text, self.head);
            if self.text.as_bytes().get(self.head) == Some(&b'\r')
                && self.text.as_bytes().get(next) == Some(&b'\n')
            {
                next += 1;
            }
            next
        };
        self.replace_range(self.head..target, "", EditKind::Delete);
    }

    pub fn indent_lines(&mut self, outdent: bool) {
        let sel = self.selection_range();
        let first_li = self.line_index_for_byte(sel.start);
        let last_li = if sel.end > sel.start
            && self
                .lines
                .get(self.line_index_for_byte(sel.end))
                .is_some_and(|l| l.byte_start == sel.end)
        {
            self.line_index_for_byte(sel.end).saturating_sub(1)
        } else {
            self.line_index_for_byte(sel.end)
        }
        .max(first_li);

        if first_li == last_li && !self.has_selection() {
            let line = self.line_str(first_li);
            let line_start = self.lines[first_li].byte_start;
            let prev_head = self.head;
            let leading_remove = if line.starts_with('\t') {
                1
            } else if line.starts_with("  ") {
                2
            } else if line.starts_with(' ') {
                1
            } else {
                0
            };

            let is_list_or_task = matches!(
                self.lines[first_li].ctx,
                LineBlockContext::ListItem { .. } | LineBlockContext::TaskItem { .. }
            );

            if is_list_or_task {
                if outdent {
                    if leading_remove > 0 {
                        self.replace_range(
                            line_start..line_start + leading_remove,
                            "",
                            EditKind::Atomic,
                        );
                        let target = prev_head.saturating_sub(leading_remove).max(line_start);
                        self.set_cursor(target, false);
                    }
                } else {
                    self.replace_range(line_start..line_start, "  ", EditKind::Atomic);
                    let target = (prev_head + 2).min(self.text.len());
                    self.set_cursor(target, false);
                }
                return;
            }

            if !outdent {
                self.replace_range(self.head..self.head, "  ", EditKind::Atomic);
            } else if leading_remove > 0 {
                self.replace_range(
                    line_start..line_start + leading_remove,
                    "",
                    EditKind::Atomic,
                );
                let target = prev_head.saturating_sub(leading_remove).max(line_start);
                self.set_cursor(target, false);
            } else if prev_head > line_start {
                let before = &self.text[line_start..prev_head];
                let inline_remove = if before.ends_with('\t') {
                    1
                } else if before.ends_with("  ") {
                    2
                } else if before.ends_with(' ') {
                    1
                } else {
                    0
                };
                if inline_remove > 0 {
                    self.replace_range(
                        prev_head - inline_remove..prev_head,
                        "",
                        EditKind::Atomic,
                    );
                }
            }
            return;
        }

        let block_start = self.lines[first_li].byte_start;
        let block_end = self.lines[last_li].byte_end;
        let mut rebuilt = String::new();
        for ix in first_li..=last_li {
            if ix > first_li {
                rebuilt.push('\n');
            }
            let line = self.line_str(ix);
            if outdent {
                if let Some(rest) = line.strip_prefix('\t') {
                    rebuilt.push_str(rest);
                } else if let Some(rest) = line.strip_prefix("  ") {
                    rebuilt.push_str(rest);
                } else if let Some(rest) = line.strip_prefix(' ') {
                    rebuilt.push_str(rest);
                } else {
                    rebuilt.push_str(line);
                }
            } else {
                rebuilt.push_str("  ");
                rebuilt.push_str(line);
            }
        }
        if rebuilt == self.text[block_start..block_end] {
            return;
        }
        let end_after = block_start + rebuilt.len();
        self.replace_range(block_start..block_end, &rebuilt, EditKind::Atomic);
        if first_li == last_li && outdent && !rebuilt.is_empty() {
            let line_end = self.lines[first_li].byte_end;
            self.set_cursor(line_end, false);
        } else {
            self.set_selection(block_start, end_after);
        }
    }

    pub fn toggle_wrap(&mut self, marker: &str) {
        let sel = self.selection_range();
        let mlen = marker.len();
        if sel.start >= mlen
            && sel.end + mlen <= self.text.len()
            && &self.text[sel.start - mlen..sel.start] == marker
            && &self.text[sel.end..sel.end + mlen] == marker
        {
            let inner = self.text[sel.clone()].to_string();
            let new_start = sel.start - mlen;
            let new_end = new_start + inner.len();
            self.replace_range(sel.start - mlen..sel.end + mlen, &inner, EditKind::Atomic);
            self.set_selection(new_start, new_end);
        } else if self.has_selection() {
            let inner = self.text[sel.clone()].to_string();
            let wrapped = format!("{marker}{inner}{marker}");
            let new_start = sel.start + mlen;
            let new_end = new_start + inner.len();
            self.replace_range(sel, &wrapped, EditKind::Atomic);
            self.set_selection(new_start, new_end);
        } else {
            let paired = format!("{marker}{marker}");
            let caret = self.head + mlen;
            self.replace_range(self.head..self.head, &paired, EditKind::Atomic);
            self.set_cursor(caret, false);
        }
    }

    pub fn insert_or_wrap_link(&mut self) {
        let sel = self.selection_range();
        if self.has_selection() {
            let label = self.text[sel.clone()].to_string();
            let formatted = format!("[{label}](url)");
            let url_start = sel.start + 1 + label.len() + 2;
            let url_end = url_start + 3;
            self.replace_range(sel, &formatted, EditKind::Atomic);
            self.set_selection(url_start, url_end);
        } else {
            let template = "[](url)";
            let caret = self.head + 1;
            self.replace_range(self.head..self.head, template, EditKind::Atomic);
            self.set_cursor(caret, false);
        }
    }

    pub fn toggle_task_on_line(&mut self, line_ix: usize) -> bool {
        let Some(dl) = self.lines.get(line_ix).cloned() else {
            return false;
        };
        let LineBlockContext::TaskItem {
            checked,
            checkbox_byte,
            ..
        } = dl.ctx
        else {
            return false;
        };
        let abs = dl.byte_start + checkbox_byte;
        if abs >= dl.byte_end {
            return false;
        }
        let saved_anchor = self.anchor;
        let saved_head = self.head;
        let replacement = if checked { " " } else { "x" };
        self.replace_range(abs..abs + 1, replacement, EditKind::Atomic);
        self.anchor = clamp_char_boundary(&self.text, saved_anchor);
        self.head = clamp_char_boundary(&self.text, saved_head);
        true
    }

    pub fn apply_mobile_format(&mut self, action: MarkdownFormatAction) -> bool {
        match action {
            MarkdownFormatAction::Bold => {
                self.toggle_wrap("**");
                true
            }
            MarkdownFormatAction::Italic => {
                self.toggle_wrap("*");
                true
            }
            MarkdownFormatAction::Code => {
                self.toggle_wrap("`");
                true
            }
            MarkdownFormatAction::Heading => self.cycle_heading_prefix(),
            MarkdownFormatAction::Checkbox => self.toggle_line_prefix("- [ ] "),
            MarkdownFormatAction::Bullet => self.toggle_line_prefix("- "),
            MarkdownFormatAction::Quote => self.toggle_line_prefix("> "),
            MarkdownFormatAction::Wikilink => {
                let sel = self.selection_range();
                if self.has_selection() {
                    let inner = self.text[sel.clone()].to_string();
                    let wrapped = format!("[[{inner}]]");
                    let start = sel.start + 2;
                    let end = start + inner.len();
                    self.replace_range(sel, &wrapped, EditKind::Atomic);
                    self.set_selection(start, end);
                } else {
                    let caret = self.head + 2;
                    self.replace_range(self.head..self.head, "[[]]", EditKind::Atomic);
                    self.set_cursor(caret, false);
                }
                true
            }
            MarkdownFormatAction::HorizontalRule => {
                self.insert_text("\n---\n");
                true
            }
        }
    }

    fn cycle_heading_prefix(&mut self) -> bool {
        let li = self.line_index_for_byte(self.head);
        let Some(dl) = self.lines.get(li).cloned() else {
            return false;
        };
        let raw_line = &self.text[dl.byte_start..dl.byte_end];
        let trimmed = raw_line.trim_start();
        let ws_len = raw_line.len() - trimmed.len();
        let existing_hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
        let has_heading = existing_hashes > 0 && trimmed[existing_hashes..].starts_with(' ');

        let caret_local = self.head.saturating_sub(dl.byte_start);
        if has_heading {
            let prefix_end = dl.byte_start + ws_len + existing_hashes + 1;
            if existing_hashes < 3 {
                // Promote # -> ## or ## -> ###
                self.replace_range(
                    dl.byte_start + ws_len..dl.byte_start + ws_len,
                    "#",
                    EditKind::Atomic,
                );
                let new_head = (dl.byte_start + caret_local + 1).min(self.text.len());
                self.set_cursor(clamp_char_boundary(&self.text, new_head), false);
            } else {
                // Cycle off after ###
                let del_len = prefix_end - (dl.byte_start + ws_len);
                self.replace_range(dl.byte_start + ws_len..prefix_end, "", EditKind::Atomic);
                let new_local = caret_local.saturating_sub(del_len);
                let new_head = (dl.byte_start + new_local).min(self.text.len());
                self.set_cursor(clamp_char_boundary(&self.text, new_head), false);
            }
        } else {
            let add_at = dl.byte_start + ws_len;
            self.replace_range(add_at..add_at, "## ", EditKind::Atomic);
            let new_head = (dl.byte_start + caret_local + 3).min(self.text.len());
            self.set_cursor(clamp_char_boundary(&self.text, new_head), false);
        }
        true
    }

    fn toggle_line_prefix(&mut self, prefix: &str) -> bool {
        let li = self.line_index_for_byte(self.head);
        let Some(dl) = self.lines.get(li).cloned() else {
            return false;
        };
        let raw_line = &self.text[dl.byte_start..dl.byte_end];
        let ws_len = raw_line.len() - raw_line.trim_start().len();
        let after_ws = &raw_line[ws_len..];
        let caret_local = self.head.saturating_sub(dl.byte_start);
        let start = dl.byte_start + ws_len;

        if after_ws.starts_with(prefix) {
            self.replace_range(start..start + prefix.len(), "", EditKind::Atomic);
            let new_local = caret_local.saturating_sub(prefix.len());
            let new_head = (dl.byte_start + new_local).min(self.text.len());
            self.set_cursor(clamp_char_boundary(&self.text, new_head), false);
        } else {
            self.replace_range(start..start, prefix, EditKind::Atomic);
            let new_head = (dl.byte_start + caret_local + prefix.len()).min(self.text.len());
            self.set_cursor(clamp_char_boundary(&self.text, new_head), false);
        }
        true
    }

    pub fn table_column_count(&self, start_li: usize, end_li: usize) -> usize {
        let mut cols = 1usize;
        let end = end_li.min(self.lines.len().saturating_sub(1));
        for li in start_li..=end {
            cols = cols.max(parse_table_row_cells(self.line_str(li)).len());
        }
        cols
    }

    fn set_cursor_in_table_cell(&mut self, row_li: usize, col_ix: usize, at_end: bool) {
        let Some(dl) = self.lines.get(row_li) else {
            return;
        };
        let raw = &self.text[dl.byte_start..dl.byte_end];
        let cells = parse_table_row_cells(raw);
        if let Some(cell) = cells.get(col_ix).or_else(|| cells.last()) {
            let local = if at_end {
                cell.content_range.end
            } else {
                cell.content_range.start
            };
            self.set_cursor(dl.byte_start + local, false);
        }
    }

    fn rebuild_table_columns<F>(&mut self, start_li: usize, end_li: usize, mut edit_row: F) -> bool
    where
        F: FnMut(usize, &mut Vec<String>),
    {
        if start_li > end_li || end_li >= self.lines.len() {
            return false;
        }
        let num_cols = self.table_column_count(start_li, end_li);
        let block_start = self.lines[start_li].byte_start;
        let block_end = self.lines[end_li].byte_end;
        let delim_li = start_li + 1;
        let mut rebuilt = String::new();
        for li in start_li..=end_li {
            if li > start_li {
                rebuilt.push('\n');
            }
            let raw = self.line_str(li);
            let mut texts: Vec<String> = parse_table_row_cells(raw)
                .into_iter()
                .map(|c| raw[c.content_range].to_string())
                .collect();
            let pad = if li == delim_li { "---" } else { "" };
            while texts.len() < num_cols {
                texts.push(pad.to_string());
            }
            edit_row(li, &mut texts);
            let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
            rebuilt.push_str(&format_table_row(&refs));
        }
        self.replace_range(block_start..block_end, &rebuilt, EditKind::Atomic);
        true
    }

    pub fn table_ensure_cell(&mut self, line_ix: usize, col_ix: usize) -> bool {
        let Some(dl) = self.lines.get(line_ix).cloned() else {
            return false;
        };
        if !matches!(
            dl.ctx,
            LineBlockContext::Table {
                is_delim: false,
                ..
            }
        ) {
            return false;
        }
        let raw = self.line_str(line_ix).to_string();
        let cells = parse_table_row_cells(&raw);
        if col_ix < cells.len() {
            self.set_cursor_in_table_cell(line_ix, col_ix, true);
            return true;
        }
        let mut texts: Vec<String> = cells
            .iter()
            .map(|c| raw[c.content_range.clone()].to_string())
            .collect();
        while texts.len() <= col_ix {
            texts.push(String::new());
        }
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let rebuilt = format_table_row(&refs);
        self.replace_range(dl.byte_start..dl.byte_end, &rebuilt, EditKind::Atomic);
        self.set_cursor_in_table_cell(line_ix, col_ix, true);
        true
    }

    pub fn table_insert_row_below(&mut self, row_li: usize) -> bool {
        let Some(dl) = self.lines.get(row_li).cloned() else {
            return false;
        };
        let LineBlockContext::Table {
            is_header,
            table_start_line,
            table_end_line,
            ..
        } = dl.ctx
        else {
            return false;
        };
        let insert_after = if is_header {
            (table_start_line + 1).min(self.lines.len().saturating_sub(1))
        } else {
            row_li
        };
        let Some(anchor_dl) = self.lines.get(insert_after).cloned() else {
            return false;
        };
        let num_cols = self.table_column_count(table_start_line, table_end_line);
        let empties = vec![""; num_cols];
        let new_row = format!("\n{}", format_table_row(&empties));
        self.replace_range(
            anchor_dl.byte_end..anchor_dl.byte_end,
            &new_row,
            EditKind::Atomic,
        );
        self.set_cursor_in_table_cell(insert_after + 1, 0, false);
        true
    }

    pub fn table_delete_row(&mut self, row_li: usize) -> bool {
        let Some(dl) = self.lines.get(row_li).cloned() else {
            return false;
        };
        let LineBlockContext::Table {
            is_header,
            is_delim,
            table_start_line,
            ..
        } = dl.ctx
        else {
            return false;
        };
        if is_header || is_delim {
            return false;
        }
        let del_range = if dl.byte_start > 0 {
            let mut start = dl.byte_start - 1;
            if start > 0 && self.text.as_bytes()[start - 1] == b'\r' {
                start -= 1;
            }
            start..dl.byte_end
        } else {
            dl.byte_start..dl.byte_end
        };
        let prev_non_delim = if row_li > table_start_line + 2 {
            row_li - 1
        } else {
            table_start_line
        };
        self.replace_range(del_range, "", EditKind::Atomic);
        self.set_cursor_in_table_cell(prev_non_delim, 0, true);
        true
    }

    pub fn table_insert_col_after(&mut self, row_li: usize, col_ix: usize) -> bool {
        let Some(dl) = self.lines.get(row_li).cloned() else {
            return false;
        };
        let LineBlockContext::Table {
            is_delim,
            table_start_line,
            table_end_line,
            ..
        } = dl.ctx
        else {
            return false;
        };
        let num_cols = self.table_column_count(table_start_line, table_end_line);
        let insert_pos = (col_ix + 1).min(num_cols);
        let delim_li = table_start_line + 1;
        if !self.rebuild_table_columns(table_start_line, table_end_line, |li, texts| {
            let val = if li == delim_li { "---" } else { "" };
            texts.insert(insert_pos.min(texts.len()), val.to_string());
        }) {
            return false;
        }
        let target_row = if is_delim { table_start_line } else { row_li };
        self.set_cursor_in_table_cell(target_row, insert_pos, false);
        true
    }

    pub fn table_delete_col(&mut self, row_li: usize, col_ix: usize) -> bool {
        let Some(dl) = self.lines.get(row_li).cloned() else {
            return false;
        };
        let LineBlockContext::Table {
            is_delim,
            table_start_line,
            table_end_line,
            ..
        } = dl.ctx
        else {
            return false;
        };
        let num_cols = self.table_column_count(table_start_line, table_end_line);
        if num_cols <= 1 {
            return false;
        }
        let rem_col = col_ix.min(num_cols - 1);
        if !self.rebuild_table_columns(table_start_line, table_end_line, |_, texts| {
            if rem_col < texts.len() {
                texts.remove(rem_col);
            }
        }) {
            return false;
        }
        let target_row = if is_delim { table_start_line } else { row_li };
        let target_col = rem_col.min(num_cols - 2);
        self.set_cursor_in_table_cell(target_row, target_col, true);
        true
    }

    pub fn table_cycle_col_align(&mut self, row_li: usize, col_ix: usize) -> bool {
        let Some(dl) = self.lines.get(row_li).cloned() else {
            return false;
        };
        let LineBlockContext::Table {
            is_delim,
            table_start_line,
            table_end_line,
            ..
        } = dl.ctx
        else {
            return false;
        };
        let delim_li = table_start_line + 1;
        let Some(delim_dl) = self.lines.get(delim_li).cloned() else {
            return false;
        };
        let num_cols = self.table_column_count(table_start_line, table_end_line);
        let target_col = col_ix.min(num_cols - 1);

        let rel_in_cell = if !is_delim {
            let raw_row = self.line_str(row_li);
            let local = self.head.saturating_sub(dl.byte_start).min(raw_row.len());
            let (cells, _) = table_cell_at_local_byte(raw_row, local);
            cells
                .get(target_col)
                .map(|c| local.saturating_sub(c.content_range.start))
                .unwrap_or(0)
        } else {
            0
        };

        let mut aligns = parse_table_alignments(self.line_str(delim_li));
        aligns.resize(num_cols, TableAlign::Left);
        aligns[target_col] = match aligns[target_col] {
            TableAlign::Left => TableAlign::Center,
            TableAlign::Center => TableAlign::Right,
            TableAlign::Right => TableAlign::Left,
        };
        let delim_strs: Vec<&str> = aligns.iter().copied().map(format_delim_cell).collect();
        let rebuilt = format_table_row(&delim_strs);
        self.replace_range(
            delim_dl.byte_start..delim_dl.byte_end,
            &rebuilt,
            EditKind::Atomic,
        );

        if is_delim {
            self.set_cursor_in_table_cell(delim_li, target_col, true);
        } else if let Some(new_dl) = self.lines.get(row_li) {
            let raw_row = &self.text[new_dl.byte_start..new_dl.byte_end];
            let cells = parse_table_row_cells(raw_row);
            if let Some(cell) = cells.get(target_col).or_else(|| cells.last()) {
                let local = (cell.content_range.start + rel_in_cell).min(cell.content_range.end);
                self.set_cursor(new_dl.byte_start + local, false);
            }
        }
        true
    }

    pub fn table_tab_move(&mut self, shift: bool) -> bool {
        let li = self.line_index_for_byte(self.head);
        let Some(dl) = self.lines.get(li).cloned() else {
            return false;
        };
        let LineBlockContext::Table {
            is_header,
            is_delim,
            table_start_line,
            table_end_line,
            ..
        } = dl.ctx
        else {
            return false;
        };
        self.break_undo_coalescing();
        let num_cols = self.table_column_count(table_start_line, table_end_line);

        if is_delim {
            if shift {
                self.table_ensure_cell(table_start_line, num_cols.saturating_sub(1));
            } else if table_end_line > table_start_line + 1 {
                self.table_ensure_cell(table_start_line + 2, 0);
            } else {
                self.table_insert_row_below(table_start_line);
            }
            return true;
        }

        let raw = self.line_str(li);
        let local = self.head.saturating_sub(dl.byte_start).min(raw.len());
        let (_, col_ix) = table_cell_at_local_byte(raw, local);

        if shift {
            if col_ix > 0 {
                self.table_ensure_cell(li, col_ix - 1);
            } else if li > table_start_line + 2 {
                self.table_ensure_cell(li - 1, num_cols.saturating_sub(1));
            } else if !is_header {
                self.table_ensure_cell(table_start_line, num_cols.saturating_sub(1));
            } else {
                self.set_cursor_in_table_cell(li, 0, false);
            }
        } else if col_ix + 1 < num_cols {
            self.table_ensure_cell(li, col_ix + 1);
        } else if is_header {
            if table_end_line > table_start_line + 1 {
                self.table_ensure_cell(table_start_line + 2, 0);
            } else {
                self.table_insert_row_below(table_start_line);
            }
        } else if li < table_end_line {
            self.table_ensure_cell(li + 1, 0);
        } else {
            self.table_insert_row_below(li);
        }
        true
    }

    pub fn move_left(&mut self, by_word: bool, extend: bool) {
        self.break_undo_coalescing();
        if !extend && self.has_selection() && !by_word {
            let start = self.selection_range().start;
            self.set_cursor(start, false);
            return;
        }
        if !by_word && !extend && !self.has_selection() {
            let li = self.line_index_for_byte(self.head);
            if let Some(dl) = self.lines.get(li)
                && let LineBlockContext::Table {
                    is_header,
                    is_delim: false,
                    table_start_line,
                    ..
                } = dl.ctx
            {
                let raw = &self.text[dl.byte_start..dl.byte_end];
                let local = self.head.saturating_sub(dl.byte_start).min(raw.len());
                let (cells, col_ix) = table_cell_at_local_byte(raw, local);
                if let Some(cell) = cells.get(col_ix) {
                    if local > cell.inner_range.end {
                        self.set_cursor(dl.byte_start + cell.content_range.end, false);
                        return;
                    }
                    if local <= cell.content_range.start {
                        if col_ix > 0 {
                            self.set_cursor(
                                dl.byte_start + cells[col_ix - 1].content_range.end,
                                false,
                            );
                        } else if !is_header {
                            let prev_li = if li == table_start_line + 2 {
                                table_start_line
                            } else {
                                li - 1
                            };
                            let prev_cells = parse_table_row_cells(self.line_str(prev_li));
                            let target = prev_cells.last().map_or(
                                self.lines[prev_li].byte_end,
                                |c| self.lines[prev_li].byte_start + c.content_range.end,
                            );
                            self.set_cursor(target, false);
                        } else if li > 0 {
                            self.set_cursor(self.lines[li - 1].byte_end, false);
                        }
                        return;
                    }
                }
            }
        }
        let target = if by_word {
            prev_word_boundary(&self.text, self.head)
        } else {
            prev_char_boundary(&self.text, self.head)
        };
        self.set_cursor(target, extend);
    }

    pub fn move_right(&mut self, by_word: bool, extend: bool) {
        self.break_undo_coalescing();
        if !extend && self.has_selection() && !by_word {
            let end = self.selection_range().end;
            self.set_cursor(end, false);
            return;
        }
        if !by_word && !extend && !self.has_selection() {
            let li = self.line_index_for_byte(self.head);
            if let Some(dl) = self.lines.get(li)
                && let LineBlockContext::Table {
                    is_header,
                    is_delim: false,
                    table_start_line,
                    table_end_line,
                    ..
                } = dl.ctx
            {
                let raw = &self.text[dl.byte_start..dl.byte_end];
                let local = self.head.saturating_sub(dl.byte_start).min(raw.len());
                let (cells, col_ix) = table_cell_at_local_byte(raw, local);
                if let Some(cell) = cells.get(col_ix) {
                    if local < cell.content_range.start {
                        self.set_cursor(dl.byte_start + cell.content_range.start, false);
                        return;
                    }
                    if local >= cell.content_range.end {
                        if col_ix + 1 < cells.len() {
                            self.set_cursor(
                                dl.byte_start + cells[col_ix + 1].content_range.start,
                                false,
                            );
                        } else {
                            let next_row = if is_header {
                                if table_end_line > table_start_line + 1 {
                                    Some(table_start_line + 2)
                                } else {
                                    None
                                }
                            } else if li < table_end_line {
                                Some(li + 1)
                            } else {
                                None
                            };
                            if let Some(nli) = next_row {
                                self.set_cursor_in_table_cell(nli, 0, false);
                            } else if table_end_line + 1 < self.lines.len() {
                                self.set_cursor(self.lines[table_end_line + 1].byte_start, false);
                            }
                        }
                        return;
                    }
                }
            }
        }
        let target = if by_word {
            next_word_boundary(&self.text, self.head)
        } else {
            next_char_boundary(&self.text, self.head)
        };
        self.set_cursor(target, extend);
    }

    pub fn move_line_start(&mut self, extend: bool) {
        self.break_undo_coalescing();
        let li = self.line_index_for_byte(self.head);
        if !extend
            && let Some(dl) = self.lines.get(li)
            && matches!(dl.ctx, LineBlockContext::Table { is_delim: false, .. })
        {
            let raw = self.line_str(li);
            let local = self.head.saturating_sub(dl.byte_start).min(raw.len());
            let (cells, col_ix) = table_cell_at_local_byte(raw, local);
            if let Some(cell) = cells.get(col_ix) {
                let cell_start = dl.byte_start + cell.content_range.start;
                let row_start = dl.byte_start + cells[0].content_range.start;
                let target = if self.head == cell_start {
                    row_start
                } else {
                    cell_start
                };
                self.set_cursor(target, false);
                return;
            }
        }
        let target = self.lines.get(li).map_or(0, |l| l.byte_start);
        self.set_cursor(target, extend);
    }

    pub fn move_line_end(&mut self, extend: bool) {
        self.break_undo_coalescing();
        let li = self.line_index_for_byte(self.head);
        if !extend
            && let Some(dl) = self.lines.get(li)
            && matches!(dl.ctx, LineBlockContext::Table { is_delim: false, .. })
        {
            let raw = self.line_str(li);
            let local = self.head.saturating_sub(dl.byte_start).min(raw.len());
            let (cells, col_ix) = table_cell_at_local_byte(raw, local);
            if let Some(cell) = cells.get(col_ix) {
                let cell_end = dl.byte_start + cell.content_range.end;
                let row_end = dl.byte_start + cells.last().unwrap().content_range.end;
                let target = if self.head == cell_end {
                    row_end
                } else {
                    cell_end
                };
                self.set_cursor(target, false);
                return;
            }
        }
        let target = self
            .lines
            .get(li)
            .map_or(self.text.len(), |l| l.byte_end);
        self.set_cursor(target, extend);
    }

    pub fn select_word_at(&mut self, byte_offset: usize) {
        let pos = clamp_char_boundary(&self.text, byte_offset);
        if self.text.is_empty() {
            self.set_cursor(0, false);
            return;
        }
        let ch = self.text[pos..].chars().next();
        if ch.is_some_and(is_word_char) {
            let mut s = pos;
            while s > 0 {
                let prev = prev_char_boundary(&self.text, s);
                if self.text[prev..s].chars().next().is_some_and(is_word_char) {
                    s = prev;
                } else {
                    break;
                }
            }
            let mut e = pos;
            while e < self.text.len() {
                let next = next_char_boundary(&self.text, e);
                if self.text[e..next].chars().next().is_some_and(is_word_char) {
                    e = next;
                } else {
                    break;
                }
            }
            self.set_selection(s, e);
        } else if pos < self.text.len() {
            let next = next_char_boundary(&self.text, pos);
            self.set_selection(pos, next);
        }
    }

    pub fn select_line_at(&mut self, byte_offset: usize) {
        let li = self.line_index_for_byte(byte_offset);
        if let Some(dl) = self.lines.get(li) {
            if matches!(dl.ctx, LineBlockContext::Table { is_delim: false, .. }) {
                let raw = self.line_str(li);
                let local = byte_offset.saturating_sub(dl.byte_start).min(raw.len());
                let (cells, col_ix) = table_cell_at_local_byte(raw, local);
                if let Some(cell) = cells.get(col_ix) {
                    if !cell.content_range.is_empty() {
                        self.set_selection(
                            dl.byte_start + cell.content_range.start,
                            dl.byte_start + cell.content_range.end,
                        );
                    } else {
                        self.set_cursor(dl.byte_start + cell.content_range.start, false);
                    }
                    return;
                }
            }
            self.set_selection(dl.byte_start, dl.byte_end);
        }
    }

    pub fn select_all(&mut self) {
        self.set_selection(0, self.text.len());
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo_stack.pop() else {
            return false;
        };
        self.undo_bytes = self.undo_bytes.saturating_sub(prev.text.len());
        self.redo_stack.push(UndoSnapshot {
            text: self.text.clone(),
            anchor: self.anchor,
            head: self.head,
        });
        self.text = prev.text;
        self.anchor = clamp_char_boundary(&self.text, prev.anchor);
        self.head = clamp_char_boundary(&self.text, prev.head);
        self.lines = classify_document_lines(&self.text, self.is_mdx);
        self.version = self.version.wrapping_add(1);
        self.marked_range = None;
        self.last_edit = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.push_undo_snapshot(UndoSnapshot {
            text: self.text.clone(),
            anchor: self.anchor,
            head: self.head,
        });
        self.text = next.text;
        self.anchor = clamp_char_boundary(&self.text, next.anchor);
        self.head = clamp_char_boundary(&self.text, next.head);
        self.lines = classify_document_lines(&self.text, self.is_mdx);
        self.version = self.version.wrapping_add(1);
        self.marked_range = None;
        self.last_edit = None;
        true
    }
}

fn format_table_row(cells: &[&str]) -> String {
    let mut out = String::from("|");
    for cell in cells {
        out.push(' ');
        out.push_str(cell.trim());
        out.push_str(" |");
    }
    out
}

fn format_delim_cell(align: TableAlign) -> &'static str {
    match align {
        TableAlign::Left => "---",
        TableAlign::Center => ":---:",
        TableAlign::Right => "---:",
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn prev_char_boundary(s: &str, pos: usize) -> usize {
    let p = clamp_char_boundary(s, pos);
    s[..p].char_indices().next_back().map_or(0, |(idx, _)| idx)
}

fn next_char_boundary(s: &str, pos: usize) -> usize {
    let p = clamp_char_boundary(s, pos);
    s[p..]
        .chars()
        .next()
        .map_or(s.len(), |ch| p + ch.len_utf8())
}

fn prev_word_boundary(s: &str, pos: usize) -> usize {
    let mut p = clamp_char_boundary(s, pos);
    while p > 0 {
        let prev = prev_char_boundary(s, p);
        let ch = s[prev..p].chars().next().unwrap();
        if ch == '\n' {
            return if p == pos { prev } else { p };
        }
        if !ch.is_whitespace() {
            break;
        }
        p = prev;
    }
    if p == 0 {
        return 0;
    }
    let first_is_word = s[prev_char_boundary(s, p)..p]
        .chars()
        .next()
        .is_some_and(is_word_char);
    while p > 0 {
        let prev = prev_char_boundary(s, p);
        let ch = s[prev..p].chars().next().unwrap();
        if ch.is_whitespace() || is_word_char(ch) != first_is_word {
            break;
        }
        p = prev;
    }
    p
}

fn next_word_boundary(s: &str, pos: usize) -> usize {
    let mut p = clamp_char_boundary(s, pos);
    while p < s.len() {
        let ch = s[p..].chars().next().unwrap();
        if ch == '\n' {
            return if p == pos { p + 1 } else { p };
        }
        if !ch.is_whitespace() {
            break;
        }
        p += ch.len_utf8();
    }
    if p >= s.len() {
        return s.len();
    }
    let first_is_word = s[p..].chars().next().is_some_and(is_word_char);
    while p < s.len() {
        let ch = s[p..].chars().next().unwrap();
        if ch.is_whitespace() || is_word_char(ch) != first_is_word {
            break;
        }
        p += ch.len_utf8();
    }
    p
}

/// Prefix-sum table tracking exact / estimated vertical line heights for O(log N) scroll lookups.
pub struct LineHeightTable {
    heights: Vec<f32>,
    prefix: Vec<f32>,
    dirty: bool,
}

impl LineHeightTable {
    pub fn from_lines(lines: &[DocLine], raw: &str, est_col_width: f32) -> Self {
        let heights: Vec<f32> = lines
            .iter()
            .map(|l| estimate_line_height(&raw[l.byte_start..l.byte_end], &l.ctx, est_col_width))
            .collect();
        let len = heights.len();
        Self {
            heights,
            prefix: vec![0.0; len + 1],
            dirty: true,
        }
    }

    pub fn sync_lines(&mut self, lines: &[DocLine], raw: &str, est_col_width: f32) {
        if self.heights.len() == lines.len() {
            return;
        }
        self.heights = lines
            .iter()
            .map(|l| estimate_line_height(&raw[l.byte_start..l.byte_end], &l.ctx, est_col_width))
            .collect();
        self.dirty = true;
    }

    fn ensure_prefix(&mut self) {
        if !self.dirty {
            return;
        }
        self.prefix.resize(self.heights.len() + 1, 0.0);
        self.prefix[0] = 0.0;
        let mut acc = 0.0f32;
        for (i, &h) in self.heights.iter().enumerate() {
            acc += h;
            self.prefix[i + 1] = acc;
        }
        self.dirty = false;
    }

    pub fn set_height(&mut self, ix: usize, h: f32) -> bool {
        if let Some(slot) = self.heights.get_mut(ix)
            && (*slot - h).abs() > 0.25
        {
            *slot = h;
            self.dirty = true;
            return true;
        }
        false
    }

    pub fn total_height(&mut self) -> f32 {
        self.ensure_prefix();
        *self.prefix.last().unwrap_or(&0.0)
    }

    pub fn y_for_line(&mut self, ix: usize) -> f32 {
        self.ensure_prefix();
        self.prefix[ix.min(self.heights.len())]
    }

    pub fn line_for_y(&mut self, y: f32) -> usize {
        self.ensure_prefix();
        if self.heights.is_empty() || y <= 0.0 {
            return 0;
        }
        let max_ix = self.heights.len() - 1;
        let p = self.prefix.partition_point(|&val| val <= y);
        p.saturating_sub(1).min(max_ix)
    }
}

pub fn estimate_line_height(raw_line: &str, ctx: &LineBlockContext, col_width: f32) -> f32 {
    let usable_w = (col_width - 24.0).max(160.0);
    let chars = raw_line.chars().count().max(1) as f32;
    match ctx {
        LineBlockContext::Heading { level, .. } => {
            let (cw, lh, pad) = match level {
                1 => (14.5, 34.0, 16.0),
                2 => (11.2, 27.0, 13.0),
                3 => (9.2, 23.0, 10.0),
                _ => (8.2, 21.0, 8.0),
            };
            let rows = ((chars * cw) / usable_w).ceil().max(1.0);
            rows * lh + pad
        }
        LineBlockContext::ImageEmbed { alt, .. } => {
            let spec = parse_image_spec(alt);
            if spec.is_floated() {
                4.0
            } else {
                compute_image_display_size(&spec, None, (col_width - 16.0).max(120.0)).1 + 14.0
            }
        }
        LineBlockContext::SceneBreak => 24.0,
        LineBlockContext::Blank => 24.75,
        LineBlockContext::Table {
            is_delim,
            is_header,
            is_first,
            is_last,
            ..
        } => {
            if *is_delim {
                0.0
            } else if *is_header {
                38.0 + if *is_first { 6.0 } else { 0.0 }
            } else {
                34.0 + if *is_last { 8.0 } else { 0.0 }
            }
        }
        LineBlockContext::CodeFence { .. }
        | LineBlockContext::Frontmatter { .. }
        | LineBlockContext::MdxBlock { .. } => {
            let rows = ((chars * 8.1) / usable_w).ceil().max(1.0);
            rows * 21.0 + 4.0
        }
        _ => {
            let rows = ((chars * 7.8) / usable_w).ceil().max(1.0);
            rows * 24.75 + 4.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_projection_invariants(raw: &str, ctx: &LineBlockContext, cursor: Option<Range<usize>>) {
        let proj = project_line(raw, ctx, cursor);
        // Spans must contiguously cover 0..display_text.len()
        let mut cur = 0usize;
        for sp in &proj.spans {
            assert_eq!(sp.range.start, cur, "gap/overlap in spans for {raw:?}");
            assert!(sp.range.end > sp.range.start);
            assert!(proj.display_text.is_char_boundary(sp.range.start));
            assert!(proj.display_text.is_char_boundary(sp.range.end));
            cur = sp.range.end;
        }
        assert_eq!(cur, proj.display_text.len(), "spans did not cover end for {raw:?}");

        // Bijective char boundaries
        for (rb, _) in raw.char_indices() {
            let db = proj.raw_to_display(rb);
            assert!(proj.display_text.is_char_boundary(db));
        }
        for (db, _) in proj.display_text.char_indices() {
            let rb = proj.display_to_raw(raw, db);
            assert!(raw.is_char_boundary(rb));
        }
    }

    #[test]
    fn classifies_all_markdown_blocks() {
        let doc = "---\ntitle: Test\n---\n\n# Heading\n- [x] Task\n- Item\n> [!note] Tip\n> Body\n```rust\nfn a() {}\n```\n| a | b |\n| --- | --- |\n| 1 | 2 |\n![[pic.png|400]]\n---\n<Callout />";
        let lines = classify_document_lines(doc, true);
        assert!(matches!(lines[0].ctx, LineBlockContext::Frontmatter { is_delim: true, .. }));
        assert!(matches!(lines[1].ctx, LineBlockContext::Frontmatter { is_delim: false, .. }));
        assert!(matches!(lines[2].ctx, LineBlockContext::Frontmatter { is_delim: true, .. }));
        assert!(matches!(lines[3].ctx, LineBlockContext::Blank));
        assert!(matches!(lines[4].ctx, LineBlockContext::Heading { level: 1, prefix_bytes: 2 }));
        assert!(matches!(lines[5].ctx, LineBlockContext::TaskItem { checked: true, .. }));
        assert!(matches!(lines[6].ctx, LineBlockContext::ListItem { ordered: false, .. }));
        assert!(matches!(lines[7].ctx, LineBlockContext::Callout { is_header: true, .. }));
        assert!(matches!(lines[8].ctx, LineBlockContext::Callout { is_header: false, .. }));
        assert!(matches!(lines[9].ctx, LineBlockContext::CodeFence { is_open: true, .. }));
        assert!(matches!(lines[10].ctx, LineBlockContext::CodeFence { is_open: false, is_close: false, .. }));
        assert!(matches!(lines[11].ctx, LineBlockContext::CodeFence { is_close: true, .. }));
        assert!(matches!(lines[12].ctx, LineBlockContext::Table { is_header: true, .. }));
        assert!(matches!(lines[13].ctx, LineBlockContext::Table { is_delim: true, .. }));
        assert!(matches!(lines[14].ctx, LineBlockContext::Table { is_header: false, is_delim: false, .. }));
        assert!(matches!(lines[15].ctx, LineBlockContext::ImageEmbed { .. }));
        assert!(matches!(lines[16].ctx, LineBlockContext::SceneBreak));
        assert!(matches!(lines[17].ctx, LineBlockContext::MdxBlock { .. }));
    }

    #[test]
    fn projects_token_level_conceal_and_preserves_snake_case() {
        let raw = "Hello **bold** and `code` with snake_case_var and [[note|Alias]] plus ==hi==";
        let ctx = LineBlockContext::Paragraph;

        // Cursor elsewhere -> syntax concealed
        let concealed = project_line(raw, &ctx, None);
        assert_eq!(
            concealed.display_text,
            "Hello bold and code with snake_case_var and Alias plus hi"
        );
        assert_eq!(concealed.links.len(), 1);
        assert_eq!(
            concealed.links[0].target,
            LinkTarget::Wikilink("note".into())
        );
        assert!(concealed.spans.iter().any(|s| s.style.bold && &concealed.display_text[s.range.clone()] == "bold"));
        assert!(concealed.spans.iter().any(|s| s.style.code && &concealed.display_text[s.range.clone()] == "code"));
        assert!(concealed.spans.iter().any(|s| s.style.highlight && &concealed.display_text[s.range.clone()] == "hi"));
        assert_projection_invariants(raw, &ctx, None);

        // Cursor touching **bold** -> ONLY **bold** markers revealed on that token
        let bold_mid = raw.find("bold").unwrap() + 1;
        let revealed = project_line(raw, &ctx, Some(bold_mid..bold_mid));
        assert_eq!(
            revealed.display_text,
            "Hello **bold** and code with snake_case_var and Alias plus hi"
        );
        assert_projection_invariants(raw, &ctx, Some(bold_mid..bold_mid));

        // Display -> raw click mapping on concealed wikilink lands on visible alias chars
        let alias_disp = concealed.display_text.find("Alias").unwrap() + 2;
        let raw_hit = concealed.display_to_raw(raw, alias_disp);
        assert_eq!(&raw[raw_hit..raw_hit + 3], "ias");

        // Cyrillic + emoji safety
        let uni = "# Заголовок **жирный** 🚀 `код`";
        let uni_ctx = LineBlockContext::Heading { level: 1, prefix_bytes: 2 };
        assert_projection_invariants(uni, &uni_ctx, None);
        assert_projection_invariants(uni, &uni_ctx, Some(2..4));
    }

    #[test]
    fn utf8_utf16_roundtrip_handles_multibyte_and_emoji() {
        let s = "Hi Привет 🚀!";
        for (byte_ix, _) in s.char_indices() {
            let u16_off = utf8_to_utf16_offset(s, byte_ix);
            assert_eq!(utf16_to_utf8_offset(s, u16_off), byte_ix);
        }
        assert_eq!(utf16_to_utf8_offset(s, utf8_to_utf16_offset(s, s.len())), s.len());
    }

    #[test]
    fn doc_buffer_handles_smart_enter_indent_wrap_task_and_undo() {
        let mut buf = LiveDocBuffer::new("- [ ] First".to_string(), false);
        buf.set_cursor(buf.text.len(), false);
        buf.smart_enter();
        assert_eq!(buf.text, "- [ ] First\n- [ ] ");

        // Enter on empty task clears prefix
        buf.smart_enter();
        assert_eq!(buf.text, "- [ ] First\n");

        // Toggle task checkbox without disturbing cursor at end
        buf.set_cursor(buf.text.len(), false);
        assert!(buf.toggle_task_on_line(0));
        assert!(buf.text.starts_with("- [x] First"));

        // Auto-wrap selection with '*'
        buf.set_selection(6, 11);
        buf.insert_text("*");
        assert_eq!(buf.text, "- [x] *First*\n");
        assert_eq!(buf.selected_text(), "First");

        // Undo restores step-by-step cleanly
        assert!(buf.undo());
        assert_eq!(buf.text, "- [x] First\n");
        assert!(buf.redo());
        assert_eq!(buf.text, "- [x] *First*\n");
    }

    #[test]
    fn line_height_table_binary_search_and_updates() {
        let raw = "a\nb\nc\nd";
        let lines = classify_document_lines(raw, false);
        let mut table = LineHeightTable::from_lines(&lines, raw, 700.0);
        for i in 0..4 {
            table.set_height(i, 30.0);
        }
        assert_eq!(table.total_height(), 120.0);
        assert_eq!(table.y_for_line(2), 60.0);
        assert_eq!(table.line_for_y(0.0), 0);
        assert_eq!(table.line_for_y(31.0), 1);
        assert_eq!(table.line_for_y(89.9), 2);
        assert_eq!(table.line_for_y(119.0), 3);
    }

    #[test]
    fn parses_table_cells_and_alignments_across_edge_cases() {
        let std_row = "| Name | Role | Notes |";
        let cells = parse_table_row_cells(std_row);
        assert_eq!(cells.len(), 3);
        assert_eq!(&std_row[cells[0].content_range.clone()], "Name");
        assert_eq!(&std_row[cells[1].content_range.clone()], "Role");
        assert_eq!(&std_row[cells[2].content_range.clone()], "Notes");

        let bare = "left | center | right";
        let bare_cells = parse_table_row_cells(bare);
        assert_eq!(bare_cells.len(), 3);
        assert_eq!(&bare[bare_cells[0].content_range.clone()], "left");
        assert_eq!(&bare[bare_cells[1].content_range.clone()], "center");
        assert_eq!(&bare[bare_cells[2].content_range.clone()], "right");

        let empty = "|  |  |";
        let empty_cells = parse_table_row_cells(empty);
        assert_eq!(empty_cells.len(), 2);
        assert_eq!(empty_cells[0].content_range, 2..2);
        assert_eq!(empty_cells[1].content_range, 5..5);

        let tricky = r"| A \| B | `x | y` | [[Note|Alias]] | Привет 🚀 |";
        let tricky_cells = parse_table_row_cells(tricky);
        assert_eq!(tricky_cells.len(), 4);
        assert_eq!(&tricky[tricky_cells[0].content_range.clone()], r"A \| B");
        assert_eq!(&tricky[tricky_cells[1].content_range.clone()], "`x | y`");
        assert_eq!(
            &tricky[tricky_cells[2].content_range.clone()],
            "[[Note|Alias]]"
        );
        assert_eq!(&tricky[tricky_cells[3].content_range.clone()], "Привет 🚀");

        let aligns = parse_table_alignments("| :--- | :---: | ---: | --- |");
        assert_eq!(
            aligns,
            vec![
                TableAlign::Left,
                TableAlign::Center,
                TableAlign::Right,
                TableAlign::Left
            ]
        );
    }

    #[test]
    fn hr_below_pipe_paragraph_is_not_misclassified_as_table() {
        let doc = "foo | bar\n---";
        let lines = classify_document_lines(doc, false);
        assert!(matches!(lines[0].ctx, LineBlockContext::Paragraph));
        assert!(matches!(lines[1].ctx, LineBlockContext::SceneBreak));
    }

    #[test]
    fn projects_table_cells_with_conceal_trailing_space_and_utf8_roundtrip() {
        let row = "| **Bold** | [[Target|Алиас]] 🚀 | Plain |";
        let (concealed, active) = project_table_cells(row, false, 4, None);
        assert_eq!(active, None);
        assert_eq!(concealed.len(), 4);
        assert_eq!(concealed[0].projected.display_text, "Bold");
        assert_eq!(concealed[1].projected.display_text, "Алиас 🚀");
        assert_eq!(concealed[2].projected.display_text, "Plain");
        assert!(concealed[3].inner_range.is_none());

        // Cursor touching cell 0 reveals **Bold** ONLY in cell 0.
        let bold_mid = row.find("Bold").unwrap() + 1;
        let (revealed, active_col) = project_table_cells(row, false, 3, Some(bold_mid..bold_mid));
        assert_eq!(active_col, Some(0));
        assert_eq!(revealed[0].projected.display_text, "**Bold**");
        assert_eq!(revealed[1].projected.display_text, "Алиас 🚀");

        // Bijective char boundary invariants inside Unicode table cell.
        let c1 = &concealed[1];
        let raw_slice = &row[c1.raw_offset..c1.raw_offset + c1.projected.raw_len];
        for (rb, _) in raw_slice.char_indices() {
            let db = c1.projected.raw_to_display(rb);
            assert!(c1.projected.display_text.is_char_boundary(db));
        }
        for (db, _) in c1.projected.display_text.char_indices() {
            let rb = c1.projected.display_to_raw(raw_slice, db);
            assert!(raw_slice.is_char_boundary(rb));
        }

        // Cursor-expanded trailing space visibility while typing in cell.
        let typing_space = "| Foo  | Bar |";
        let (typed, active2) = project_table_cells(typing_space, false, 2, Some(6..6));
        assert_eq!(active2, Some(0));
        assert_eq!(typed[0].projected.display_text, "Foo ");
        assert_eq!(
            typed[0]
                .projected
                .raw_to_display(6 - typed[0].raw_offset),
            4
        );
    }

    #[test]
    fn doc_buffer_table_navigation_protection_and_structural_edits() {
        let initial = "| Col1 | Col2 |\n| --- | --- |\n| A | B |";
        let mut buf = LiveDocBuffer::new(initial.to_string(), false);

        // Tab from header cell 0 -> header cell 1 -> body row 0 cell 0 -> cell 1 -> append new row
        let a_start = parse_table_row_cells(buf.line_str(0))[0].content_range.start;
        buf.set_cursor(buf.lines[0].byte_start + a_start, false);
        assert!(buf.table_tab_move(false));
        assert_eq!(buf.line_index_for_byte(buf.head), 0);
        assert!(buf.table_tab_move(false));
        assert_eq!(buf.line_index_for_byte(buf.head), 2);
        assert!(buf.table_tab_move(false));
        assert_eq!(buf.line_index_for_byte(buf.head), 2);
        assert!(buf.table_tab_move(false));
        assert_eq!(buf.line_index_for_byte(buf.head), 3);
        assert_eq!(buf.line_str(3), "|  |  |");

        // Shift+Tab navigates back cleanly across rows and skips delim
        assert!(buf.table_tab_move(true));
        assert_eq!(buf.line_index_for_byte(buf.head), 2);
        assert!(buf.table_tab_move(true));
        assert_eq!(buf.line_index_for_byte(buf.head), 2);
        assert!(buf.table_tab_move(true));
        assert_eq!(buf.line_index_for_byte(buf.head), 0);

        // Pipe protection on Backspace & Delete inside a cell
        let row2_start = buf.lines[2].byte_start;
        let row2_cells = parse_table_row_cells(buf.line_str(2));
        buf.set_cursor(row2_start + row2_cells[1].content_range.start, false);
        buf.backspace(false);
        assert_eq!(buf.line_str(2), "| A | B |");
        buf.set_cursor(row2_start + row2_cells[1].content_range.end, false);
        buf.delete(false);
        assert_eq!(buf.line_str(2), "| A | B |");

        // Enter on empty trailing row exits table onto blank line; undo brings it back
        let row3_cells = parse_table_row_cells(buf.line_str(3));
        buf.set_cursor(buf.lines[3].byte_start + row3_cells[0].content_range.start, false);
        buf.smart_enter();
        assert_eq!(buf.text, "| Col1 | Col2 |\n| --- | --- |\n| A | B |\n");
        assert!(buf.undo());
        assert_eq!(
            buf.text,
            "| Col1 | Col2 |\n| --- | --- |\n| A | B |\n|  |  |"
        );

        // Delete body row 3 cleanly, pad ragged cell, insert/delete col, cycle align
        assert!(buf.table_delete_row(3));
        assert_eq!(buf.text, "| Col1 | Col2 |\n| --- | --- |\n| A | B |");

        assert!(buf.table_ensure_cell(2, 2));
        assert_eq!(buf.line_str(2), "| A | B |  |");

        assert!(buf.table_insert_col_after(0, 0));
        assert_eq!(buf.line_str(0), "| Col1 |  | Col2 |  |");
        assert_eq!(buf.line_str(1), "| --- | --- | --- | --- |");
        assert_eq!(buf.line_str(2), "| A |  | B |  |");

        assert!(buf.table_cycle_col_align(0, 1));
        assert_eq!(buf.line_str(1), "| --- | :---: | --- | --- |");
        assert!(buf.table_cycle_col_align(0, 1));
        assert_eq!(buf.line_str(1), "| --- | ---: | --- | --- |");

        assert!(buf.table_delete_col(0, 1));
        assert_eq!(buf.line_str(0), "| Col1 | Col2 |  |");
        assert!(buf.undo());
        assert_eq!(buf.line_str(0), "| Col1 |  | Col2 |  |");
        assert!(buf.redo());
        assert_eq!(buf.line_str(0), "| Col1 | Col2 |  |");

        // Selecting entire table and deleting still removes the table normally
        buf.select_all();
        buf.backspace(false);
        assert_eq!(buf.text, "");
    }

    #[test]
    fn undo_stack_enforces_byte_budget_on_large_docs() {
        let big = "a".repeat(1024 * 1024); // 1 MiB
        let mut buf = LiveDocBuffer::new(big, false);
        for i in 0..10u8 {
            let ch = (b'0' + i) as char;
            let len = buf.text.len();
            buf.replace_range(len..len, &ch.to_string(), EditKind::Atomic);
        }
        assert!(buf.undo_bytes <= MAX_UNDO_BYTES);
        assert!(buf.undo_stack.len() >= 2);
        assert!(buf.undo_stack.len() <= 6);
        assert!(buf.undo());
        assert!(buf.redo());
    }

    #[test]
    fn rewrites_updates_and_deletes_image_embed_lines_with_cursor_preservation() {
        let raw = "Intro line\n  ![[attachments/photo.png|300|left]]\nTrailing prose line";
        let mut buf = LiveDocBuffer::new(raw.to_string(), false);
        // Place caret on "prose" in line 2 (last line)
        let trailing_pos = buf.text.find("prose").unwrap();
        buf.set_cursor(trailing_pos, false);

        let mut spec = parse_image_spec("300|left");
        spec.width = Some(420.0);
        spec.align = ImageAlign::Right;
        spec.wrap = true;
        spec.border = true;

        assert!(buf.update_image_embed_spec(1, &spec));
        assert_eq!(
            buf.line_str(1),
            "  ![[attachments/photo.png|420|right|wrap|border]]"
        );
        // Caret stays on "prose" in line 2 despite byte length change above
        assert_eq!(&buf.text[buf.head..buf.head + 5], "prose");

        // No-op update does not add an undo snapshot
        let ver_before = buf.version;
        assert!(!buf.update_image_embed_spec(1, &spec));
        assert_eq!(buf.version, ver_before);

        // Undo restores exact line & cursor
        assert!(buf.undo());
        assert_eq!(buf.line_str(1), "  ![[attachments/photo.png|300|left]]");
        assert_eq!(buf.head, trailing_pos);
        assert!(buf.redo());
        assert_eq!(
            buf.line_str(1),
            "  ![[attachments/photo.png|420|right|wrap|border]]"
        );

        // Standard markdown image rewrite
        let md_rewritten = rewrite_image_embed_line("![old alt|200](assets/a.jpg)", &spec).unwrap();
        assert_eq!(md_rewritten, "![420|right|wrap|border](assets/a.jpg)");

        // Empty spec removes trailing pipe on obsidian embed
        let clean = rewrite_image_embed_line("![[photo.png|320|center]]", &StyledImageSpec::default()).unwrap();
        assert_eq!(clean, "![[photo.png]]");

        // Floated image embeds estimate minimal advance height so prose wraps beside them on frame 1
        assert_eq!(
            estimate_line_height(
                "![[photo.png|240|left|wrap]]",
                &LineBlockContext::ImageEmbed {
                    target: "photo.png".into(),
                    alt: "240|left|wrap".into(),
                },
                760.0
            ),
            4.0
        );

        // Delete image line cleanly
        assert!(buf.delete_line(1));
        assert_eq!(buf.text, "Intro line\nTrailing prose line");
        assert!(buf.undo());
        assert_eq!(
            buf.line_str(1),
            "  ![[attachments/photo.png|420|right|wrap|border]]"
        );
    }

    #[test]
    fn indent_and_outdent_behave_cleanly_for_lists_prose_and_selections() {
        // List item mid-line: Tab indents whole item and preserves caret position in text
        let mut list_buf = LiveDocBuffer::new("- First item\n- [ ] Task item".to_string(), false);
        let item_pos = list_buf.text.find("item").unwrap();
        list_buf.set_cursor(item_pos, false);
        list_buf.indent_lines(false);
        assert_eq!(list_buf.line_str(0), "  - First item");
        assert_eq!(&list_buf.text[list_buf.head..list_buf.head + 4], "item");

        list_buf.indent_lines(true);
        assert_eq!(list_buf.line_str(0), "- First item");
        assert_eq!(&list_buf.text[list_buf.head..list_buf.head + 4], "item");

        // Task item nests too
        let task_pos = list_buf.text.find("Task").unwrap();
        list_buf.set_cursor(task_pos, false);
        list_buf.indent_lines(false);
        assert_eq!(list_buf.line_str(1), "  - [ ] Task item");
        assert_eq!(&list_buf.text[list_buf.head..list_buf.head + 4], "Task");

        // Prose line: Tab inserts two spaces at caret, Shift+Tab removes inline or leading indent cleanly
        let mut prose = LiveDocBuffer::new("Hello world".to_string(), false);
        prose.set_cursor(5, false);
        prose.indent_lines(false);
        assert_eq!(prose.text, "Hello   world");
        assert_eq!(prose.head, 7);
        prose.indent_lines(true);
        assert_eq!(prose.text, "Hello world");
        assert_eq!(prose.head, 5);

        // Leading-indented prose outdents cleanly while preserving caret on the same character
        let mut indented_prose = LiveDocBuffer::new("  Paragraph body".to_string(), false);
        let body_pos = indented_prose.text.find("body").unwrap();
        indented_prose.set_cursor(body_pos, false);
        indented_prose.indent_lines(true);
        assert_eq!(indented_prose.text, "Paragraph body");
        assert_eq!(&indented_prose.text[indented_prose.head..indented_prose.head + 4], "body");

        // Multi-line selection indents and outdents as a block + supports undo/redo
        let mut multi = LiveDocBuffer::new("Alpha\nBeta".to_string(), false);
        multi.set_selection(0, multi.text.len());
        multi.indent_lines(false);
        assert_eq!(multi.text, "  Alpha\n  Beta");
        multi.indent_lines(true);
        assert_eq!(multi.text, "Alpha\nBeta");
        // No-op outdent on unindented lines does not add an extra undo step
        let ver = multi.version;
        multi.indent_lines(true);
        assert_eq!(multi.version, ver);
        assert!(multi.undo());
        assert_eq!(multi.text, "  Alpha\n  Beta");
    }

    #[test]
    fn mobile_format_toolbar_actions_transform_lines_and_caret() {
        let mut buf = LiveDocBuffer::new("Hello world".to_string(), false);
        buf.set_cursor(5, false);

        assert!(buf.apply_mobile_format(MarkdownFormatAction::Heading));
        assert_eq!(buf.text, "## Hello world");
        assert!(buf.apply_mobile_format(MarkdownFormatAction::Heading));
        assert_eq!(buf.text, "### Hello world");
        assert!(buf.apply_mobile_format(MarkdownFormatAction::Heading));
        assert_eq!(buf.text, "Hello world");

        buf.set_selection(0, 5);
        assert!(buf.apply_mobile_format(MarkdownFormatAction::Bold));
        assert_eq!(buf.text, "**Hello** world");
        assert!(buf.apply_mobile_format(MarkdownFormatAction::Bold));
        assert_eq!(buf.text, "Hello world");

        buf.set_cursor(11, false);
        assert!(buf.apply_mobile_format(MarkdownFormatAction::Checkbox));
        assert_eq!(buf.text, "- [ ] Hello world");
        assert!(buf.apply_mobile_format(MarkdownFormatAction::Checkbox));
        assert_eq!(buf.text, "Hello world");

        buf.set_cursor(buf.text.len(), false);
        assert!(buf.apply_mobile_format(MarkdownFormatAction::Wikilink));
        assert_eq!(buf.text, "Hello world[[]]");
        assert_eq!(buf.head, "Hello world[[".len());
    }
}
