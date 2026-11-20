#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveBlockKind {
    Frontmatter,
    CodeFence,
    Table,
    Callout,
    Blockquote,
    MdxBlock,
    Heading(u8),
    SceneBreak,
    ImageEmbed,
    TaskItem,
    ListItem,
    Blank,
    ParagraphLine,
}

impl LiveBlockKind {
    pub fn is_multiline(self) -> bool {
        matches!(
            self,
            Self::Frontmatter
                | Self::CodeFence
                | Self::Table
                | Self::Callout
                | Self::Blockquote
                | Self::MdxBlock
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveBlock {
    pub kind: LiveBlockKind,
    pub start_line: usize,
    pub end_line: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveEnterAction {
    ClearLine,
    ExpandCodeFence {
        text: String,
        cursor_offset: usize,
    },
    SplitLine {
        left: String,
        right: String,
        right_cursor: usize,
    },
}

struct LineSpan {
    byte_start: usize,
    byte_end: usize,
    text: String,
}

/// Parses a Markdown/MDX document into Obsidian Live Preview editable units:
/// multi-line atomic blocks (frontmatter, fenced code, tables, callouts, blockquotes,
/// multi-line MDX blocks) and single-line blocks for prose, headings, lists, and embeds.
pub fn parse_live_blocks(raw: &str, is_mdx: bool) -> Vec<LiveBlock> {
    if raw.is_empty() {
        return vec![LiveBlock {
            kind: LiveBlockKind::Blank,
            start_line: 0,
            end_line: 0,
            byte_start: 0,
            byte_end: 0,
            raw: String::new(),
        }];
    }

    let mut lines = Vec::new();
    let mut cursor = 0usize;
    for chunk in raw.split('\n') {
        let clean = chunk.trim_end_matches('\r');
        let byte_start = cursor;
        let byte_end = cursor + clean.len();
        lines.push(LineSpan {
            byte_start,
            byte_end,
            text: clean.to_string(),
        });
        cursor += chunk.len() + 1;
    }

    let mut blocks = Vec::with_capacity(lines.len());
    let mut i = 0usize;

    while i < lines.len() {
        let trimmed = lines[i].text.trim();

        // 1. YAML frontmatter at line 0 when properly closed later.
        if i == 0 && trimmed == "---" {
            if let Some(close_idx) = (1..lines.len()).find(|&j| {
                let t = lines[j].text.trim();
                t == "---" || t == "..."
            }) {
                blocks.push(make_block(&lines, 0, close_idx, LiveBlockKind::Frontmatter));
                i = close_idx + 1;
                continue;
            }
        }

        // 2. Fenced code block (``` or ~~~).
        if let Some((fence_ch, fence_len)) = parse_fence_open(trimmed) {
            let mut close_idx = lines.len() - 1;
            for j in (i + 1)..lines.len() {
                if is_fence_close(lines[j].text.trim(), fence_ch, fence_len) {
                    close_idx = j;
                    break;
                }
            }
            blocks.push(make_block(&lines, i, close_idx, LiveBlockKind::CodeFence));
            i = close_idx + 1;
            continue;
        }

        // 3. Markdown GFM table (header with | followed by delimiter row).
        if lines[i].text.contains('|')
            && !trimmed.is_empty()
            && i + 1 < lines.len()
            && is_table_delimiter(&lines[i + 1].text)
        {
            let mut end_idx = i + 1;
            while end_idx + 1 < lines.len() && lines[end_idx + 1].text.contains('|') {
                end_idx += 1;
            }
            blocks.push(make_block(&lines, i, end_idx, LiveBlockKind::Table));
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
            let after_gt = trimmed.trim_start_matches('>').trim_start();
            let kind = if after_gt.starts_with("[!") && after_gt.contains(']') {
                LiveBlockKind::Callout
            } else {
                LiveBlockKind::Blockquote
            };
            blocks.push(make_block(&lines, i, end_idx, kind));
            i = end_idx + 1;
            continue;
        }

        // 5. MDX imports, exports, video/iframe, or PascalCase JSX tags.
        if is_mdx && starts_mdx_block(trimmed) {
            let end_idx = find_mdx_block_end(&lines, i);
            blocks.push(make_block(&lines, i, end_idx, LiveBlockKind::MdxBlock));
            i = end_idx + 1;
            continue;
        }

        // 6. Single-line block.
        let kind = classify_single_line(&lines[i].text, is_mdx);
        blocks.push(make_block(&lines, i, i, kind));
        i += 1;
    }

    blocks
}

fn make_block(
    lines: &[LineSpan],
    start_line: usize,
    end_line: usize,
    kind: LiveBlockKind,
) -> LiveBlock {
    let byte_start = lines[start_line].byte_start;
    let byte_end = lines[end_line].byte_end;
    let mut raw = String::new();
    for (offset, line) in lines[start_line..=end_line].iter().enumerate() {
        if offset > 0 {
            raw.push('\n');
        }
        raw.push_str(&line.text);
    }
    LiveBlock {
        kind,
        start_line,
        end_line,
        byte_start,
        byte_end,
        raw,
    }
}

pub fn classify_single_line(line: &str, is_mdx: bool) -> LiveBlockKind {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return LiveBlockKind::Blank;
    }
    if let Some(level) = parse_atx_heading_level(trimmed) {
        return LiveBlockKind::Heading(level);
    }
    if is_scene_break(trimmed) {
        return LiveBlockKind::SceneBreak;
    }
    if is_standalone_image_embed(trimmed) {
        return LiveBlockKind::ImageEmbed;
    }
    if let Some(prefix) = parse_list_or_task_prefix(line) {
        return if prefix.is_task {
            LiveBlockKind::TaskItem
        } else {
            LiveBlockKind::ListItem
        };
    }
    if is_mdx && starts_mdx_block(trimmed) {
        return LiveBlockKind::MdxBlock;
    }
    LiveBlockKind::ParagraphLine
}

fn parse_fence_open(trimmed: &str) -> Option<(char, usize)> {
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == first).count();
    if count < 3 {
        return None;
    }
    let info = &trimmed[count..];
    if first == '`' && info.contains('`') {
        return None;
    }
    Some((first, count))
}

fn is_fence_close(trimmed: &str, fence_ch: char, min_len: usize) -> bool {
    let count = trimmed.chars().take_while(|&c| c == fence_ch).count();
    count >= min_len && trimmed[count..].trim().is_empty()
}

fn is_table_delimiter(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || !t.contains('-') {
        return false;
    }
    if !t.contains('|') && !t.contains("---") {
        return false;
    }
    t.chars()
        .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

fn parse_atx_heading_level(trimmed: &str) -> Option<u8> {
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    match trimmed.as_bytes().get(hashes) {
        Some(b' ' | b'\t') => Some(hashes as u8),
        _ => None,
    }
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

fn is_standalone_image_embed(trimmed: &str) -> bool {
    if let Some(inner) = trimmed.strip_prefix("![[").and_then(|s| s.strip_suffix("]]")) {
        return !inner.is_empty() && !inner.contains("[[") && !inner.contains("]]");
    }
    if let Some(after) = trimmed.strip_prefix("![") {
        if let Some(close_bracket) = after.find("](") {
            let after_paren = &after[close_bracket + 2..];
            if let Some(close_paren) = after_paren.rfind(')') {
                return close_paren == after_paren.len() - 1;
            }
        }
    }
    false
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
    if first.is_ascii_uppercase() || name.eq_ignore_ascii_case("video") || name.eq_ignore_ascii_case("iframe") {
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

fn find_mdx_block_end(lines: &[LineSpan], start: usize) -> usize {
    let trimmed = lines[start].text.trim();
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
        let t = lines[j].text.trim();
        if t.is_empty() {
            saw_blank = true;
            continue;
        }
        if saw_blank && parse_atx_heading_level(t).is_some() {
            return start;
        }
        if t.ends_with("/>") || t.contains(&closing) {
            return j;
        }
    }
    start
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListOrTaskPrefix {
    prefix_len: usize,
    indent: String,
    is_task: bool,
    next_prefix: String,
}

fn parse_list_or_task_prefix(line: &str) -> Option<ListOrTaskPrefix> {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let indent = line[..indent_len].to_string();
    let rest = &line[indent_len..];

    let bytes = rest.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'-' | b'*' | b'+') && bytes[1] == b' ' {
        let marker = rest[..1].to_string();
        let after_marker = &rest[2..];
        if let Some(task_len) = task_marker_len(after_marker) {
            return Some(ListOrTaskPrefix {
                prefix_len: indent_len + 2 + task_len,
                indent: indent.clone(),
                is_task: true,
                next_prefix: format!("{indent}{marker} [ ] "),
            });
        }
        return Some(ListOrTaskPrefix {
            prefix_len: indent_len + 2,
            indent: indent.clone(),
            is_task: false,
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
        let delim = rest.as_bytes()[digits] as char;
        let base_len = digits + 2;
        let after_marker = &rest[base_len..];
        if let Some(task_len) = task_marker_len(after_marker) {
            return Some(ListOrTaskPrefix {
                prefix_len: indent_len + base_len + task_len,
                indent: indent.clone(),
                is_task: true,
                next_prefix: format!("{indent}{}{delim} [ ] ", num + 1),
            });
        }
        return Some(ListOrTaskPrefix {
            prefix_len: indent_len + base_len,
            indent: indent.clone(),
            is_task: false,
            next_prefix: format!("{indent}{}{delim} ", num + 1),
        });
    }

    None
}

fn task_marker_len(after_marker: &str) -> Option<usize> {
    let bytes = after_marker.as_bytes();
    if bytes.len() >= 3
        && bytes[0] == b'['
        && matches!(bytes[1], b' ' | b'x' | b'X')
        && bytes[2] == b']'
    {
        match bytes.get(3) {
            Some(b' ') => Some(4),
            None => Some(3),
            _ => None,
        }
    } else {
        None
    }
}

pub fn list_indent_px(line: &str) -> f32 {
    let mut spaces = 0usize;
    let mut tabs = 0usize;
    for b in line.bytes() {
        match b {
            b' ' => spaces += 1,
            b'\t' => tabs += 1,
            _ => break,
        }
    }
    ((spaces / 2) + tabs) as f32 * 20.0
}

pub fn clamp_to_char_boundary(s: &str, byte_idx: usize) -> usize {
    let mut idx = byte_idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

pub fn compute_smart_enter(line: &str, sel_start: usize, sel_end: usize) -> LiveEnterAction {
    let s = clamp_to_char_boundary(line, sel_start.min(sel_end));
    let e = clamp_to_char_boundary(line, sel_start.max(sel_end));
    let before = &line[..s];
    let after = &line[e..];

    let before_trim = before.trim();
    if after.trim().is_empty() {
        if let Some((fence_ch, fence_len)) = parse_fence_open(before_trim) {
            let indent = &before[..before.len() - before.trim_start().len()];
            let closing_fence: String = std::iter::repeat_n(fence_ch, fence_len).collect();
            let text = format!("{before}\n\n{indent}{closing_fence}");
            let cursor_offset = before.len() + 1;
            return LiveEnterAction::ExpandCodeFence {
                text,
                cursor_offset,
            };
        }
    }

    if let Some(prefix) = parse_list_or_task_prefix(line) {
        let remainder_after_prefix = line[prefix.prefix_len..].trim();
        if remainder_after_prefix.is_empty() && after.trim().is_empty() && s >= prefix.prefix_len
        {
            return LiveEnterAction::ClearLine;
        }
        if s >= prefix.prefix_len {
            let right = format!("{}{}", prefix.next_prefix, after);
            let right_cursor = prefix.next_prefix.len();
            return LiveEnterAction::SplitLine {
                left: before.to_string(),
                right,
                right_cursor,
            };
        }
    }

    LiveEnterAction::SplitLine {
        left: before.to_string(),
        right: after.to_string(),
        right_cursor: 0,
    }
}

/// Maps a 0-based visible column index in rendered preview text onto the corresponding
/// valid UTF-8 byte offset inside `raw_line`, skipping concealed Markdown markers.
pub fn visible_col_to_raw_byte(raw_line: &str, visible_col: usize) -> usize {
    if raw_line.is_empty() {
        return 0;
    }

    let start_byte = concealed_prefix_bytes(raw_line);
    if visible_col == 0 {
        return start_byte;
    }

    let bytes = raw_line.as_bytes();
    let mut i = start_byte;
    let mut vis = 0usize;

    while i < bytes.len() {
        // Skip inline markdown delimiter pairs: **, __, ~~, ==
        if i + 1 < bytes.len() {
            let pair = &bytes[i..i + 2];
            if matches!(pair, b"**" | b"__" | b"~~" | b"==") {
                i += 2;
                continue;
            }
            // Wikilinks: [[target|alias]] -> visible alias; [[target]] -> visible target
            if pair == b"[[" {
                if let Some(close_rel) = raw_line[i + 2..].find("]]") {
                    let inner_start = i + 2;
                    let inner_end = inner_start + close_rel;
                    let visible_part_start = raw_line[inner_start..inner_end]
                        .rfind('|')
                        .map(|p| inner_start + p + 1)
                        .unwrap_or(inner_start);
                    for (rel_off, _) in raw_line[visible_part_start..inner_end].char_indices() {
                        if vis >= visible_col {
                            return visible_part_start + rel_off;
                        }
                        vis += 1;
                    }
                    i = inner_end + 2;
                    continue;
                }
            }
        }

        // Single-character delimiters (`*`, `` ` ``).
        if matches!(bytes[i], b'*' | b'`') {
            i += 1;
            continue;
        }

        if vis >= visible_col {
            return i;
        }

        let ch = raw_line[i..].chars().next().unwrap();
        vis += 1;
        i += ch.len_utf8();
    }

    raw_line.len()
}

fn concealed_prefix_bytes(raw_line: &str) -> usize {
    let leading_ws = raw_line.len() - raw_line.trim_start().len();
    let trimmed = &raw_line[leading_ws..];
    if let Some(level) = parse_atx_heading_level(trimmed) {
        let hashes = level as usize;
        let after = &trimmed[hashes..];
        let ws = after.len() - after.trim_start_matches([' ', '\t']).len();
        return leading_ws + hashes + ws;
    }
    if let Some(after_gt) = trimmed.strip_prefix('>') {
        let ws = after_gt.len() - after_gt.trim_start_matches(' ').len();
        return leading_ws + 1 + ws.min(1);
    }
    if let Some(prefix) = parse_list_or_task_prefix(raw_line) {
        return prefix.prefix_len;
    }
    0
}

pub fn byte_offset_to_line_col(text: &str, byte_offset: usize) -> (usize, usize) {
    let clamped = clamp_to_char_boundary(text, byte_offset);
    let prefix = &text[..clamped];
    let line = prefix.bytes().filter(|&b| b == b'\n').count();
    let col = prefix
        .rfind('\n')
        .map(|pos| clamped - (pos + 1))
        .unwrap_or(clamped);
    (line, col)
}

pub fn line_col_to_byte_offset(text: &str, target_line: usize, target_byte_col: usize) -> usize {
    let mut cur_line = 0usize;
    let mut cursor = 0usize;
    for chunk in text.split('\n') {
        let clean_len = chunk.trim_end_matches('\r').len();
        if cur_line == target_line {
            let clean = &chunk[..clean_len];
            let col = clamp_to_char_boundary(clean, target_byte_col);
            return cursor + col;
        }
        cur_line += 1;
        cursor += chunk.len() + 1;
    }
    text.len()
}

pub fn splice_block_in_doc(
    doc: &str,
    byte_start: usize,
    byte_end: usize,
    replacement: &str,
) -> String {
    let start = clamp_to_char_boundary(doc, byte_start);
    let end = clamp_to_char_boundary(doc, byte_end.max(start));
    let mut out = String::with_capacity(doc.len() - (end - start) + replacement.len());
    out.push_str(&doc[..start]);
    out.push_str(replacement);
    out.push_str(&doc[end..]);
    out
}

pub fn toggle_task_in_line(line: &str, checked: bool) -> Option<String> {
    let prefix = parse_list_or_task_prefix(line)?;
    if !prefix.is_task {
        return None;
    }
    let bracket_rel = line[..prefix.prefix_len].find('[')?;
    let mut out = String::with_capacity(line.len());
    out.push_str(&line[..bracket_rel + 1]);
    out.push(if checked { 'x' } else { ' ' });
    out.push_str(&line[bracket_rel + 2..]);
    Some(out)
}

pub fn block_index_for_line(blocks: &[LiveBlock], line_0based: usize) -> usize {
    blocks
        .iter()
        .position(|b| line_0based >= b.start_line && line_0based <= b.end_line)
        .unwrap_or_else(|| blocks.len().saturating_sub(1))
}

pub fn line_has_link(raw: &str) -> bool {
    raw.contains("[[") || raw.contains("](") || raw.contains("http://") || raw.contains("https://")
}

/// Maps pointer coordinates relative to a block's top-left corner onto the matching valid UTF-8
/// byte offset inside `block.raw`, handling both multi-line blocks and soft-wrapped single lines.
pub fn click_pos_to_raw_byte(
    block: &LiveBlock,
    rel_x: f32,
    rel_y: f32,
    row_width: f32,
    avg_char_px: f32,
    line_height_px: f32,
) -> usize {
    let char_px = avg_char_px.max(1.0);
    let lh = line_height_px.max(1.0);
    if block.kind.is_multiline() {
        let clicked_line = (rel_y.max(0.0) / lh).floor() as usize;
        let max_line = block.raw.bytes().filter(|&b| b == b'\n').count();
        let target_line = clicked_line.min(max_line);
        let char_col = (rel_x.max(0.0) / char_px).round() as usize;
        let line_str = block.raw.split('\n').nth(target_line).unwrap_or("");
        let byte_col = line_str
            .char_indices()
            .nth(char_col)
            .map(|(b, _)| b)
            .unwrap_or(line_str.len());
        return line_col_to_byte_offset(&block.raw, target_line, byte_col);
    }

    let visual_row = (rel_y.max(0.0) / lh).floor() as usize;
    let chars_per_row = (row_width.max(80.0) / char_px).max(12.0);
    let approx_vis_col =
        (visual_row as f32 * chars_per_row + (rel_x.max(0.0) / char_px)).round() as usize;
    visible_col_to_raw_byte(&block.raw, approx_vis_col)
}

pub const LIVE_TAIL_SPACER_HEIGHT: f32 = 280.0;

fn wrap_lines(text: &str, avg_char_px: f32, width: f32) -> f32 {
    let chars_per_line = (width / avg_char_px.max(1.0)).max(8.0);
    let chars = text.chars().count().max(1) as f32;
    (chars / chars_per_line).ceil().max(1.0)
}

/// Estimates the rendered height (in logical pixels) of a single `LiveBlock` for the given
/// text content width so off-screen items contribute realistic height from frame 1.
pub fn estimate_live_block_height(block: &LiveBlock, content_width: f32) -> f32 {
    let w = content_width.max(120.0);
    match block.kind {
        LiveBlockKind::Blank => 24.75,
        LiveBlockKind::SceneBreak => 40.0,
        LiveBlockKind::ImageEmbed => 240.0,
        LiveBlockKind::Heading(1) => {
            let trimmed = block.raw.trim_start_matches('#').trim();
            wrap_lines(trimmed, 14.5, w) * 34.0 + 16.0
        }
        LiveBlockKind::Heading(2) => {
            let trimmed = block.raw.trim_start_matches('#').trim();
            wrap_lines(trimmed, 11.0, w) * 27.0 + 13.0
        }
        LiveBlockKind::Heading(3) => {
            let trimmed = block.raw.trim_start_matches('#').trim();
            wrap_lines(trimmed, 9.5, w) * 23.0 + 10.0
        }
        LiveBlockKind::Heading(_) => {
            let trimmed = block.raw.trim_start_matches('#').trim();
            wrap_lines(trimmed, 8.2, w) * 20.5 + 10.0
        }
        LiveBlockKind::ParagraphLine => wrap_lines(&block.raw, 8.0, w) * 24.75 + 3.0,
        LiveBlockKind::ListItem | LiveBlockKind::TaskItem => {
            let avail = (w - (list_indent_px(&block.raw) + 24.0)).max(120.0);
            wrap_lines(block.raw.trim_start(), 8.0, avail) * 24.75 + 3.0
        }
        LiveBlockKind::CodeFence | LiveBlockKind::Frontmatter | LiveBlockKind::MdxBlock => {
            block.raw.lines().count().max(1) as f32 * 20.25 + 36.0
        }
        LiveBlockKind::Table => {
            block.raw.lines().count().saturating_sub(1).max(1) as f32 * 34.0 + 16.0
        }
        LiveBlockKind::Callout | LiveBlockKind::Blockquote => {
            let inner_w = (w - 32.0).max(120.0);
            let lines: f32 = block
                .raw
                .lines()
                .map(|line| {
                    let stripped = line.trim_start().trim_start_matches('>').trim_start();
                    wrap_lines(stripped, 8.0, inner_w)
                })
                .sum::<f32>()
                .max(1.0);
            lines * 24.75 + 24.0
        }
    }
}

/// Cumulative height model for Live Preview blocks plus the trailing spacer row, blending
/// width-aware estimates for off-screen rows with exact measured heights once laid out.
#[derive(Debug, Clone)]
pub struct LiveHeightIndex {
    heights: Vec<f32>,
    is_measured: Vec<bool>,
    prefix_sums: Vec<f32>,
    content_width: f32,
}

impl LiveHeightIndex {
    pub fn new(blocks: &[LiveBlock], content_width: f32) -> Self {
        let mut idx = Self {
            heights: Vec::new(),
            is_measured: Vec::new(),
            prefix_sums: Vec::new(),
            content_width,
        };
        idx.rebuild(blocks, content_width);
        idx
    }

    pub fn item_count(&self) -> usize {
        self.heights.len()
    }

    pub fn total_height(&self) -> f32 {
        self.prefix_sums.last().copied().unwrap_or(0.0)
    }

    pub fn content_width(&self) -> f32 {
        self.content_width
    }

    pub fn rebuild(&mut self, blocks: &[LiveBlock], content_width: f32) {
        self.content_width = content_width;
        let n = blocks.len() + 1;
        self.heights.clear();
        self.heights.reserve(n);
        self.is_measured.clear();
        self.is_measured.reserve(n);
        for b in blocks {
            self.heights.push(estimate_live_block_height(b, content_width));
            self.is_measured.push(false);
        }
        self.heights.push(LIVE_TAIL_SPACER_HEIGHT);
        self.is_measured.push(true);
        self.recompute_prefix_from(0);
    }

    pub fn splice_blocks(
        &mut self,
        old_range: std::ops::Range<usize>,
        replacement: &[LiveBlock],
    ) {
        let blocks_len = self.heights.len().saturating_sub(1);
        let start = old_range.start.min(blocks_len);
        let end = old_range.end.clamp(start, blocks_len);
        let rep_heights: Vec<f32> = replacement
            .iter()
            .map(|b| estimate_live_block_height(b, self.content_width))
            .collect();
        let rep_measured = vec![false; replacement.len()];
        self.heights.splice(start..end, rep_heights);
        self.is_measured.splice(start..end, rep_measured);
        self.recompute_prefix_from(start);
    }

    pub fn update_measured_batch(&mut self, updates: &[(usize, f32)]) -> bool {
        let mut first_changed: Option<usize> = None;
        for &(ix, h) in updates {
            if h <= 1.0 || ix >= self.heights.len() {
                continue;
            }
            self.is_measured[ix] = true;
            if (self.heights[ix] - h).abs() > 0.05 {
                self.heights[ix] = h;
                first_changed = Some(first_changed.map_or(ix, |m| m.min(ix)));
            }
        }
        if let Some(start) = first_changed {
            self.recompute_prefix_from(start);
            true
        } else {
            false
        }
    }

    pub fn y_to_item_offset(&self, y: f32) -> (usize, f32) {
        if self.heights.is_empty() {
            return (0, 0.0);
        }
        let y = y.clamp(0.0, self.total_height());
        let ix = self
            .prefix_sums
            .partition_point(|&top| top <= y)
            .saturating_sub(1)
            .min(self.heights.len() - 1);
        let raw_off = (y - self.prefix_sums[ix]).max(0.0);
        let max_off = if self.is_measured[ix] {
            (self.heights[ix] - 0.5).max(0.0)
        } else {
            (self.heights[ix] * 0.75).max(0.0)
        };
        (ix, raw_off.min(max_off))
    }

    pub fn scroll_top_to_y(&self, item_ix: usize, offset_in_item: f32, viewport_height: f32) -> f32 {
        let max_y = (self.total_height() - viewport_height).max(0.0);
        if item_ix >= self.heights.len() {
            return max_y;
        }
        (self.prefix_sums[item_ix] + offset_in_item).clamp(0.0, max_y)
    }

    pub fn item_range(&self, ix: usize) -> Option<(f32, f32)> {
        if ix < self.heights.len() {
            Some((self.prefix_sums[ix], self.prefix_sums[ix + 1]))
        } else {
            None
        }
    }

    fn recompute_prefix_from(&mut self, start: usize) {
        self.prefix_sums.resize(self.heights.len() + 1, 0.0);
        self.prefix_sums[0] = 0.0;
        let from = start.min(self.heights.len());
        for i in from..self.heights.len() {
            self.prefix_sums[i + 1] = self.prefix_sums[i] + self.heights[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_markdown_and_mdx_into_exact_contiguous_blocks() {
        let raw = "\
---
title: Demo
---

# Heading One

Paragraph line with **bold** and ==mark==.
- [ ] First task
  - Nested item

```rust
fn main() {
    println!(\"hi\");
}
```

| Col A | Col B |
| --- | --- |
| 1 | 2 |

> [!note] Callout
> Line two

<ModelViewer
  src=\"/foo.glb\"
/>
***
![[pic.png|400|center]]
";
        let blocks = parse_live_blocks(raw, true);
        let kinds: Vec<LiveBlockKind> = blocks.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            vec![
                LiveBlockKind::Frontmatter,
                LiveBlockKind::Blank,
                LiveBlockKind::Heading(1),
                LiveBlockKind::Blank,
                LiveBlockKind::ParagraphLine,
                LiveBlockKind::TaskItem,
                LiveBlockKind::ListItem,
                LiveBlockKind::Blank,
                LiveBlockKind::CodeFence,
                LiveBlockKind::Blank,
                LiveBlockKind::Table,
                LiveBlockKind::Blank,
                LiveBlockKind::Callout,
                LiveBlockKind::Blank,
                LiveBlockKind::MdxBlock,
                LiveBlockKind::SceneBreak,
                LiveBlockKind::ImageEmbed,
                LiveBlockKind::Blank,
            ]
        );

        // Byte spans roundtrip identically to slice content & lines cover contiguously
        let mut expected_line = 0usize;
        for b in &blocks {
            assert_eq!(b.start_line, expected_line);
            assert_eq!(b.raw, raw[b.byte_start..b.byte_end].replace('\r', ""));
            expected_line = b.end_line + 1;
        }
    }

    #[test]
    fn handles_unclosed_frontmatter_and_mdx_gracefully() {
        let raw = "---\nThis is prose after thematic break.";
        let blocks = parse_live_blocks(raw, false);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, LiveBlockKind::SceneBreak);
        assert_eq!(blocks[1].kind, LiveBlockKind::ParagraphLine);

        let mdx = "<ModelViewer\nline without close for a bit\n\n# Real Heading\nProse";
        let mdx_blocks = parse_live_blocks(mdx, true);
        assert!(mdx_blocks.iter().any(|b| b.kind == LiveBlockKind::Heading(1)));
    }

    #[test]
    fn smart_enter_splits_lists_tasks_increments_ordered_and_expands_fences() {
        assert_eq!(
            compute_smart_enter("- hello world", 7, 7),
            LiveEnterAction::SplitLine {
                left: "- hello".to_string(),
                right: "-  world".to_string(),
                right_cursor: 2,
            }
        );

        assert_eq!(
            compute_smart_enter("  - [x] Done step", 17, 17),
            LiveEnterAction::SplitLine {
                left: "  - [x] Done step".to_string(),
                right: "  - [ ] ".to_string(),
                right_cursor: 8,
            }
        );

        assert_eq!(
            compute_smart_enter("1. First item", 13, 13),
            LiveEnterAction::SplitLine {
                left: "1. First item".to_string(),
                right: "2. ".to_string(),
                right_cursor: 3,
            }
        );

        assert_eq!(compute_smart_enter("  - [ ] ", 8, 8), LiveEnterAction::ClearLine);
        assert_eq!(compute_smart_enter("```rust", 7, 7), LiveEnterAction::ExpandCodeFence {
            text: "```rust\n\n```".to_string(),
            cursor_offset: 8,
        });
    }

    #[test]
    fn visible_col_to_raw_byte_skips_concealed_syntax_and_handles_cyrillic() {
        let heading = "## Hello **world**";
        assert_eq!(visible_col_to_raw_byte(heading, 0), 3); // 'H'
        assert_eq!(visible_col_to_raw_byte(heading, 6), 11); // 'w' inside **world**

        let wiki = "See [[Some Long Target|Alias]] now";
        assert_eq!(visible_col_to_raw_byte(wiki, 4), 23); // 'A' of Alias

        let cyrillic = "Привет **мир**";
        let idx = visible_col_to_raw_byte(cyrillic, 7); // after "Привет " + "**" -> 'м'
        assert!(cyrillic.is_char_boundary(idx));
        assert_eq!(&cyrillic[idx..], "мир**");
    }

    #[test]
    fn toggle_task_in_line_flips_checkbox_in_place() {
        assert_eq!(
            toggle_task_in_line("  - [ ] Write chapter", true).as_deref(),
            Some("  - [x] Write chapter")
        );
        assert_eq!(
            toggle_task_in_line("1. [X] Done", false).as_deref(),
            Some("1. [ ] Done")
        );
        assert_eq!(toggle_task_in_line("- Plain bullet", true), None);
    }

    #[test]
    fn click_pos_to_raw_byte_maps_wrapped_paragraphs_and_multiline_blocks() {
        let para_text = "First visual row of text continues onto second visual row and reaches third visual row easily here.";
        let para = LiveBlock {
            kind: LiveBlockKind::ParagraphLine,
            start_line: 0,
            end_line: 0,
            byte_start: 0,
            byte_end: para_text.len(),
            raw: para_text.to_string(),
        };
        let off_row0 = click_pos_to_raw_byte(&para, 40.0, 5.0, 240.0, 8.0, 24.75);
        let off_row2 = click_pos_to_raw_byte(&para, 40.0, 55.0, 240.0, 8.0, 24.75);
        assert!(off_row0 < 15);
        assert!(off_row2 > 45 && off_row2 <= para.raw.len());
        assert!(para.raw.is_char_boundary(off_row2));

        let code_text = "```rust\nlet alpha = 1;\nlet привет = 2;\n```";
        let code = LiveBlock {
            kind: LiveBlockKind::CodeFence,
            start_line: 0,
            end_line: 3,
            byte_start: 0,
            byte_end: code_text.len(),
            raw: code_text.to_string(),
        };
        let off_line2 = click_pos_to_raw_byte(&code, 48.0, 45.0, 600.0, 8.0, 20.25);
        assert!(code.raw.is_char_boundary(off_line2));
        assert!(code.raw[off_line2..].starts_with("ивет") || code.raw[off_line2..].contains("= 2"));
    }

    #[test]
    fn live_height_index_estimates_large_doc_and_maps_positions() {
        let blocks: Vec<LiveBlock> = (0..5000)
            .map(|i| LiveBlock {
                kind: LiveBlockKind::ParagraphLine,
                start_line: i,
                end_line: i,
                byte_start: 0,
                byte_end: 40,
                raw: "Standard paragraph line of ~forty chars".to_string(),
            })
            .collect();
        let idx = LiveHeightIndex::new(&blocks, 760.0);
        assert_eq!(idx.item_count(), 5001);
        assert!(idx.total_height() > 130_000.0 && idx.total_height() < 200_000.0);

        let (top_ix, top_off) = idx.y_to_item_offset(0.0);
        assert_eq!(top_ix, 0);
        assert_eq!(top_off, 0.0);

        let mid_y = idx.total_height() * 0.5;
        let (mid_ix, _) = idx.y_to_item_offset(mid_y);
        assert!(mid_ix > 2400 && mid_ix < 2600);

        let max_y = idx.total_height() - 800.0;
        let (bot_ix, _) = idx.y_to_item_offset(max_y);
        assert!(bot_ix >= 4960 && bot_ix <= 5000);
    }

    #[test]
    fn live_height_index_measured_batch_updates_prefix_sums_and_bottom_scroll() {
        let blocks: Vec<LiveBlock> = (0..10)
            .map(|i| LiveBlock {
                kind: LiveBlockKind::ParagraphLine,
                start_line: i,
                end_line: i,
                byte_start: 0,
                byte_end: 10,
                raw: "Short line".to_string(),
            })
            .collect();
        let mut idx = LiveHeightIndex::new(&blocks, 760.0);
        assert!(idx.update_measured_batch(&[(0, 50.0), (1, 100.0), (99, 500.0), (2, 0.5)]));
        assert!(!idx.update_measured_batch(&[(0, 50.0), (1, 100.0)]));

        let vp_h = 300.0;
        let expected_max = idx.total_height() - vp_h;
        assert!((idx.scroll_top_to_y(idx.item_count(), 0.0, vp_h) - expected_max).abs() < 0.01);
        assert!((idx.scroll_top_to_y(0, 12.0, vp_h) - 12.0).abs() < 0.01);
        assert!((idx.scroll_top_to_y(1, 0.0, vp_h) - 50.0).abs() < 0.01);
        assert!((idx.scroll_top_to_y(2, 0.0, vp_h) - 150.0).abs() < 0.01);
    }

    #[test]
    fn live_height_index_splice_preserves_measured_outside_range() {
        let blocks: Vec<LiveBlock> = (0..5)
            .map(|i| LiveBlock {
                kind: LiveBlockKind::ParagraphLine,
                start_line: i,
                end_line: i,
                byte_start: 0,
                byte_end: 4,
                raw: "Line".to_string(),
            })
            .collect();
        let mut idx = LiveHeightIndex::new(&blocks, 760.0);
        idx.update_measured_batch(&[(0, 42.0), (4, 64.0)]);
        let rep = vec![
            LiveBlock {
                kind: LiveBlockKind::Heading(1),
                start_line: 1,
                end_line: 1,
                byte_start: 0,
                byte_end: 5,
                raw: "# Big".to_string(),
            },
            LiveBlock {
                kind: LiveBlockKind::ParagraphLine,
                start_line: 2,
                end_line: 2,
                byte_start: 0,
                byte_end: 3,
                raw: "Two".to_string(),
            },
        ];
        idx.splice_blocks(1..3, &rep);
        assert_eq!(idx.item_count(), 6);
        assert!(idx.is_measured[0]);
        assert!((idx.heights[0] - 42.0).abs() < 0.01);
        assert!(!idx.is_measured[1]);
        assert!(idx.is_measured[4]);
        assert!((idx.heights[4] - 64.0).abs() < 0.01);
        assert!(idx.is_measured[5]);
        assert!((idx.heights[5] - LIVE_TAIL_SPACER_HEIGHT).abs() < 0.01);

        let (t0, b0) = idx.item_range(0).unwrap();
        assert!((t0 - 0.0).abs() < 0.01 && (b0 - 42.0).abs() < 0.01);
        let (t4, b4) = idx.item_range(4).unwrap();
        assert!((b4 - t4 - 64.0).abs() < 0.01);
        assert!(idx.item_range(99).is_none());
    }

    #[test]
    fn live_height_index_cyrillic_uses_char_count() {
        let latin = LiveBlock {
            kind: LiveBlockKind::ParagraphLine,
            start_line: 0,
            end_line: 0,
            byte_start: 0,
            byte_end: 100,
            raw: "a".repeat(100),
        };
        let cyrillic = LiveBlock {
            kind: LiveBlockKind::ParagraphLine,
            start_line: 0,
            end_line: 0,
            byte_start: 0,
            byte_end: 200,
            raw: "а".repeat(100),
        };
        let h_latin = estimate_live_block_height(&latin, 400.0);
        let h_cyr = estimate_live_block_height(&cyrillic, 400.0);
        assert!((h_latin - h_cyr).abs() < 0.01);
    }
}
