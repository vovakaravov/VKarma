use crate::live_md::{
    DocLine, EditKind, InlineStyle, LineBlockContext, LineHeightTable, LinkTarget, LiveDocBuffer,
    OffsetSegment, PrefixVisual, ProjectedLine, TableAlign, clamp_char_boundary,
    fnv1a_line_ctx_hash, list_indent_px_from_cols, parse_table_alignments, project_line,
    project_table_cells, utf8_to_utf16_offset, utf16_to_utf8_offset,
};
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use crate::vault::MarkdownFormatAction;
use gpui::{prelude::*, *};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarHandle};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

const MOBILE_MAX_COL_WIDTH: f32 = 740.0;
const MOBILE_SIDE_PAD: f32 = 16.0;
const MOBILE_TOP_PAD: f32 = 12.0;
const MOBILE_BOTTOM_SCROLL_PAD: f32 = 140.0;
const TABLE_CELL_PAD_X: f32 = 10.0;
const TABLE_FONT_SIZE: f32 = 13.5;
const TABLE_LINE_HEIGHT: f32 = 20.0;

pub enum LiveEditorEvent {
    Changed,
    OpenLink(LinkTarget),
    FocusChanged(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionHandle {
    Start,
    End,
}

struct LiveScrollState {
    scroll_y: f32,
    total_height: f32,
    viewport_bounds: Bounds<Pixels>,
    drag_total_height: Option<f32>,
}

#[derive(Clone)]
struct LiveScrollbarHandle {
    state: Rc<RefCell<LiveScrollState>>,
}

impl LiveScrollbarHandle {
    fn new() -> (Self, Rc<RefCell<LiveScrollState>>) {
        let state = Rc::new(RefCell::new(LiveScrollState {
            scroll_y: 0.0,
            total_height: 0.0,
            viewport_bounds: Bounds::default(),
            drag_total_height: None,
        }));
        (
            Self {
                state: Rc::clone(&state),
            },
            state,
        )
    }

    fn max_scroll_y(st: &LiveScrollState) -> f32 {
        let vp_h = f32::from(st.viewport_bounds.size.height);
        (st.total_height - vp_h).max(0.0)
    }

    fn clamp_scroll(st: &mut LiveScrollState) {
        let max_y = Self::max_scroll_y(st);
        st.scroll_y = st.scroll_y.clamp(0.0, max_y);
    }
}

impl ScrollbarHandle for LiveScrollbarHandle {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.state.borrow().viewport_bounds
    }

    fn offset(&self) -> Point<Pixels> {
        let st = self.state.borrow();
        let live_max = Self::max_scroll_y(&st);
        if live_max <= 0.0 {
            return point(px(0.), px(0.));
        }
        let reported_y = if let Some(frozen_total) = st.drag_total_height {
            let vp_h = f32::from(st.viewport_bounds.size.height);
            let frozen_max = (frozen_total - vp_h).max(0.0);
            if frozen_max <= 0.0 {
                0.0
            } else {
                (st.scroll_y / live_max).clamp(0.0, 1.0) * frozen_max
            }
        } else {
            st.scroll_y
        };
        point(px(0.), -px(reported_y))
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        let mut st = self.state.borrow_mut();
        let live_max = Self::max_scroll_y(&st);
        let vp_h = f32::from(st.viewport_bounds.size.height);
        let active_total = st.drag_total_height.unwrap_or(st.total_height);
        let active_max = (active_total - vp_h).max(0.0);
        if live_max > 0.0 && active_max > 0.0 {
            let req = (-f32::from(offset.y)).clamp(0.0, active_max);
            st.scroll_y = (req / active_max) * live_max;
        }
    }

    fn content_size(&self) -> Size<Pixels> {
        let st = self.state.borrow();
        let total = st.drag_total_height.unwrap_or(st.total_height);
        size(st.viewport_bounds.size.width, px(total))
    }

    fn start_drag(&self) {
        let mut st = self.state.borrow_mut();
        st.drag_total_height = Some(st.total_height);
    }

    fn end_drag(&self) {
        self.state.borrow_mut().drag_total_height = None;
    }
}

struct CachedTableCell {
    x_rel: f32,
    width: f32,
    align: TableAlign,
    raw_offset: usize,
    inner_range: Option<Range<usize>>,
    projected: ProjectedLine,
    shaped: Option<WrappedLine>,
}

struct CachedTableRow {
    table_width: f32,
    frame_top_margin: f32,
    frame_bottom_margin: f32,
    cell_pad_y: f32,
    is_visual_last: bool,
    active_col: Option<usize>,
    cells: Vec<CachedTableCell>,
}

#[derive(Clone)]
struct TableBlockLayout {
    num_cols: usize,
    col_widths: Vec<f32>,
    alignments: Vec<TableAlign>,
    total_width: f32,
    delim_line: usize,
    table_end_line: usize,
}

struct CachedLine {
    doc_version: u64,
    raw_hash: u64,
    source_mode: bool,
    cursor_key: Option<(usize, usize)>,
    wrap_width: f32,
    raw_len: usize,
    table_active: bool,
    delim_expanded: bool,
    ctx: LineBlockContext,
    projected: ProjectedLine,
    shaped: Option<WrappedLine>,
    table_row: Option<CachedTableRow>,
    line_height: f32,
    total_height: f32,
    text_x_rel: f32,
    text_y_rel: f32,
}

struct HitLinkInfo {
    target: LinkTarget,
    concealed: bool,
}

struct HitTestResult {
    raw_byte: usize,
    line_ix: usize,
    hit_link: Option<HitLinkInfo>,
    clicked_checkbox: bool,
    phantom_table_cell: Option<(usize, usize)>,
}

fn compute_column_geometry(vp_w: f32) -> (f32, f32, f32) {
    let col_width = (vp_w - MOBILE_SIDE_PAD * 2.0).clamp(160.0, MOBILE_MAX_COL_WIDTH);
    let col_left_rel = ((vp_w - col_width) * 0.5).max(MOBILE_SIDE_PAD * 0.5);
    (col_left_rel, col_width, MOBILE_TOP_PAD)
}

fn block_font_and_line_height(ctx: &LineBlockContext) -> (f32, f32) {
    match ctx {
        LineBlockContext::Heading { level, .. } => match *level {
            1 => (24.0, 33.0),
            2 => (20.5, 29.0),
            3 => (17.5, 25.0),
            _ => (16.0, 23.0),
        },
        LineBlockContext::CodeFence { .. } => (13.5, 20.5),
        LineBlockContext::Frontmatter { .. } => (13.0, 19.5),
        LineBlockContext::Table { .. } => (TABLE_FONT_SIZE, TABLE_LINE_HEIGHT),
        _ => (15.5, 24.5),
    }
}

fn gpui_text_align(align: TableAlign) -> TextAlign {
    match align {
        TableAlign::Left => TextAlign::Left,
        TableAlign::Center => TextAlign::Center,
        TableAlign::Right => TextAlign::Right,
    }
}

fn run_colors_from_style(style: InlineStyle, ctx: &LineBlockContext) -> Hsla {
    if style.syntax_muted {
        return hex_color(kd::FG_MUTED);
    }
    if style.checked_task {
        return hex_color(kd::FG_MUTED);
    }
    if style.wikilink || style.link {
        return hex_color(kd::LINK);
    }
    if style.callout_title {
        return hex_color(kd::ACCENT_HOVER);
    }
    if style.code {
        return hex_color(kd::AQUA);
    }
    if style.bullet_marker {
        return hex_color(kd::ORANGE);
    }
    if style.heading_level > 0 {
        return match style.heading_level {
            1 => hex_color(kd::ACCENT_HOVER),
            2 => hex_color(kd::BLUE),
            3 => hex_color(kd::AQUA),
            _ => hex_color(kd::FG),
        };
    }
    if style.dim || matches!(ctx, LineBlockContext::Frontmatter { .. }) {
        return hex_color(kd::FG_SUBTLE);
    }
    if matches!(ctx, LineBlockContext::Blockquote { .. }) {
        return hex_color(kd::FG_SUBTLE);
    }
    hex_color(kd::FG)
}

fn build_text_runs(
    projected: &ProjectedLine,
    ctx: &LineBlockContext,
    prose_font: &Font,
    mono_font: &Font,
) -> Vec<TextRun> {
    if projected.display_text.is_empty() {
        return Vec::new();
    }
    if projected.spans.is_empty() {
        let mut f = prose_font.clone();
        if matches!(ctx, LineBlockContext::Heading { .. }) {
            f.weight = FontWeight::BOLD;
        }
        return vec![TextRun {
            len: projected.display_text.len(),
            font: f,
            color: hex_color(kd::FG),
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
    }

    projected
        .spans
        .iter()
        .filter(|s| s.range.start < s.range.end)
        .map(|span| {
            let st = span.style;
            let use_mono = st.monospace || st.code;
            let mut font = if use_mono {
                mono_font.clone()
            } else {
                prose_font.clone()
            };
            if st.bold || st.heading_level > 0 || st.callout_title {
                font.weight = FontWeight::BOLD;
            }
            if st.italic {
                font.style = FontStyle::Italic;
            }
            let bg = if st.highlight {
                Some(hex_alpha(0xc4b28a40))
            } else if st.code && !matches!(ctx, LineBlockContext::CodeFence { .. }) {
                Some(hex_color(kd::BG_PANEL))
            } else {
                None
            };
            let underline = if st.wikilink || st.link {
                Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(hex_color(kd::LINK)),
                    wavy: false,
                })
            } else {
                None
            };
            let strikethrough = if st.strikethrough || st.checked_task {
                Some(StrikethroughStyle {
                    thickness: px(1.2),
                    color: Some(hex_color(kd::FG_MUTED)),
                })
            } else {
                None
            };
            TextRun {
                len: span.range.end - span.range.start,
                font,
                color: run_colors_from_style(st, ctx),
                background_color: bg,
                underline,
                strikethrough,
            }
        })
        .collect()
}

fn cursor_range_for_line(
    doc: &LiveDocBuffer,
    dl: &DocLine,
    focused: bool,
) -> Option<Range<usize>> {
    if !focused {
        return None;
    }
    let sel = doc.selection_range();
    let (s, e) = if sel.start == sel.end {
        (doc.head, doc.head)
    } else {
        (sel.start, sel.end)
    };
    if e < dl.byte_start || s > dl.byte_end {
        return None;
    }
    let local_s = s.saturating_sub(dl.byte_start).min(dl.byte_end - dl.byte_start);
    let local_e = e.saturating_sub(dl.byte_start).min(dl.byte_end - dl.byte_start);
    Some(local_s..local_e)
}

fn disp_position_for_index(
    shaped: Option<&WrappedLine>,
    disp_len: usize,
    disp_idx: usize,
    line_height: Pixels,
) -> Point<Pixels> {
    let Some(wl) = shaped else {
        return point(px(0.), px(0.));
    };
    let clamped = disp_idx.min(disp_len);
    wl.position_for_index(clamped, line_height)
        .unwrap_or_else(|| point(px(0.), px(0.)))
}

fn row_right_x(shaped: Option<&WrappedLine>, row: usize) -> f32 {
    let Some(wl) = shaped else {
        return 0.0;
    };
    let unwrapped = &wl.unwrapped_layout;
    let total_rows = wl.wrap_boundaries.len() + 1;
    let r = row.min(total_rows.saturating_sub(1));
    let row_start_idx = if r == 0 {
        0
    } else {
        let b = wl.wrap_boundaries[r - 1];
        unwrapped.runs[b.run_ix].glyphs[b.glyph_ix].index
    };
    let row_end_x = if r + 1 < total_rows {
        let b = wl.wrap_boundaries[r];
        let g = &unwrapped.runs[b.run_ix].glyphs[b.glyph_ix];
        f32::from(g.position.x)
    } else {
        f32::from(unwrapped.width)
    };
    let row_start_x = f32::from(unwrapped.x_for_index(row_start_idx));
    (row_end_x - row_start_x).max(0.0)
}

fn row_align_offset_x(
    shaped: Option<&WrappedLine>,
    row: usize,
    align: TextAlign,
    align_width: f32,
) -> f32 {
    match align {
        TextAlign::Left => 0.0,
        TextAlign::Center => ((align_width - row_right_x(shaped, row)) * 0.5).max(0.0),
        TextAlign::Right => (align_width - row_right_x(shaped, row)).max(0.0),
    }
}

fn find_table_cell_for_local_raw(tr: &CachedTableRow, local_raw: usize) -> Option<usize> {
    for (ix, cell) in tr.cells.iter().enumerate() {
        if let Some(ref inner) = cell.inner_range
            && ((local_raw >= inner.start && local_raw <= inner.end) || local_raw < inner.start)
        {
            return Some(ix);
        }
    }
    tr.active_col
        .filter(|&c| {
            tr.cells
                .get(c)
                .is_some_and(|cell| cell.inner_range.is_some())
        })
        .or_else(|| tr.cells.iter().rposition(|c| c.inner_range.is_some()))
}

fn raw_offset_to_visual_point(c: &CachedLine, local_raw: usize) -> (f32, f32, f32) {
    if let Some(tr) = &c.table_row
        && let Some(cix) = find_table_cell_for_local_raw(tr, local_raw)
    {
        let cell = &tr.cells[cix];
        let cell_local = local_raw
            .saturating_sub(cell.raw_offset)
            .min(cell.projected.raw_len);
        let disp = cell.projected.raw_to_display(cell_local);
        let pt = disp_position_for_index(
            cell.shaped.as_ref(),
            cell.projected.display_text.len(),
            disp,
            px(c.line_height),
        );
        let wrap_row = (f32::from(pt.y) / c.line_height).round().max(0.0) as usize;
        let inner_w = (cell.width - 2.0 * TABLE_CELL_PAD_X).max(20.0);
        let align_dx = row_align_offset_x(
            cell.shaped.as_ref(),
            wrap_row,
            gpui_text_align(cell.align),
            inner_w,
        );
        return (
            cell.x_rel + TABLE_CELL_PAD_X + align_dx + f32::from(pt.x),
            tr.frame_top_margin + tr.cell_pad_y + f32::from(pt.y),
            c.line_height,
        );
    }

    let disp = c.projected.raw_to_display(local_raw);
    let pt = disp_position_for_index(
        c.shaped.as_ref(),
        c.projected.display_text.len(),
        disp,
        px(c.line_height),
    );
    (
        c.text_x_rel + f32::from(pt.x),
        c.text_y_rel + f32::from(pt.y),
        c.line_height,
    )
}

fn table_state_for_line(
    doc: &LiveDocBuffer,
    ctx: &LineBlockContext,
    focused: bool,
) -> (Option<(usize, usize)>, bool, bool) {
    let LineBlockContext::Table {
        table_start_line,
        table_end_line,
        ..
    } = *ctx
    else {
        return (None, false, false);
    };
    let active = if focused {
        let cur_li = doc.line_index_for_byte(doc.head);
        let sel = doc.selection_range();
        let s_li = doc.line_index_for_byte(sel.start);
        let e_li = doc.line_index_for_byte(sel.end);
        (cur_li >= table_start_line && cur_li <= table_end_line)
            || (e_li >= table_start_line && s_li <= table_end_line)
    } else {
        false
    };
    let delim_li = table_start_line + 1;
    let delim_expanded = if focused {
        doc.lines
            .get(delim_li)
            .is_some_and(|dl| cursor_range_for_line(doc, dl, true).is_some())
    } else {
        false
    };
    (Some((table_start_line, table_end_line)), active, delim_expanded)
}

fn compute_table_layout(
    doc: &LiveDocBuffer,
    table_start_line: usize,
    table_end_line: usize,
    col_width: f32,
) -> TableBlockLayout {
    let delim_line = (table_start_line + 1).min(table_end_line);
    let alignments = parse_table_alignments(doc.line_str(delim_line));
    let mut num_cols = alignments.len().max(1);
    let mut max_chars = vec![4usize; num_cols];

    for li in table_start_line..=table_end_line {
        if li == delim_line {
            continue;
        }
        let raw = doc.line_str(li);
        let cells = crate::live_md::parse_table_row_cells(raw);
        if cells.len() > num_cols {
            num_cols = cells.len();
            max_chars.resize(num_cols, 4);
        }
        for (cix, cell) in cells.iter().enumerate() {
            let trimmed = raw[cell.inner_range.clone()].trim();
            max_chars[cix] = max_chars[cix].max(trimmed.chars().count().min(36));
        }
    }

    let min_col = 54.0_f32;
    let weights: Vec<f32> = max_chars
        .iter()
        .map(|&n| (n as f32).sqrt().max(2.0))
        .collect();
    let sum_w: f32 = weights.iter().sum::<f32>().max(1.0);
    let avail = col_width.max(min_col * num_cols as f32);
    let mut col_widths: Vec<f32> = weights
        .iter()
        .map(|&w| ((avail * (w / sum_w)).max(min_col)).floor())
        .collect();
    let used: f32 = col_widths.iter().sum();
    if let Some(last) = col_widths.last_mut() {
        *last = (*last + (avail - used)).max(min_col);
    }
    let total_width = col_widths.iter().sum();

    TableBlockLayout {
        num_cols,
        col_widths,
        alignments,
        total_width,
        delim_line,
        table_end_line,
    }
}

#[allow(clippy::too_many_arguments)]
fn shape_cached_line(
    dl: &DocLine,
    raw_line: &str,
    cursor_local: Option<Range<usize>>,
    col_width: f32,
    doc_version: u64,
    source_mode: bool,
    table_layout: Option<&TableBlockLayout>,
    table_active: bool,
    delim_expanded: bool,
    prose_font: &Font,
    mono_font: &Font,
    window: &Window,
) -> CachedLine {
    let cursor_key = cursor_local.as_ref().map(|r| (r.start, r.end));
    let raw_hash = fnv1a_line_ctx_hash(raw_line, &dl.ctx);

    if source_mode {
        let projected = ProjectedLine {
            display_text: raw_line.to_string(),
            spans: Vec::new(),
            segments: if raw_line.is_empty() {
                Vec::new()
            } else {
                vec![OffsetSegment::Verbatim {
                    raw: 0..raw_line.len(),
                    disp: 0..raw_line.len(),
                }]
            },
            links: Vec::new(),
            prefix_visual: PrefixVisual::None,
            raw_len: raw_line.len(),
        };
        let avail_w = (col_width - 4.0).max(100.0);
        let shaped = if projected.display_text.is_empty() {
            None
        } else {
            let color = match &dl.ctx {
                LineBlockContext::Heading { level: 1, .. } => hex_color(kd::ACCENT_HOVER),
                LineBlockContext::Heading { level: 2, .. } => hex_color(kd::BLUE),
                LineBlockContext::Heading { .. } => hex_color(kd::AQUA),
                LineBlockContext::CodeFence { .. }
                | LineBlockContext::Frontmatter { .. }
                | LineBlockContext::Blockquote { .. }
                | LineBlockContext::Callout { .. } => hex_color(kd::FG_SUBTLE),
                _ => hex_color(kd::FG),
            };
            let mut font = mono_font.clone();
            if matches!(dl.ctx, LineBlockContext::Heading { .. }) {
                font.weight = FontWeight::BOLD;
            }
            let run = TextRun {
                len: projected.display_text.len(),
                font,
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shared: SharedString = projected.display_text.clone().into();
            window
                .text_system()
                .shape_text(shared, px(13.5), &[run], Some(px(avail_w)), None)
                .ok()
                .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) })
        };
        let line_height = 21.5;
        let wrap_rows = shaped
            .as_ref()
            .map_or(1, |wl| wl.wrap_boundaries.len() + 1) as f32;
        let total_height = if raw_line.is_empty() {
            line_height
        } else {
            (3.0 + wrap_rows * line_height).max(line_height)
        };
        return CachedLine {
            doc_version,
            raw_hash,
            source_mode: true,
            cursor_key,
            wrap_width: col_width,
            raw_len: raw_line.len(),
            table_active: false,
            delim_expanded: false,
            ctx: dl.ctx.clone(),
            projected,
            shaped,
            table_row: None,
            line_height,
            total_height,
            text_x_rel: 0.0,
            text_y_rel: 1.5,
        };
    }

    if let LineBlockContext::Table {
        is_header,
        is_delim,
        is_first,
        is_last,
        ..
    } = &dl.ctx
    {
        if *is_delim {
            let empty_proj = ProjectedLine {
                display_text: String::new(),
                spans: Vec::new(),
                segments: Vec::new(),
                links: Vec::new(),
                prefix_visual: PrefixVisual::None,
                raw_len: raw_line.len(),
            };
            if cursor_local.is_none() {
                return CachedLine {
                    doc_version,
                    raw_hash,
                    source_mode: false,
                    cursor_key,
                    wrap_width: col_width,
                    raw_len: raw_line.len(),
                    table_active,
                    delim_expanded,
                    ctx: dl.ctx.clone(),
                    projected: empty_proj,
                    shaped: None,
                    table_row: None,
                    line_height: TABLE_LINE_HEIGHT,
                    total_height: 0.0,
                    text_x_rel: 10.0,
                    text_y_rel: 0.0,
                };
            }
            let projected = project_line(raw_line, &dl.ctx, cursor_local);
            let avail_w = (col_width - 20.0).max(100.0);
            let shaped = if projected.display_text.is_empty() {
                None
            } else {
                let runs = build_text_runs(&projected, &dl.ctx, prose_font, mono_font);
                let shared: SharedString = projected.display_text.clone().into();
                window
                    .text_system()
                    .shape_text(shared, px(13.0), &runs, Some(px(avail_w)), None)
                    .ok()
                    .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) })
            };
            let wrap_rows = shaped
                .as_ref()
                .map_or(1, |wl| wl.wrap_boundaries.len() + 1) as f32;
            let total_height = 6.0 + wrap_rows * TABLE_LINE_HEIGHT + if *is_last { 8.0 } else { 4.0 };
            return CachedLine {
                doc_version,
                raw_hash,
                source_mode: false,
                cursor_key,
                wrap_width: col_width,
                raw_len: raw_line.len(),
                table_active,
                delim_expanded,
                ctx: dl.ctx.clone(),
                projected,
                shaped,
                table_row: None,
                line_height: TABLE_LINE_HEIGHT,
                total_height,
                text_x_rel: 10.0,
                text_y_rel: 4.0,
            };
        }

        if let Some(tbl) = table_layout {
            let (proj_cells, active_col) =
                project_table_cells(raw_line, *is_header, tbl.num_cols, cursor_local);
            let mut cached_cells = Vec::with_capacity(tbl.num_cols);
            let mut max_rows = 1usize;
            let mut x_acc = 0.0_f32;
            for (cix, cell) in proj_cells.into_iter().enumerate() {
                let cw = tbl.col_widths.get(cix).copied().unwrap_or(80.0);
                let inner_w = (cw - 2.0 * TABLE_CELL_PAD_X).max(24.0);
                let align = tbl.alignments.get(cix).copied().unwrap_or(TableAlign::Left);
                let shaped = if cell.projected.display_text.is_empty() {
                    None
                } else {
                    let runs = build_text_runs(&cell.projected, &dl.ctx, prose_font, mono_font);
                    let shared: SharedString = cell.projected.display_text.clone().into();
                    window
                        .text_system()
                        .shape_text(shared, px(TABLE_FONT_SIZE), &runs, Some(px(inner_w)), None)
                        .ok()
                        .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) })
                };
                if let Some(ref wl) = shaped {
                    max_rows = max_rows.max(wl.wrap_boundaries.len() + 1);
                }
                cached_cells.push(CachedTableCell {
                    x_rel: x_acc,
                    width: cw,
                    align,
                    raw_offset: cell.raw_offset,
                    inner_range: cell.inner_range,
                    projected: cell.projected,
                    shaped,
                });
                x_acc += cw;
            }
            let frame_top_margin = if *is_first { 6.0 } else { 0.0 };
            let is_visual_last =
                *is_last || (tbl.table_end_line == tbl.delim_line && *is_header && !delim_expanded);
            let frame_bottom_margin = if is_visual_last { 8.0 } else { 0.0 };
            let cell_pad_y = 6.0;
            let row_h = cell_pad_y * 2.0 + max_rows as f32 * TABLE_LINE_HEIGHT;
            let total_height = frame_top_margin + row_h + frame_bottom_margin;
            let empty_proj = ProjectedLine {
                display_text: String::new(),
                spans: Vec::new(),
                segments: Vec::new(),
                links: Vec::new(),
                prefix_visual: PrefixVisual::None,
                raw_len: raw_line.len(),
            };
            return CachedLine {
                doc_version,
                raw_hash,
                source_mode: false,
                cursor_key,
                wrap_width: col_width,
                raw_len: raw_line.len(),
                table_active,
                delim_expanded,
                ctx: dl.ctx.clone(),
                projected: empty_proj,
                shaped: None,
                table_row: Some(CachedTableRow {
                    table_width: tbl.total_width,
                    frame_top_margin,
                    frame_bottom_margin,
                    cell_pad_y,
                    is_visual_last,
                    active_col,
                    cells: cached_cells,
                }),
                line_height: TABLE_LINE_HEIGHT,
                total_height,
                text_x_rel: TABLE_CELL_PAD_X,
                text_y_rel: frame_top_margin + cell_pad_y,
            };
        }
    }

    let projected = project_line(raw_line, &dl.ctx, cursor_local);
    let (font_size, line_height) = block_font_and_line_height(&dl.ctx);

    let indent_x = match &dl.ctx {
        LineBlockContext::TaskItem { indent_cols, .. } => {
            list_indent_px_from_cols(*indent_cols) + 28.0
        }
        LineBlockContext::ListItem { indent_cols, .. } => {
            list_indent_px_from_cols(*indent_cols) + 22.0
        }
        LineBlockContext::Callout { .. } => 14.0,
        LineBlockContext::Blockquote { depth, .. } => (*depth as f32).min(4.0) * 14.0 + 6.0,
        LineBlockContext::CodeFence { .. } | LineBlockContext::Frontmatter { .. } => 12.0,
        _ => 0.0,
    };

    let text_x_rel = match &projected.prefix_visual {
        PrefixVisual::CalloutBadge { kind } => indent_x + (kind.len() as f32 * 7.2 + 22.0),
        _ => indent_x,
    };

    let avail_w = (col_width - text_x_rel - 4.0).max(100.0);
    let shaped = if projected.display_text.is_empty() {
        None
    } else {
        let runs = build_text_runs(&projected, &dl.ctx, prose_font, mono_font);
        let shared: SharedString = projected.display_text.clone().into();
        window
            .text_system()
            .shape_text(shared, px(font_size), &runs, Some(px(avail_w)), None)
            .ok()
            .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) })
    };

    let wrap_rows = shaped
        .as_ref()
        .map_or(1, |wl| wl.wrap_boundaries.len() + 1) as f32;

    let (pad_top, pad_bot) = match &dl.ctx {
        LineBlockContext::Heading { level, .. } => {
            if *level <= 2 {
                (10.0, 5.0)
            } else {
                (7.0, 3.0)
            }
        }
        LineBlockContext::Blank => (0.0, 0.0),
        LineBlockContext::SceneBreak => (10.0, 10.0),
        LineBlockContext::CodeFence {
            is_open: true,
            lang: Some(_),
            is_last,
            ..
        } => (22.0, if *is_last { 8.0 } else { 2.0 }),
        LineBlockContext::CodeFence {
            is_first, is_last, ..
        } => (
            if *is_first { 8.0 } else { 2.0 },
            if *is_last { 8.0 } else { 2.0 },
        ),
        LineBlockContext::Frontmatter {
            is_first, is_last, ..
        } => (
            if *is_first { 6.0 } else { 1.0 },
            if *is_last { 6.0 } else { 1.0 },
        ),
        LineBlockContext::Callout {
            is_first, is_last, ..
        } => (
            if *is_first { 8.0 } else { 2.0 },
            if *is_last { 8.0 } else { 2.0 },
        ),
        _ => (2.0, 4.0),
    };

    let text_h = if matches!(dl.ctx, LineBlockContext::SceneBreak)
        && matches!(projected.prefix_visual, PrefixVisual::SceneBreakRule)
    {
        10.0
    } else if matches!(dl.ctx, LineBlockContext::Blank) {
        13.0
    } else {
        wrap_rows * line_height
    };
    let extra_chip = if let LineBlockContext::ImageEmbed { .. } = &dl.ctx {
        36.0
    } else {
        0.0
    };
    let total_height = (pad_top + text_h + pad_bot + extra_chip).max(line_height);

    CachedLine {
        doc_version,
        raw_hash,
        source_mode: false,
        cursor_key,
        wrap_width: col_width,
        raw_len: raw_line.len(),
        table_active,
        delim_expanded,
        ctx: dl.ctx.clone(),
        projected,
        shaped,
        table_row: None,
        line_height,
        total_height,
        text_x_rel,
        text_y_rel: pad_top,
    }
}

fn find_hit_link(
    links: &[crate::live_md::ProjectedLink],
    wl: &WrappedLine,
    raw_local_x: f32,
    raw_local_y: f32,
    line_height_f: f32,
    num_rows: usize,
) -> Option<HitLinkInfo> {
    if links.is_empty()
        || raw_local_x < 0.0
        || raw_local_y < 0.0
        || raw_local_y >= num_rows as f32 * line_height_f
    {
        return None;
    }
    let exact_i = wl
        .index_for_position(point(px(raw_local_x), px(raw_local_y)), px(line_height_f))
        .ok()?;
    links.iter().find_map(|link| {
        (exact_i >= link.disp_range.start && exact_i < link.disp_range.end).then(|| HitLinkInfo {
            target: link.target.clone(),
            concealed: link.concealed,
        })
    })
}

fn paint_wrapped_selection_rects(
    shaped: Option<&WrappedLine>,
    projected: &ProjectedLine,
    cell_raw_offset: usize,
    cell_raw_len: usize,
    ls: usize,
    le: usize,
    inc_nl: bool,
    text_origin_x: Pixels,
    text_origin_y: Pixels,
    line_height_f: f32,
    align: TextAlign,
    align_width: f32,
    color: Hsla,
    window: &mut Window,
) {
    let lh = px(line_height_f);
    let ds = projected.raw_to_display(ls.saturating_sub(cell_raw_offset).min(cell_raw_len));
    let de = projected.raw_to_display(le.saturating_sub(cell_raw_offset).min(cell_raw_len));
    let disp_len = projected.display_text.len();

    let Some(wl) = shaped else {
        if inc_nl {
            window.paint_quad(fill(
                Bounds {
                    origin: point(text_origin_x, text_origin_y),
                    size: size(px(8.), lh),
                },
                color,
            ));
        }
        return;
    };

    let p0 = disp_position_for_index(Some(wl), disp_len, ds, lh);
    let p1 = disp_position_for_index(Some(wl), disp_len, de, lh);
    let r0 = (f32::from(p0.y) / line_height_f).round().max(0.0) as usize;
    let r1 = (f32::from(p1.y) / line_height_f).round().max(0.0) as usize;
    let max_row = wl.wrap_boundaries.len();

    for row in r0..=r1.min(max_row) {
        let align_dx = row_align_offset_x(Some(wl), row, align, align_width);
        let row_end_x = row_right_x(Some(wl), row);
        let x0 = if row == r0 { f32::from(p0.x) } else { 0.0 };
        let mut x1 = if row == r1 {
            f32::from(p1.x)
        } else {
            row_end_x
        };
        if row == r1 && inc_nl {
            x1 = (x1 + 8.0).min(align_width);
        }
        let w = (x1 - x0).max(0.0);
        if w > 0.5 {
            window.paint_quad(fill(
                Bounds {
                    origin: point(
                        text_origin_x + px(align_dx + x0),
                        text_origin_y + px(row as f32 * line_height_f),
                    ),
                    size: size(px(w), lh),
                },
                color,
            ));
        }
    }
}

pub struct LiveMarkdownEditor {
    focus_handle: FocusHandle,
    pub doc: LiveDocBuffer,
    scroll: Rc<RefCell<LiveScrollState>>,
    scrollbar: LiveScrollbarHandle,
    height_table: LineHeightTable,
    layout_cache: HashMap<usize, Rc<CachedLine>>,
    checkbox_hit_rects: HashMap<usize, Bounds<Pixels>>,
    last_col_geo: Option<(f32, f32, f32)>,
    preferred_x: Option<f32>,
    dragging: bool,
    touch_selecting: bool,
    touch_anchor_range: Option<Range<usize>>,
    dragging_handle: Option<SelectionHandle>,
    context_menu_open: bool,
    context_menu_anchor: Option<Point<Pixels>>,
    sel_handle_positions: Rc<RefCell<Option<(Option<Point<Pixels>>, Option<Point<Pixels>>)>>>,
    context_menu_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    caret_visible: bool,
    caret_blink_epoch: u64,
    read_only: bool,
    source_mode: bool,
    interaction_blocked: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<LiveEditorEvent> for LiveMarkdownEditor {}

impl Focusable for LiveMarkdownEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl LiveMarkdownEditor {
    pub fn new(raw: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let doc = LiveDocBuffer::new(raw, false);
        let height_table = LineHeightTable::from_lines(&doc.lines, &doc.text, 360.0);
        let (scrollbar, scroll) = LiveScrollbarHandle::new();
        let focus_handle = cx.focus_handle();

        let sub_focus = cx.on_focus(&focus_handle, window, |this, _, cx| {
            this.reset_caret_blink();
            this.sync_ime_context(true);
            cx.emit(LiveEditorEvent::FocusChanged(true));
            cx.notify();
        });
        let sub_blur = cx.on_blur(&focus_handle, window, |this, _, cx| {
            this.doc.marked_range = None;
            this.caret_visible = false;
            cx.emit(LiveEditorEvent::FocusChanged(false));
            cx.notify();
        });

        let mut editor = Self {
            focus_handle,
            doc,
            scroll,
            scrollbar,
            height_table,
            layout_cache: HashMap::new(),
            checkbox_hit_rects: HashMap::new(),
            last_col_geo: None,
            preferred_x: None,
            dragging: false,
            touch_selecting: false,
            touch_anchor_range: None,
            dragging_handle: None,
            context_menu_open: false,
            context_menu_anchor: None,
            sel_handle_positions: Rc::new(RefCell::new(None)),
            context_menu_bounds: Rc::new(RefCell::new(None)),
            caret_visible: true,
            caret_blink_epoch: 0,
            read_only: false,
            source_mode: false,
            interaction_blocked: false,
            _subs: vec![sub_focus, sub_blur],
        };
        editor.start_blink_loop(window, cx);
        editor
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        if self.read_only != read_only {
            self.read_only = read_only;
            self.dragging = false;
            self.touch_selecting = false;
            self.touch_anchor_range = None;
            self.dragging_handle = None;
            self.context_menu_open = false;
            *self.context_menu_bounds.borrow_mut() = None;
            *self.sel_handle_positions.borrow_mut() = None;
            self.caret_visible = false;
            self.doc.marked_range = None;
            self.layout_cache.clear();
        }
    }

    pub fn set_interaction_blocked(&mut self, blocked: bool) {
        if self.interaction_blocked != blocked {
            self.interaction_blocked = blocked;
            if blocked {
                self.dragging = false;
                self.touch_selecting = false;
                self.touch_anchor_range = None;
                self.dragging_handle = None;
                self.context_menu_open = false;
                *self.context_menu_bounds.borrow_mut() = None;
            }
        }
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn set_source_mode(&mut self, source_mode: bool) {
        if self.source_mode != source_mode {
            self.source_mode = source_mode;
            self.layout_cache.clear();
            self.checkbox_hit_rects.clear();
            let col_w = self.last_col_geo.map_or(360.0, |g| g.1);
            self.height_table = LineHeightTable::from_lines(&self.doc.lines, &self.doc.text, col_w);
        }
    }

    pub fn is_source_mode(&self) -> bool {
        self.source_mode
    }

    pub fn is_interaction_blocked(&self) -> bool {
        self.interaction_blocked
    }

    pub fn text(&self) -> &str {
        &self.doc.text
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    fn sync_ime_context(&self, configure_mode: bool) {
        if self.read_only || self.doc.marked_range.is_some() {
            return;
        }
        let (before, after) = crate::ime_surrounding_context(&self.doc.text, self.doc.head);
        if configure_mode {
            crate::configure_android_ime(crate::AndroidImeMode::Prose, &before, &after, true);
        } else {
            crate::sync_android_ime_context(&before, &after);
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
    }

    pub fn set_text(&mut self, raw: String, cx: &mut Context<Self>) {
        self.doc.reset_text(raw, 0, 0);
        self.layout_cache.clear();
        let col_w = self.last_col_geo.map_or(360.0, |g| g.1);
        self.height_table = LineHeightTable::from_lines(&self.doc.lines, &self.doc.text, col_w);
        self.preferred_x = None;
        self.touch_selecting = false;
        self.touch_anchor_range = None;
        self.dragging_handle = None;
        self.context_menu_open = false;
        *self.context_menu_bounds.borrow_mut() = None;
        *self.sel_handle_positions.borrow_mut() = None;
        self.scroll.borrow_mut().scroll_y = 0.0;
        self.reset_caret_blink();
        cx.notify();
    }

    pub fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.doc.has_selection() {
            return false;
        }
        let s = self.doc.text[self.doc.selection_range()].to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(s));
        self.context_menu_open = false;
        cx.notify();
        true
    }

    pub fn cut_selection(&mut self, cx: &mut Context<Self>) -> bool {
        if self.read_only || !self.doc.has_selection() {
            return false;
        }
        let sel = self.doc.selection_range();
        let s = self.doc.text[sel.clone()].to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(s));
        self.doc.replace_range(sel, "", EditKind::Atomic);
        self.sync_height_table_after_edit();
        self.preferred_x = None;
        self.context_menu_open = false;
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.emit(LiveEditorEvent::Changed);
        cx.notify();
        true
    }

    pub fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) -> bool {
        if self.read_only {
            return false;
        }
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };
        let Some(txt) = item.text() else {
            return false;
        };
        self.doc.insert_text(&txt);
        self.sync_height_table_after_edit();
        self.preferred_x = None;
        self.context_menu_open = false;
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.emit(LiveEditorEvent::Changed);
        cx.notify();
        true
    }

    pub fn select_all_text(&mut self, cx: &mut Context<Self>) {
        if self.doc.text.is_empty() {
            return;
        }
        self.doc.set_selection(0, self.doc.text.len());
        self.context_menu_open = true;
        cx.notify();
    }

    pub fn toggle_highlight_selection(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        self.doc.marked_range = None;
        self.doc.toggle_wrap("==");
        self.sync_height_table_after_edit();
        self.preferred_x = None;
        self.context_menu_open = false;
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.emit(LiveEditorEvent::Changed);
        cx.notify();
    }

    fn visual_point_for_byte(
        &mut self,
        byte_off: usize,
        bounds: Bounds<Pixels>,
        col_left_rel: f32,
        top_pad: f32,
        scroll_y: f32,
    ) -> Option<(Point<Pixels>, f32)> {
        let li = self.doc.line_index_for_byte(byte_off);
        let dl = self.doc.lines.get(li)?;
        let c = self.layout_cache.get(&li)?;
        let local_raw = byte_off.saturating_sub(dl.byte_start);
        let (vx, vy, lh) = raw_offset_to_visual_point(c, local_raw);
        let x = bounds.origin.x + px(col_left_rel + vx);
        let y = bounds.origin.y + px(top_pad + self.height_table.y_for_line(li) - scroll_y + vy);
        Some((point(x, y), lh))
    }

    pub fn scroll_to_line(&mut self, line_0based: usize, window: &mut Window, cx: &mut Context<Self>) {
        let clamped = line_0based.min(self.doc.lines.len().saturating_sub(1));
        if let Some(dl) = self.doc.lines.get(clamped) {
            self.doc.set_cursor(dl.byte_start, false);
        }
        let y = self.height_table.y_for_line(clamped);
        {
            let mut st = self.scroll.borrow_mut();
            st.scroll_y = y.max(0.0);
            LiveScrollbarHandle::clamp_scroll(&mut st);
        }
        self.focus_handle.focus(window, cx);
        self.reset_caret_blink();
        self.sync_ime_context(false);
        cx.notify();
    }

    pub fn apply_format(
        &mut self,
        action: MarkdownFormatAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_handle.focus(window, cx);
        self.doc.marked_range = None;
        if self.doc.apply_mobile_format(action) {
            self.sync_height_table_after_edit();
            self.preferred_x = None;
            self.ensure_caret_visible();
            self.reset_caret_blink();
            self.sync_ime_context(false);
            cx.emit(LiveEditorEvent::Changed);
            cx.notify();
        }
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if self.doc.undo() {
            self.sync_height_table_after_edit();
            self.preferred_x = None;
            self.ensure_caret_visible();
            self.reset_caret_blink();
            cx.emit(LiveEditorEvent::Changed);
            cx.notify();
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if self.doc.redo() {
            self.sync_height_table_after_edit();
            self.preferred_x = None;
            self.ensure_caret_visible();
            self.reset_caret_blink();
            cx.emit(LiveEditorEvent::Changed);
            cx.notify();
        }
    }

    fn reset_caret_blink(&mut self) {
        self.caret_visible = true;
        self.caret_blink_epoch = self.caret_blink_epoch.wrapping_add(1);
    }

    fn start_blink_loop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            let mut last_epoch = 0u64;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(530))
                    .await;
                let cont = this
                    .update_in(cx, |ed, window, cx| {
                        if !ed.focus_handle.is_focused(window) {
                            return;
                        }
                        if ed.caret_blink_epoch != last_epoch {
                            last_epoch = ed.caret_blink_epoch;
                            ed.caret_visible = true;
                        } else {
                            ed.caret_visible = !ed.caret_visible;
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !cont {
                    break;
                }
            }
        })
        .detach();
    }

    fn sync_height_table_after_edit(&mut self) {
        let col_w = self.last_col_geo.map_or(360.0, |g| g.1);
        self.height_table
            .sync_lines(&self.doc.lines, &self.doc.text, col_w);
    }

    fn ensure_caret_visible(&mut self) {
        let li = self.doc.line_index_for_byte(self.doc.head);
        let y_top = self.height_table.y_for_line(li);
        let line_h = self
            .layout_cache
            .get(&li)
            .map_or(28.0, |c| c.total_height);
        let abs_top = MOBILE_TOP_PAD + y_top;
        let abs_bottom = abs_top + line_h;

        let mut st = self.scroll.borrow_mut();
        let vp_h = f32::from(st.viewport_bounds.size.height);
        if vp_h <= 20.0 {
            return;
        }
        if abs_top < st.scroll_y + 12.0 {
            st.scroll_y = (abs_top - 16.0).max(0.0);
        } else if abs_bottom > st.scroll_y + vp_h - 24.0 {
            st.scroll_y = (abs_bottom - vp_h + 32.0).max(0.0);
        }
        LiveScrollbarHandle::clamp_scroll(&mut st);
    }

    fn resolved_fonts(&self, window: &Window, cx: &App) -> (Font, Font) {
        let mut prose = window.text_style().font();
        prose.family = cx.theme().font_family.clone();
        let mut mono = window.text_style().font();
        mono.family = cx.theme().mono_font_family.clone();
        (prose, mono)
    }

    fn ensure_line_cached(
        &mut self,
        li: usize,
        focused: bool,
        window: &Window,
        cx: &App,
    ) -> Option<Rc<CachedLine>> {
        let dl = self.doc.lines.get(li)?.clone();
        let col_w = self.last_col_geo.map_or(360.0, |g| g.1);
        let effective_focused = focused && !self.read_only;
        let cursor_local = cursor_range_for_line(&self.doc, &dl, effective_focused);
        let key = cursor_local.as_ref().map(|r| (r.start, r.end));
        let raw_len = dl.byte_end - dl.byte_start;
        let (table_span, table_active, delim_expanded) = if self.source_mode {
            (None, false, false)
        } else {
            table_state_for_line(&self.doc, &dl.ctx, effective_focused)
        };

        let raw_hash =
            fnv1a_line_ctx_hash(&self.doc.text[dl.byte_start..dl.byte_end], &dl.ctx);

        let hit = self.layout_cache.get(&li).is_some_and(|c| {
            (table_span.is_none() || c.doc_version == self.doc.version)
                && c.raw_hash == raw_hash
                && c.source_mode == self.source_mode
                && c.ctx == dl.ctx
                && c.cursor_key == key
                && (c.wrap_width - col_w).abs() < 0.5
                && c.raw_len == raw_len
                && c.table_active == table_active
                && c.delim_expanded == delim_expanded
        });
        if !hit {
            let tbl = table_span.map(|(s, e)| compute_table_layout(&self.doc, s, e, col_w));
            let (prose, mono) = self.resolved_fonts(window, cx);
            let raw_line = &self.doc.text[dl.byte_start..dl.byte_end];
            let cached = shape_cached_line(
                &dl,
                raw_line,
                cursor_local,
                col_w,
                self.doc.version,
                self.source_mode,
                tbl.as_ref(),
                table_active,
                delim_expanded,
                &prose,
                &mono,
                window,
            );
            self.height_table.set_height(li, cached.total_height);
            self.layout_cache.insert(li, Rc::new(cached));
        }
        self.layout_cache.get(&li).cloned()
    }

    fn prepare_layout_for_bounds(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &Window,
        cx: &App,
    ) -> ((f32, f32, f32), f32, bool, usize, usize) {
        let vp_w = f32::from(bounds.size.width).max(200.0);
        let vp_h = f32::from(bounds.size.height).max(80.0);
        let geo = compute_column_geometry(vp_w);
        let focused = self.focus_handle.is_focused(window) && !self.read_only;

        if self
            .last_col_geo
            .is_none_or(|prev| (prev.1 - geo.1).abs() > 1.0)
        {
            self.height_table = LineHeightTable::from_lines(&self.doc.lines, &self.doc.text, geo.1);
            self.layout_cache.clear();
        }
        self.last_col_geo = Some(geo);

        let scroll_y = {
            let st = self.scroll.borrow();
            st.scroll_y
        };
        let total = self.doc.lines.len();
        if total == 0 {
            return (geo, scroll_y, focused, 0, 0);
        }

        let start_y = (scroll_y - geo.2 - 220.0).max(0.0);
        let end_y = (scroll_y + vp_h - geo.2 + 280.0).max(0.0);
        let first_li = self.height_table.line_for_y(start_y);
        let last_li = self.height_table.line_for_y(end_y).min(total - 1);

        for ix in first_li..=last_li {
            let _ = self.ensure_line_cached(ix, focused, window, cx);
        }

        let caret_li = self.doc.line_index_for_byte(self.doc.head);
        let _ = self.ensure_line_cached(caret_li, focused, window, cx);

        if self.layout_cache.len() > 260 {
            let keep_lo = first_li.saturating_sub(40);
            let keep_hi = last_li.saturating_add(40);
            self.layout_cache
                .retain(|&ix, _| (keep_lo..=keep_hi).contains(&ix) || ix == caret_li);
        }

        let total_doc_h = geo.2 + self.height_table.total_height() + MOBILE_BOTTOM_SCROLL_PAD;
        {
            let mut st = self.scroll.borrow_mut();
            st.viewport_bounds = bounds;
            st.total_height = total_doc_h;
            LiveScrollbarHandle::clamp_scroll(&mut st);
        }
        let clamped_scroll = self.scroll.borrow().scroll_y;
        (geo, clamped_scroll, focused, first_li, last_li)
    }

    fn hit_test_byte_at_point(&mut self, pos: Point<Pixels>) -> Option<HitTestResult> {
        let st = self.scroll.borrow();
        let bounds = st.viewport_bounds;
        if bounds.size.width <= px(1.) || self.doc.lines.is_empty() {
            return None;
        }
        let doc_y = (f32::from(pos.y - bounds.origin.y) + st.scroll_y).max(0.0);
        let (col_left_rel, col_width, top_pad) = self
            .last_col_geo
            .unwrap_or_else(|| compute_column_geometry(f32::from(bounds.size.width)));
        let rel_doc_y = (doc_y - top_pad).max(0.0);
        let scroll_y = st.scroll_y;
        drop(st);

        if rel_doc_y >= self.height_table.total_height() {
            return Some(HitTestResult {
                raw_byte: self.doc.text.len(),
                line_ix: self.doc.lines.len() - 1,
                hit_link: None,
                clicked_checkbox: false,
                phantom_table_cell: None,
            });
        }

        let mut li = self
            .height_table
            .line_for_y(rel_doc_y)
            .min(self.doc.lines.len() - 1);
        if let Some(c) = self.layout_cache.get(&li)
            && c.total_height <= 0.5
            && li > 0
        {
            li -= 1;
        }
        let dl = &self.doc.lines[li];
        let raw_line = &self.doc.text[dl.byte_start..dl.byte_end];

        let Some(cached) = self.layout_cache.get(&li) else {
            return Some(HitTestResult {
                raw_byte: dl.byte_start,
                line_ix: li,
                hit_link: None,
                clicked_checkbox: false,
                phantom_table_cell: None,
            });
        };

        // Thumb-friendly checkbox tap zone (generous padding).
        if let Some(&cb_rect) = self.checkbox_hit_rects.get(&li) {
            let expanded = Bounds {
                origin: point(cb_rect.origin.x - px(10.), cb_rect.origin.y - px(8.)),
                size: size(cb_rect.size.width + px(20.), cb_rect.size.height + px(16.)),
            };
            if expanded.contains(&pos) {
                return Some(HitTestResult {
                    raw_byte: dl.byte_start,
                    line_ix: li,
                    hit_link: None,
                    clicked_checkbox: true,
                    phantom_table_cell: None,
                });
            }
        }

        let col_left = bounds.origin.x + px(col_left_rel);
        let y_line = self.height_table.y_for_line(li);

        if let Some(tr) = &cached.table_row {
            let rel_x = f32::from(pos.x - col_left);
            let col_ix = tr
                .cells
                .iter()
                .position(|c| rel_x < c.x_rel + c.width)
                .unwrap_or_else(|| tr.cells.len().saturating_sub(1));
            let cell = &tr.cells[col_ix];
            if cell.inner_range.is_none() {
                return Some(HitTestResult {
                    raw_byte: dl.byte_end,
                    line_ix: li,
                    hit_link: None,
                    clicked_checkbox: false,
                    phantom_table_cell: Some((li, col_ix)),
                });
            }
            let slice_end = (cell.raw_offset + cell.projected.raw_len).min(raw_line.len());
            let cell_raw = &raw_line[cell.raw_offset..slice_end];
            let Some(wl) = cell.shaped.as_ref() else {
                let local_raw = cell.raw_offset + cell.projected.display_to_raw(cell_raw, 0);
                let abs_raw = clamp_char_boundary(&self.doc.text, dl.byte_start + local_raw);
                return Some(HitTestResult {
                    raw_byte: abs_raw,
                    line_ix: li,
                    hit_link: None,
                    clicked_checkbox: false,
                    phantom_table_cell: None,
                });
            };
            let cell_text_top = bounds.origin.y + px(top_pad + y_line - scroll_y)
                + px(tr.frame_top_margin + tr.cell_pad_y);
            let inner_w = (cell.width - 2.0 * TABLE_CELL_PAD_X).max(20.0);
            let num_rows = wl.wrap_boundaries.len() + 1;
            let raw_local_y = f32::from(pos.y - cell_text_top);
            let clamped_row = ((raw_local_y / cached.line_height).floor().max(0.0) as usize)
                .min(num_rows.saturating_sub(1));
            let align_dx =
                row_align_offset_x(Some(wl), clamped_row, gpui_text_align(cell.align), inner_w);
            let cell_text_left = col_left + px(cell.x_rel + TABLE_CELL_PAD_X + align_dx);
            let raw_local_x = f32::from(pos.x - cell_text_left);
            let local_x = raw_local_x.max(0.0);
            let local_y = raw_local_y.clamp(0.0, (num_rows as f32 * cached.line_height - 1.0).max(0.0));
            let hit_link = if (rel_x >= 0.0 && rel_x <= tr.table_width) && !cell.projected.links.is_empty() {
                find_hit_link(
                    &cell.projected.links,
                    wl,
                    raw_local_x,
                    raw_local_y,
                    cached.line_height,
                    num_rows,
                )
            } else {
                None
            };
            let disp_idx = match wl
                .closest_index_for_position(point(px(local_x), px(local_y)), px(cached.line_height))
            {
                Ok(i) | Err(i) => i,
            };
            let local_raw = cell.raw_offset + cell.projected.display_to_raw(cell_raw, disp_idx);
            let abs_raw = clamp_char_boundary(&self.doc.text, dl.byte_start + local_raw);
            return Some(HitTestResult {
                raw_byte: abs_raw,
                line_ix: li,
                hit_link,
                clicked_checkbox: false,
                phantom_table_cell: None,
            });
        }

        let Some(wl) = cached.shaped.as_ref() else {
            let local_raw = cached.projected.display_to_raw(raw_line, 0);
            return Some(HitTestResult {
                raw_byte: clamp_char_boundary(&self.doc.text, dl.byte_start + local_raw),
                line_ix: li,
                hit_link: None,
                clicked_checkbox: false,
                phantom_table_cell: None,
            });
        };

        let text_top = bounds.origin.y + px(top_pad + y_line - scroll_y + cached.text_y_rel);
        let text_left = col_left + px(cached.text_x_rel);
        let num_rows = wl.wrap_boundaries.len() + 1;
        let raw_local_x = f32::from(pos.x - text_left);
        let raw_local_y = f32::from(pos.y - text_top);
        let local_x = raw_local_x.clamp(0.0, col_width.max(100.0));
        let local_y = raw_local_y.clamp(0.0, (num_rows as f32 * cached.line_height - 1.0).max(0.0));

        let hit_link = if !cached.projected.links.is_empty() {
            find_hit_link(
                &cached.projected.links,
                wl,
                raw_local_x,
                raw_local_y,
                cached.line_height,
                num_rows,
            )
        } else {
            None
        };

        let disp_idx = match wl
            .closest_index_for_position(point(px(local_x), px(local_y)), px(cached.line_height))
        {
            Ok(i) | Err(i) => i,
        };
        let local_raw = cached.projected.display_to_raw(raw_line, disp_idx);
        let abs_raw = clamp_char_boundary(&self.doc.text, dl.byte_start + local_raw);
        Some(HitTestResult {
            raw_byte: abs_raw,
            line_ix: li,
            hit_link,
            clicked_checkbox: false,
            phantom_table_cell: None,
        })
    }

    fn move_vertical(&mut self, dir: isize, extend: bool) {
        let li = self.doc.line_index_for_byte(self.doc.head);
        let cur_x = if let Some(px_val) = self.preferred_x {
            px_val
        } else if let Some(c) = self.layout_cache.get(&li)
            && let Some(dl) = self.doc.lines.get(li)
        {
            let local_raw = self.doc.head.saturating_sub(dl.byte_start);
            let (vx, _, _) = raw_offset_to_visual_point(c, local_raw);
            self.preferred_x = Some(vx);
            vx
        } else {
            0.0
        };

        let target_li = if dir < 0 {
            if li == 0 {
                self.doc.set_cursor(0, extend);
                return;
            }
            li - 1
        } else {
            if li + 1 >= self.doc.lines.len() {
                self.doc.set_cursor(self.doc.text.len(), extend);
                return;
            }
            li + 1
        };

        let Some(dl_t) = self.doc.lines.get(target_li) else {
            return;
        };
        let target_byte = if let Some(c) = self.layout_cache.get(&target_li)
            && let Some(wl) = c.shaped.as_ref()
        {
            let local_x = (cur_x - c.text_x_rel).max(0.0);
            let disp = match wl
                .closest_index_for_position(point(px(local_x), px(2.)), px(c.line_height))
            {
                Ok(i) | Err(i) => i,
            };
            let raw_t = &self.doc.text[dl_t.byte_start..dl_t.byte_end];
            dl_t.byte_start + c.projected.display_to_raw(raw_t, disp)
        } else {
            dl_t.byte_start
        };
        self.doc
            .set_cursor(clamp_char_boundary(&self.doc.text, target_byte), extend);
    }

    fn handle_key_down(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let ks = &ev.keystroke;
        let cmd = ks.modifiers.control || ks.modifiers.platform;
        let shift = ks.modifiers.shift;
        let alt = ks.modifiers.alt;

        if cmd && !alt {
            match ks.key.as_str() {
                "z" => {
                    if shift {
                        self.redo(cx);
                    } else {
                        self.undo(cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                "y" => {
                    self.redo(cx);
                    cx.stop_propagation();
                    return;
                }
                "a" => {
                    self.doc.select_all();
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                "c" => {
                    if self.doc.has_selection() {
                        let s = self.doc.text[self.doc.selection_range()].to_string();
                        cx.write_to_clipboard(ClipboardItem::new_string(s));
                    }
                    cx.stop_propagation();
                    return;
                }
                "x" => {
                    if self.doc.has_selection() {
                        let sel = self.doc.selection_range();
                        let s = self.doc.text[sel.clone()].to_string();
                        cx.write_to_clipboard(ClipboardItem::new_string(s));
                        self.doc.replace_range(sel, "", EditKind::Atomic);
                        self.sync_height_table_after_edit();
                        cx.emit(LiveEditorEvent::Changed);
                        cx.notify();
                    }
                    cx.stop_propagation();
                    return;
                }
                "v" => {
                    if let Some(item) = cx.read_from_clipboard()
                        && let Some(txt) = item.text()
                    {
                        self.doc.insert_text(&txt);
                        self.sync_height_table_after_edit();
                        self.ensure_caret_visible();
                        self.reset_caret_blink();
                        cx.emit(LiveEditorEvent::Changed);
                        cx.notify();
                    }
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }

        let key = ks.key.as_str();
        let handled = match key {
            "enter" | "return" => {
                self.doc.smart_enter();
                true
            }
            "backspace" => {
                self.doc.backspace(cmd);
                true
            }
            "delete" => {
                self.doc.delete(cmd);
                true
            }
            "tab" => {
                self.doc.indent_lines(shift);
                true
            }
            "left" => {
                self.doc.move_left(cmd, shift);
                self.preferred_x = None;
                false
            }
            "right" => {
                self.doc.move_right(cmd, shift);
                self.preferred_x = None;
                false
            }
            "up" => {
                self.move_vertical(-1, shift);
                false
            }
            "down" => {
                self.move_vertical(1, shift);
                false
            }
            "home" => {
                self.doc.move_line_start(shift);
                self.preferred_x = None;
                false
            }
            "end" => {
                self.doc.move_line_end(shift);
                self.preferred_x = None;
                false
            }
            _ => {
                if !cmd && !alt {
                    if let Some(ch) = ks.key_char.as_deref()
                        && !ch.is_empty()
                        && ch.chars().all(|c| !c.is_control())
                    {
                        self.doc.insert_text(ch);
                        true
                    } else if key.chars().count() == 1
                        && key.chars().all(|c| !c.is_control())
                    {
                        self.doc.insert_text(key);
                        true
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
        };

        cx.stop_propagation();
        if handled {
            self.sync_height_table_after_edit();
            self.preferred_x = None;
            cx.emit(LiveEditorEvent::Changed);
        }
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.notify();
    }
}

impl EntityInputHandler for LiveMarkdownEditor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        if range_utf16.start == 0 && range_utf16.end == usize::MAX {
            let total_utf16 = self.doc.text.encode_utf16().count();
            *adjusted_range = Some(0..total_utf16);
            return Some(self.doc.text.clone());
        }
        let s_u8 = utf16_to_utf8_offset(&self.doc.text, range_utf16.start);
        let e_u8 = utf16_to_utf8_offset(&self.doc.text, range_utf16.end);
        let lo = s_u8.min(e_u8);
        let hi = s_u8.max(e_u8);
        *adjusted_range = Some(
            utf8_to_utf16_offset(&self.doc.text, lo)..utf8_to_utf16_offset(&self.doc.text, hi),
        );
        Some(self.doc.text[lo..hi].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let sel = self.doc.selection_range();
        Some(UTF16Selection {
            range: utf8_to_utf16_offset(&self.doc.text, sel.start)
                ..utf8_to_utf16_offset(&self.doc.text, sel.end),
            reversed: self.doc.head < self.doc.anchor,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.doc.marked_range.as_ref().map(|r| {
            utf8_to_utf16_offset(&self.doc.text, r.start)
                ..utf8_to_utf16_offset(&self.doc.text, r.end)
        })
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.doc.marked_range.take().is_some() {
            cx.notify();
        }
    }

    fn paste(&mut self, item: ClipboardItem, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = item.text() {
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        if let Some(r16) = range_utf16 {
            self.doc.marked_range = None;
            let s = utf16_to_utf8_offset(&self.doc.text, r16.start);
            let e = utf16_to_utf8_offset(&self.doc.text, r16.end);
            self.doc.replace_range(s..e, text, EditKind::Atomic);
        } else if let Some(marked) = self.doc.marked_range.take() {
            self.doc.replace_range(marked, text, EditKind::Atomic);
        } else if text == "\n" {
            self.doc.smart_enter();
            self.sync_height_table_after_edit();
            self.preferred_x = None;
            self.ensure_caret_visible();
            self.reset_caret_blink();
            self.sync_ime_context(false);
            cx.emit(LiveEditorEvent::Changed);
            cx.notify();
            return;
        } else {
            if text.is_empty() && !self.doc.has_selection() {
                return;
            }
            self.doc.insert_text(text);
        }

        self.sync_height_table_after_edit();
        self.preferred_x = None;
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.emit(LiveEditorEvent::Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let replace_u8 = if let Some(r16) = range_utf16 {
            utf16_to_utf8_offset(&self.doc.text, r16.start)
                ..utf16_to_utf8_offset(&self.doc.text, r16.end)
        } else if let Some(marked) = self.doc.marked_range.clone() {
            marked
        } else {
            self.doc.selection_range()
        };
        let insert_start = replace_u8.start.min(replace_u8.end);
        self.doc.replace_range(replace_u8, new_text, EditKind::Ime);
        let mark_end = insert_start + new_text.len();
        self.doc.marked_range = (!new_text.is_empty()).then_some(insert_start..mark_end);
        if let Some(sel16) = new_selected_range_utf16 {
            let s = utf16_to_utf8_offset(&self.doc.text[insert_start..mark_end], sel16.start);
            let e = utf16_to_utf8_offset(&self.doc.text[insert_start..mark_end], sel16.end);
            self.doc.set_selection(insert_start + s, insert_start + e);
        } else {
            self.doc.set_cursor(mark_end, false);
        }
        self.sync_height_table_after_edit();
        self.preferred_x = None;
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.emit(LiveEditorEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let pos_u8 = utf16_to_utf8_offset(&self.doc.text, range_utf16.start);
        let li = self.doc.line_index_for_byte(pos_u8);
        let st = self.scroll.borrow();
        let (col_left_rel, _, top_pad) = self
            .last_col_geo
            .unwrap_or_else(|| compute_column_geometry(f32::from(element_bounds.size.width)));
        if let (Some(dl), Some(cached)) = (self.doc.lines.get(li), self.layout_cache.get(&li)) {
            let local_raw = pos_u8.saturating_sub(dl.byte_start);
            let (vx, vy, lh) = raw_offset_to_visual_point(cached, local_raw);
            let origin_x = element_bounds.origin.x + px(col_left_rel + vx);
            let origin_y = element_bounds.origin.y
                + px(top_pad + self.height_table.y_for_line(li) - st.scroll_y + vy);
            return Some(Bounds {
                origin: point(origin_x, origin_y),
                size: size(px(2.), px(lh)),
            });
        }
        let origin_y =
            element_bounds.origin.y + px(top_pad + self.height_table.y_for_line(li) - st.scroll_y);
        Some(Bounds {
            origin: point(element_bounds.origin.x + px(col_left_rel), origin_y),
            size: size(px(2.), px(24.)),
        })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let hit = self.hit_test_byte_at_point(point)?;
        Some(utf8_to_utf16_offset(&self.doc.text, hit.raw_byte))
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let s = utf16_to_utf8_offset(&self.doc.text, range_utf16.start);
        let e = utf16_to_utf8_offset(&self.doc.text, range_utf16.end);
        self.doc.set_selection(s, e);
        cx.notify();
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.doc.text.encode_utf16().count())
    }
}

fn compute_context_menu_y(sel_top: f32, sel_bottom: f32, menu_h: f32, vp_h: f32) -> f32 {
    let above_gap = 16.0_f32;
    let below_gap = 32.0_f32;
    let max_y = (vp_h - menu_h - 8.0).max(8.0);
    if sel_top - menu_h - above_gap >= 8.0 {
        sel_top - menu_h - above_gap
    } else if sel_bottom + below_gap + menu_h <= vp_h - 8.0 {
        sel_bottom + below_gap
    } else if sel_top > (vp_h - sel_bottom) {
        8.0
    } else {
        max_y
    }
}

impl Render for LiveMarkdownEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let show_menu = self.context_menu_open
            && !self.interaction_blocked
            && !self.touch_selecting
            && self.dragging_handle.is_none();
        if !show_menu {
            *self.context_menu_bounds.borrow_mut() = None;
        }
        let floating_menu = if show_menu {
            let st = self.scroll.borrow();
            let bounds = st.viewport_bounds;
            let scroll_y = st.scroll_y;
            drop(st);
            let (col_left_rel, _, top_pad) = self
                .last_col_geo
                .unwrap_or((MOBILE_SIDE_PAD, 360.0, MOBILE_TOP_PAD));
            let vp_w = f32::from(bounds.size.width).max(320.0);
            let vp_h = f32::from(bounds.size.height).max(200.0);
            let has_sel = self.doc.has_selection();

            let (est_w, menu_h) = {
                let b = self.context_menu_bounds.borrow();
                match *b {
                    Some(rect) if f32::from(rect.size.height) > 18.0 => (
                        f32::from(rect.size.width).clamp(120.0, (vp_w - 16.0).max(120.0)),
                        f32::from(rect.size.height),
                    ),
                    _ => (
                        if has_sel {
                            (vp_w - 16.0).min(336.0)
                        } else {
                            180.0_f32.min((vp_w - 16.0).max(120.0))
                        },
                        46.0,
                    ),
                }
            };

            let mut menu_x = ((vp_w - est_w) * 0.5).clamp(8.0, (vp_w - est_w - 8.0).max(8.0));
            let mut menu_y = 24.0_f32;
            if has_sel {
                let sel = self.doc.selection_range();
                if let Some((sp, slh)) =
                    self.visual_point_for_byte(sel.start, bounds, col_left_rel, top_pad, scroll_y)
                {
                    let rel_sx = f32::from(sp.x - bounds.origin.x);
                    let rel_sy = f32::from(sp.y - bounds.origin.y);
                    let ep_opt = self.visual_point_for_byte(
                        sel.end,
                        bounds,
                        col_left_rel,
                        top_pad,
                        scroll_y,
                    );
                    let center_x = match ep_opt {
                        Some((ep, _)) if (f32::from(ep.y - sp.y)).abs() < 10.0 => {
                            (rel_sx + f32::from(ep.x - bounds.origin.x)) * 0.5
                        }
                        _ => vp_w * 0.5,
                    };
                    let sel_top = rel_sy;
                    let sel_bottom = ep_opt
                        .map(|(ep, elh)| f32::from(ep.y - bounds.origin.y) + elh)
                        .unwrap_or(rel_sy + slh);
                    menu_x = (center_x - est_w * 0.5).clamp(8.0, (vp_w - est_w - 8.0).max(8.0));
                    menu_y = compute_context_menu_y(sel_top, sel_bottom, menu_h, vp_h);
                } else if let Some(anchor) = self.context_menu_anchor {
                    let ay = f32::from(anchor.y - bounds.origin.y);
                    menu_x = (f32::from(anchor.x - bounds.origin.x) - est_w * 0.5)
                        .clamp(8.0, (vp_w - est_w - 8.0).max(8.0));
                    menu_y = compute_context_menu_y(ay, ay + 24.0, menu_h, vp_h);
                }
            } else if let Some(anchor) = self.context_menu_anchor {
                let ay = f32::from(anchor.y - bounds.origin.y);
                menu_x = (f32::from(anchor.x - bounds.origin.x) - est_w * 0.5)
                    .clamp(8.0, (vp_w - est_w - 8.0).max(8.0));
                menu_y = compute_context_menu_y(ay, ay + 24.0, menu_h, vp_h);
            }

            let menu_btn = |id: &'static str,
                            label: &'static str,
                            accent: bool,
                            bold_style: bool,
                            italic_style: bool,
                            on_click: Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>| {
                div()
                    .id(id)
                    .h(px(32.))
                    .px_2()
                    .rounded(px(7.))
                    .cursor_pointer()
                    .bg(hex_color(if accent { kd::ACCENT } else { kd::BG_PANEL }))
                    .border_1()
                    .border_color(hex_color(if accent {
                        kd::ACCENT
                    } else {
                        kd::BORDER_SUBTLE
                    }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .font_weight(if bold_style {
                        FontWeight::BOLD
                    } else {
                        FontWeight::SEMIBOLD
                    })
                    .when(italic_style, |el| el.italic())
                    .text_color(hex_color(if accent { kd::BG_MAIN } else { kd::FG }))
                    .on_click(move |ev, window, cx| on_click(ev, window, cx))
                    .child(label)
            };

            let bounds_tracker = self.context_menu_bounds.clone();
            let ed_resize = cx.entity().clone();
            let assumed_h = menu_h;
            let mut row = div()
                .id("live-ed-ctx-menu")
                .relative()
                .max_w(px((vp_w - 16.0).min(340.0)))
                .p_1p5()
                .rounded(px(12.))
                .bg(hex_color(kd::BG_DEEP))
                .border_1()
                .border_color(hex_color(kd::BORDER))
                .shadow_lg()
                .occlude()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_1()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_scroll(|_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    canvas(
                        move |b, window, _| {
                            let h_now = f32::from(b.size.height);
                            let mut slot = bounds_tracker.borrow_mut();
                            let needs_relayout = match *slot {
                                Some(prev) => (f32::from(prev.size.height) - h_now).abs() > 2.0,
                                None => (h_now - assumed_h).abs() > 2.0,
                            };
                            *slot = Some(b);
                            if needs_relayout {
                                let ed_next = ed_resize.clone();
                                window.on_next_frame(move |_, cx| {
                                    let _ = ed_next.update(cx, |_, cx| cx.notify());
                                });
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                );

            if has_sel {
                if !self.read_only {
                    row = row.child(menu_btn(
                        "ed-ctx-hl",
                        "Highlight",
                        true,
                        false,
                        false,
                        Box::new(cx.listener(|this, _, _, cx| {
                            this.toggle_highlight_selection(cx);
                        })),
                    ));
                }
                row = row.child(menu_btn(
                    "ed-ctx-copy",
                    "Copy",
                    false,
                    false,
                    false,
                    Box::new(cx.listener(|this, _, _, cx| {
                        this.copy_selection(cx);
                    })),
                ));
                if !self.read_only {
                    row = row
                        .child(menu_btn(
                            "ed-ctx-cut",
                            "Cut",
                            false,
                            false,
                            false,
                            Box::new(cx.listener(|this, _, _, cx| {
                                this.cut_selection(cx);
                            })),
                        ))
                        .child(menu_btn(
                            "ed-ctx-paste",
                            "Paste",
                            false,
                            false,
                            false,
                            Box::new(cx.listener(|this, _, _, cx| {
                                this.paste_from_clipboard(cx);
                            })),
                        ))
                        .child(menu_btn(
                            "ed-ctx-bold",
                            "B",
                            false,
                            true,
                            false,
                            Box::new(cx.listener(|this, _, window, cx| {
                                this.context_menu_open = false;
                                this.apply_format(MarkdownFormatAction::Bold, window, cx);
                            })),
                        ))
                        .child(menu_btn(
                            "ed-ctx-italic",
                            "I",
                            false,
                            false,
                            true,
                            Box::new(cx.listener(|this, _, window, cx| {
                                this.context_menu_open = false;
                                this.apply_format(MarkdownFormatAction::Italic, window, cx);
                            })),
                        ));
                }
                row = row.child(menu_btn(
                    "ed-ctx-all",
                    "All",
                    false,
                    false,
                    false,
                    Box::new(cx.listener(|this, _, _, cx| {
                        this.select_all_text(cx);
                    })),
                ));
            } else if !self.read_only {
                row = row
                    .child(menu_btn(
                        "ed-ctx-paste",
                        "Paste",
                        false,
                        false,
                        false,
                        Box::new(cx.listener(|this, _, _, cx| {
                            this.paste_from_clipboard(cx);
                        })),
                    ))
                    .child(menu_btn(
                        "ed-ctx-all",
                        "Select All",
                        false,
                        false,
                        false,
                        Box::new(cx.listener(|this, _, _, cx| {
                            this.select_all_text(cx);
                        })),
                    ));
            }

            Some(
                div()
                    .absolute()
                    .top(px(menu_y))
                    .left(px(menu_x))
                    .child(
                        row.with_animation(
                            "live-ed-ctx-pop",
                            Animation::new(Duration::from_millis(145)).with_easing(ease_out_quint()),
                            |el, t| el.mt(px(8.0 * (1.0 - t))).opacity(t),
                        ),
                    ),
            )
        } else {
            None
        };

        div()
            .id(SharedString::from(format!("live-md-editor-{:?}", cx.entity_id())))
            .relative()
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                this.handle_key_down(ev, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    if this.interaction_blocked {
                        return;
                    }
                    if this
                        .context_menu_bounds
                        .borrow()
                        .is_some_and(|b| b.contains(&ev.position))
                    {
                        return;
                    }
                    this.context_menu_open = false;
                    cx.stop_propagation();
                    this.doc.break_undo_coalescing();

                    let Some(hit) = this.hit_test_byte_at_point(ev.position) else {
                        if this.doc.has_selection() {
                            let h = this.doc.head;
                            this.doc.set_cursor(h, false);
                            cx.notify();
                        }
                        if !this.read_only {
                            this.focus_handle.focus(window, cx);
                            cx.notify();
                        }
                        return;
                    };

                    if !this.read_only
                        && let Some((row_li, col_ix)) = hit.phantom_table_cell
                    {
                        this.doc.table_ensure_cell(row_li, col_ix);
                        this.sync_height_table_after_edit();
                        this.focus_handle.focus(window, cx);
                        this.reset_caret_blink();
                        this.sync_ime_context(false);
                        cx.emit(LiveEditorEvent::Changed);
                        cx.notify();
                        return;
                    }

                    // Toggle task checkbox directly without forcing soft keyboard open.
                    if hit.clicked_checkbox {
                        if this.doc.toggle_task_on_line(hit.line_ix) {
                            this.sync_height_table_after_edit();
                            cx.emit(LiveEditorEvent::Changed);
                            cx.notify();
                        }
                        return;
                    }

                    // Tapping a concealed link jumps cleanly without popping keyboard.
                    if ev.click_count == 1
                        && let Some(hl) = hit.hit_link
                        && hl.concealed
                    {
                        window.blur(cx);
                        cx.emit(LiveEditorEvent::OpenLink(hl.target));
                        return;
                    }

                    if this.read_only {
                        if this.doc.has_selection() {
                            let h = this.doc.head;
                            this.doc.set_cursor(h, false);
                        }
                        cx.notify();
                        return;
                    }

                    this.focus_handle.focus(window, cx);
                    this.preferred_x = None;
                    match ev.click_count {
                        2 => this.doc.select_word_at(hit.raw_byte),
                        n if n >= 3 => this.doc.select_line_at(hit.raw_byte),
                        _ => {
                            this.doc.set_cursor(hit.raw_byte, ev.modifiers.shift);
                            this.dragging = true;
                        }
                    }
                    this.ensure_caret_visible();
                    this.reset_caret_blink();
                    this.sync_ime_context(false);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _window, cx| {
                if this.interaction_blocked || this.read_only || !this.dragging {
                    return;
                }
                if !ev.dragging() {
                    this.dragging = false;
                    return;
                }
                if let Some(hit) = this.hit_test_byte_at_point(ev.position)
                    && hit.raw_byte != this.doc.head
                {
                    this.doc.set_cursor(hit.raw_byte, true);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _window, _cx| {
                    this.dragging = false;
                }),
            )
            .on_scroll(cx.listener(|this, ev: &ScrollEvent, _window, cx| {
                if this.interaction_blocked {
                    return;
                }
                let dy = f32::from(ev.delta.pixel_delta(px(24.)).y);
                if dy.abs() > 0.01 {
                    let mut st = this.scroll.borrow_mut();
                    let old_y = st.scroll_y;
                    st.scroll_y -= dy;
                    LiveScrollbarHandle::clamp_scroll(&mut st);
                    let moved = (st.scroll_y - old_y).abs() > 0.01;
                    drop(st);
                    cx.stop_propagation();
                    if moved {
                        cx.notify();
                    }
                }
            }))
            .child(LiveCanvasElement {
                editor: cx.entity(),
            })
            .child(Scrollbar::vertical(&self.scrollbar))
            .when_some(floating_menu, |root, m| root.child(m))
    }
}

struct LiveCanvasElement {
    editor: Entity<LiveMarkdownEditor>,
}

struct PaintRow {
    dl: DocLine,
    cached: Rc<CachedLine>,
    y_top: f32,
    checkbox_rect: Option<Bounds<Pixels>>,
}

struct PaintSnapshot {
    bounds: Bounds<Pixels>,
    focus_handle: FocusHandle,
    col_left_rel: f32,
    col_width: f32,
    top_pad: f32,
    scroll_y: f32,
    paint_rows: Vec<PaintRow>,
    caret_cached: Option<Rc<CachedLine>>,
    caret_y_top: f32,
    caret_disp_x_y: Option<(f32, f32, f32)>,
    selection_range: Range<usize>,
    start_handle: Option<(Point<Pixels>, f32)>,
    end_handle: Option<(Point<Pixels>, f32)>,
    caret_visible: bool,
    focused: bool,
    read_only: bool,
    source_mode: bool,
    marked_range: Option<Range<usize>>,
    mono_font: Font,
    prose_bold_font: Font,
}

impl IntoElement for LiveCanvasElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for LiveCanvasElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<PaintSnapshot>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (snap, height_adjusted) = self.editor.update(cx, |ed, cx| {
            let height_before = ed.height_table.total_height();
            let (geo, scroll_y, focused, first_li, last_li) =
                ed.prepare_layout_for_bounds(bounds, window, cx);
            let col_left = bounds.origin.x + px(geo.0);
            let mut paint_rows = Vec::with_capacity(last_li.saturating_sub(first_li) + 1);
            ed.checkbox_hit_rects.clear();

            for ix in first_li..=last_li {
                let Some(dl) = ed.doc.lines.get(ix).cloned() else {
                    continue;
                };
                let y_line = ed.height_table.y_for_line(ix);
                let Some(c_rc) = ed.layout_cache.get(&ix).cloned() else {
                    continue;
                };

                let checkbox_rect = if !ed.source_mode
                    && let PrefixVisual::Checkbox { indent_cols, .. } =
                        &c_rc.projected.prefix_visual
                {
                    let cb_x = col_left + px(list_indent_px_from_cols(*indent_cols) + 2.0);
                    let cb_y = bounds.origin.y + px(geo.2 + y_line - scroll_y)
                        + px(c_rc.text_y_rel + (c_rc.line_height - 16.0) * 0.5);
                    let rect = Bounds {
                        origin: point(cb_x, cb_y),
                        size: size(px(16.), px(16.)),
                    };
                    ed.checkbox_hit_rects.insert(ix, rect);
                    Some(rect)
                } else {
                    None
                };

                paint_rows.push(PaintRow {
                    dl,
                    cached: c_rc,
                    y_top: y_line,
                    checkbox_rect,
                });
            }

            let caret_li = ed.doc.line_index_for_byte(ed.doc.head);
            let caret_dl = ed.doc.lines.get(caret_li).cloned();
            let caret_y_top = ed.height_table.y_for_line(caret_li);
            let caret_cached = ed.layout_cache.get(&caret_li).cloned();

            let caret_disp_x_y = if let (Some(dl), Some(c)) = (caret_dl.as_ref(), caret_cached.as_ref()) {
                let local_raw = ed.doc.head.saturating_sub(dl.byte_start);
                Some(raw_offset_to_visual_point(c, local_raw))
            } else {
                None
            };

            let selection_range = ed.doc.selection_range();
            let (start_handle, end_handle) = if !selection_range.is_empty() && !ed.interaction_blocked
            {
                let mut vis = |byte_off: usize| -> Option<(Point<Pixels>, f32)> {
                    let (pt, lh) =
                        ed.visual_point_for_byte(byte_off, bounds, geo.0, geo.2, scroll_y)?;
                    (pt.y + px(lh) >= bounds.top() && pt.y <= bounds.bottom()).then_some((pt, lh))
                };
                let sh = vis(selection_range.start);
                let eh = vis(selection_range.end);
                let sk = sh.map(|(p, lh)| point(p.x, p.y + px(lh) + px(8.0)));
                let ek = eh.map(|(p, lh)| point(p.x, p.y + px(lh) + px(8.0)));
                *ed.sel_handle_positions.borrow_mut() = Some((sk, ek));
                (sh, eh)
            } else {
                *ed.sel_handle_positions.borrow_mut() = None;
                (None, None)
            };

            let (prose_base, mono_font) = ed.resolved_fonts(window, cx);
            let mut prose_bold_font = prose_base;
            prose_bold_font.weight = FontWeight::BOLD;

            let height_adjusted = (ed.height_table.total_height() - height_before).abs() > 0.5;
            (
                PaintSnapshot {
                    bounds,
                    focus_handle: ed.focus_handle.clone(),
                    col_left_rel: geo.0,
                    col_width: geo.1,
                    top_pad: geo.2,
                    scroll_y,
                    paint_rows,
                    caret_cached,
                    caret_y_top,
                    caret_disp_x_y,
                    selection_range,
                    start_handle,
                    end_handle,
                    caret_visible: ed.caret_visible,
                    focused,
                    read_only: ed.read_only,
                    source_mode: ed.source_mode,
                    marked_range: ed.doc.marked_range.clone(),
                    mono_font,
                    prose_bold_font,
                },
                height_adjusted,
            )
        });

        if height_adjusted {
            let ed_next = self.editor.clone();
            window.on_next_frame(move |_, cx| {
                let _ = ed_next.update(cx, |_, cx| cx.notify());
            });
        }
        Some(snap)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(snap) = prepaint.take() else {
            return;
        };

        if !snap.read_only {
            window.handle_input(
                &snap.focus_handle,
                ElementInputHandler::new(bounds, self.editor.clone()),
                cx,
            );
        }

        let sel_bg = cx.theme().colors.selection;
        let col_left = snap.bounds.origin.x + px(snap.col_left_rel);
        let col_w = px(snap.col_width);

        window.with_content_mask(Some(ContentMask { bounds: snap.bounds }), |window| {
            for row in &snap.paint_rows {
                let dl = &row.dl;
                let c = &row.cached;
                if c.total_height <= 0.5 {
                    continue;
                }
                let top_y = snap.bounds.origin.y + px(snap.top_pad + row.y_top - snap.scroll_y);
                let row_h = px(c.total_height);

                if let Some(tr) = &c.table_row {
                    let (is_header, is_first, body_row_ix) = match &c.ctx {
                        LineBlockContext::Table {
                            is_header,
                            is_first,
                            body_row_index,
                            ..
                        } => (*is_header, *is_first, *body_row_index),
                        _ => (false, false, 0),
                    };
                    let row_top = top_y + px(tr.frame_top_margin);
                    let frame_h =
                        (row_h - px(tr.frame_top_margin + tr.frame_bottom_margin)).max(px(1.));
                    let row_w = px(tr.table_width);
                    let bg_col = if is_header {
                        hex_color(kd::BG_PANEL)
                    } else if body_row_ix % 2 == 0 {
                        hex_color(kd::BG_MAIN)
                    } else {
                        hex_color(kd::BG_SIDEBAR)
                    };
                    let r_top = if is_first { px(6.) } else { px(0.) };
                    let r_bot = if tr.is_visual_last { px(6.) } else { px(0.) };
                    let card_h = if tr.is_visual_last {
                        frame_h
                    } else {
                        frame_h + px(0.5)
                    };
                    window.paint_quad(quad(
                        Bounds {
                            origin: point(col_left, row_top),
                            size: size(row_w, card_h),
                        },
                        gpui::Corners {
                            top_left: r_top,
                            top_right: r_top,
                            bottom_right: r_bot,
                            bottom_left: r_bot,
                        },
                        bg_col,
                        gpui::Edges {
                            top: if is_first { px(1.) } else { px(0.) },
                            right: px(1.),
                            bottom: if tr.is_visual_last { px(1.) } else { px(0.) },
                            left: px(1.),
                        },
                        hex_color(kd::BORDER),
                        BorderStyle::Solid,
                    ));

                    let div_top = row_top + if is_first { px(1.) } else { px(0.) };
                    let div_h = (frame_h
                        - if is_first { px(1.) } else { px(0.) }
                        - if tr.is_visual_last { px(1.) } else { px(0.) })
                    .max(px(1.));
                    for (cix, cell) in tr.cells.iter().enumerate() {
                        if cix >= 1 {
                            window.paint_quad(fill(
                                Bounds {
                                    origin: point(col_left + px(cell.x_rel), div_top),
                                    size: size(px(1.), div_h),
                                },
                                hex_color(kd::BORDER_SUBTLE),
                            ));
                        }
                        if snap.focused && tr.active_col == Some(cix) {
                            window.paint_quad(quad(
                                Bounds {
                                    origin: point(col_left + px(cell.x_rel + 0.5), row_top),
                                    size: size(px((cell.width - 1.0).max(2.0)), frame_h),
                                },
                                px(2.),
                                accent_alpha(0x14),
                                gpui::Edges::all(px(1.5)),
                                hex_color(kd::ACCENT),
                                BorderStyle::Solid,
                            ));
                        }
                    }

                    let sel = &snap.selection_range;
                    let raw_len = dl.byte_end - dl.byte_start;
                    let row_has_sel =
                        sel.start < sel.end && sel.start <= dl.byte_end && sel.end >= dl.byte_start;
                    let ls = sel.start.saturating_sub(dl.byte_start).min(raw_len);
                    let le = sel.end.saturating_sub(dl.byte_start).min(raw_len);
                    let inc_nl = sel.end > dl.byte_end;

                    for cell in &tr.cells {
                        let cell_clip = Bounds {
                            origin: point(col_left + px(cell.x_rel + 1.0), row_top + px(1.0)),
                            size: size(
                                px((cell.width - 2.0).max(1.0)),
                                (frame_h - px(2.0)).max(px(1.)),
                            ),
                        };
                        let cell_inner_w = (cell.width - 2.0 * TABLE_CELL_PAD_X).max(20.0);
                        let cell_text_x = col_left + px(cell.x_rel + TABLE_CELL_PAD_X);
                        let cell_text_y = row_top + px(tr.cell_pad_y);
                        let cell_align = gpui_text_align(cell.align);

                        window.with_content_mask(
                            Some(ContentMask { bounds: cell_clip }),
                            |window| {
                                if row_has_sel && let Some(ref inner) = cell.inner_range {
                                    let cell_raw_end = cell.raw_offset + cell.projected.raw_len;
                                    if ls <= cell_raw_end && le >= cell.raw_offset {
                                        let cross = le > inner.end && (inc_nl || ls <= inner.start);
                                        paint_wrapped_selection_rects(
                                            cell.shaped.as_ref(),
                                            &cell.projected,
                                            cell.raw_offset,
                                            cell.projected.raw_len,
                                            ls,
                                            le,
                                            cross,
                                            cell_text_x,
                                            cell_text_y,
                                            c.line_height,
                                            cell_align,
                                            cell_inner_w,
                                            sel_bg,
                                            window,
                                        );
                                    }
                                }
                                if let Some(wl) = cell.shaped.as_ref() {
                                    let align_bounds = Bounds {
                                        origin: point(cell_text_x, cell_text_y),
                                        size: size(px(cell_inner_w), frame_h),
                                    };
                                    let lh = px(c.line_height);
                                    let _ = wl.paint_background(
                                        point(cell_text_x, cell_text_y),
                                        lh,
                                        cell_align,
                                        Some(align_bounds),
                                        window,
                                        cx,
                                    );
                                    let _ = wl.paint(
                                        point(cell_text_x, cell_text_y),
                                        lh,
                                        cell_align,
                                        Some(align_bounds),
                                        window,
                                        cx,
                                    );
                                }
                            },
                        );
                    }
                    continue;
                }

                // Block decorations & prefix visuals (suppressed in raw Source Mode)
                if !snap.source_mode {
                    match &c.ctx {
                    LineBlockContext::CodeFence {
                        is_first, is_last, ..
                    } => {
                        let r_top = if *is_first { px(6.) } else { px(0.) };
                        let r_bot = if *is_last { px(6.) } else { px(0.) };
                        window.paint_quad(quad(
                            Bounds {
                                origin: point(col_left, top_y),
                                size: size(col_w, row_h + px(0.5)),
                            },
                            gpui::Corners {
                                top_left: r_top,
                                top_right: r_top,
                                bottom_right: r_bot,
                                bottom_left: r_bot,
                            },
                            hex_color(kd::BG_DEEP),
                            gpui::Edges::default(),
                            gpui::transparent_black(),
                            BorderStyle::Solid,
                        ));
                    }
                    LineBlockContext::Frontmatter { .. } => {
                        window.paint_quad(fill(
                            Bounds {
                                origin: point(col_left, top_y),
                                size: size(col_w, row_h + px(0.5)),
                            },
                            hex_color(kd::BG_SIDEBAR),
                        ));
                    }
                    LineBlockContext::Callout {
                        is_first, is_last, ..
                    } => {
                        let r_top = if *is_first { px(6.) } else { px(0.) };
                        let r_bot = if *is_last { px(6.) } else { px(0.) };
                        window.paint_quad(quad(
                            Bounds {
                                origin: point(col_left, top_y),
                                size: size(col_w, row_h + px(0.5)),
                            },
                            gpui::Corners {
                                top_left: r_top,
                                top_right: r_top,
                                bottom_right: r_bot,
                                bottom_left: r_bot,
                            },
                            accent_alpha(0x18),
                            gpui::Edges::default(),
                            gpui::transparent_black(),
                            BorderStyle::Solid,
                        ));
                        window.paint_quad(fill(
                            Bounds {
                                origin: point(col_left, top_y),
                                size: size(px(3.), row_h + px(0.5)),
                            },
                            hex_color(kd::ACCENT),
                        ));
                    }
                    LineBlockContext::Blockquote { depth, .. } => {
                        for d in 0..(*depth).min(4) {
                            window.paint_quad(fill(
                                Bounds {
                                    origin: point(col_left + px(d as f32 * 14.0 + 2.0), top_y + px(1.)),
                                    size: size(px(3.), row_h),
                                },
                                hex_color(kd::ACCENT),
                            ));
                        }
                    }
                    LineBlockContext::ImageEmbed { target, .. } => {
                        let pill_y = top_y + row_h - px(32.);
                        window.paint_quad(quad(
                            Bounds {
                                origin: point(col_left, pill_y),
                                size: size(col_w, px(26.)),
                            },
                            px(6.),
                            hex_color(kd::BG_SIDEBAR),
                            gpui::Edges::all(px(1.)),
                            hex_color(kd::BORDER_SUBTLE),
                            BorderStyle::Solid,
                        ));
                        let lbl: SharedString = format!("Image: {target}").into();
                        let run = TextRun {
                            len: lbl.len(),
                            font: snap.mono_font.clone(),
                            color: hex_color(kd::FG_SUBTLE),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shaped = window
                            .text_system()
                            .shape_line(lbl, px(12.), &[run], None);
                        let _ = shaped.paint(
                            point(col_left + px(10.), pill_y + px(5.)),
                            px(16.),
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                    _ => {}
                }

                // Prefix visuals (rules, bullets, checkboxes, language pill, callout pill)
                match &c.projected.prefix_visual {
                    PrefixVisual::SceneBreakRule => {
                        let rule_y = top_y + row_h * 0.5;
                        window.paint_quad(fill(
                            Bounds {
                                origin: point(col_left + px(8.), rule_y),
                                size: size(col_w - px(16.), px(1.)),
                            },
                            hex_color(kd::BORDER),
                        ));
                    }
                    PrefixVisual::Bullet { indent_cols } => {
                        let dot_x = col_left + px(list_indent_px_from_cols(*indent_cols) + 7.0);
                        let dot_y = top_y + px(c.text_y_rel + c.line_height * 0.5 - 2.5);
                        window.paint_quad(quad(
                            Bounds {
                                origin: point(dot_x, dot_y),
                                size: size(px(5.5), px(5.5)),
                            },
                            px(3.),
                            hex_color(kd::ORANGE),
                            gpui::Edges::default(),
                            gpui::transparent_black(),
                            BorderStyle::Solid,
                        ));
                    }
                    PrefixVisual::Checkbox { checked, .. } => {
                        if let Some(cb_rect) = row.checkbox_rect {
                            let bg = if *checked {
                                hex_color(kd::ACCENT)
                            } else {
                                hex_color(kd::BG_DEEP)
                            };
                            let border_c = if *checked {
                                hex_color(kd::ACCENT_HOVER)
                            } else {
                                hex_color(kd::FG_MUTED)
                            };
                            window.paint_quad(quad(
                                cb_rect,
                                px(4.),
                                bg,
                                gpui::Edges::all(px(1.3)),
                                border_c,
                                BorderStyle::Solid,
                            ));
                            if *checked {
                                window.paint_quad(fill(
                                    Bounds {
                                        origin: point(
                                            cb_rect.origin.x + px(4.),
                                            cb_rect.origin.y + px(4.),
                                        ),
                                        size: size(px(8.), px(8.)),
                                    },
                                    hex_color(kd::BG_MAIN),
                                ));
                            }
                        }
                    }
                    PrefixVisual::CodeFenceBar {
                        lang: Some(lang_str),
                        is_open: true,
                    } => {
                        let chip_w = (lang_str.len() as f32 * 6.8 + 12.0).max(28.0);
                        let chip_origin = point(col_left + px(12.), top_y + px(4.));
                        window.paint_quad(quad(
                            Bounds {
                                origin: chip_origin,
                                size: size(px(chip_w), px(15.)),
                            },
                            px(4.),
                            hex_color(kd::BG_PANEL),
                            gpui::Edges::default(),
                            gpui::transparent_black(),
                            BorderStyle::Solid,
                        ));
                        let lbl: SharedString = lang_str.clone().into();
                        let run = TextRun {
                            len: lbl.len(),
                            font: snap.mono_font.clone(),
                            color: hex_color(kd::FG_MUTED),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shaped = window
                            .text_system()
                            .shape_line(lbl, px(10.5), &[run], None);
                        let _ = shaped.paint(
                            point(chip_origin.x + px(6.), chip_origin.y + px(1.)),
                            px(13.),
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                    PrefixVisual::CalloutBadge { kind } => {
                        let pill_w = (kind.len() as f32 * 7.2 + 14.0).max(46.0);
                        let pill_origin = point(
                            col_left + px(14.),
                            top_y + px(c.text_y_rel + (c.line_height - 18.0) * 0.5),
                        );
                        window.paint_quad(quad(
                            Bounds {
                                origin: pill_origin,
                                size: size(px(pill_w), px(18.)),
                            },
                            px(4.),
                            accent_alpha(0x2e),
                            gpui::Edges::default(),
                            gpui::transparent_black(),
                            BorderStyle::Solid,
                        ));
                        let upper: SharedString = kind.to_ascii_uppercase().into();
                        let run = TextRun {
                            len: upper.len(),
                            font: snap.prose_bold_font.clone(),
                            color: hex_color(kd::ACCENT_HOVER),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shaped = window
                            .text_system()
                            .shape_line(upper, px(10.5), &[run], None);
                        let _ = shaped.paint(
                            point(pill_origin.x + px(6.), pill_origin.y + px(1.5)),
                            px(15.),
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                    _ => {}
                }
                }

                // Selection highlight rects
                let sel = &snap.selection_range;
                if sel.start < sel.end && sel.start <= dl.byte_end && sel.end >= dl.byte_start {
                    let raw_len = dl.byte_end - dl.byte_start;
                    let ls = sel.start.saturating_sub(dl.byte_start).min(raw_len);
                    let le = sel.end.saturating_sub(dl.byte_start).min(raw_len);
                    let inc_nl = sel.end > dl.byte_end;
                    paint_wrapped_selection_rects(
                        c.shaped.as_ref(),
                        &c.projected,
                        0,
                        raw_len,
                        ls,
                        le,
                        inc_nl,
                        col_left + px(c.text_x_rel),
                        top_y + px(c.text_y_rel),
                        c.line_height,
                        TextAlign::Left,
                        snap.col_width,
                        sel_bg,
                        window,
                    );
                }

                // Paint shaped wrapped line
                let text_origin = point(col_left + px(c.text_x_rel), top_y + px(c.text_y_rel));
                if let Some(wl) = c.shaped.as_ref() {
                    let lh = px(c.line_height);
                    let _ = wl.paint_background(text_origin, lh, TextAlign::Left, None, window, cx);
                    let _ = wl.paint(text_origin, lh, TextAlign::Left, None, window, cx);
                }

                // IME composing underline
                if !snap.read_only
                    && let Some(mr) = &snap.marked_range
                    && mr.start < mr.end
                    && mr.start <= dl.byte_end
                    && mr.end >= dl.byte_start
                {
                    let raw_len = dl.byte_end - dl.byte_start;
                    let ds = c
                        .projected
                        .raw_to_display(mr.start.saturating_sub(dl.byte_start).min(raw_len));
                    let de = c
                        .projected
                        .raw_to_display(mr.end.saturating_sub(dl.byte_start).min(raw_len));
                    let disp_len = c.projected.display_text.len();
                    let lh = px(c.line_height);
                    let p0 = disp_position_for_index(c.shaped.as_ref(), disp_len, ds, lh);
                    let p1 = disp_position_for_index(c.shaped.as_ref(), disp_len, de, lh);
                    if (f32::from(p1.x) - f32::from(p0.x)) > 1.0 && p0.y == p1.y {
                        let uy = top_y + px(c.text_y_rel) + p0.y + lh - px(2.);
                        window.paint_quad(fill(
                            Bounds {
                                origin: point(col_left + px(c.text_x_rel) + p0.x, uy),
                                size: size(p1.x - p0.x, px(1.5)),
                            },
                            hex_color(kd::YELLOW),
                        ));
                    }
                }
            }

            // Blinking Caret
            if !snap.read_only
                && snap.focused
                && snap.caret_visible
                && let Some((vx, vy, lh)) = snap.caret_disp_x_y
                && snap.caret_cached.is_some()
            {
                let caret_x = col_left + px(vx);
                let caret_y = snap.bounds.origin.y
                    + px(snap.top_pad + snap.caret_y_top - snap.scroll_y + vy + 1.0);
                window.paint_quad(fill(
                    Bounds {
                        origin: point(caret_x, caret_y),
                        size: size(px(2.), px((lh - 2.0).max(14.0))),
                    },
                    hex_color(kd::ACCENT),
                ));
            }

            // Mobile selection teardrop handles
            let paint_handle = |win: &mut Window, pt: Point<Pixels>, lh: f32| {
                win.paint_quad(fill(
                    Bounds {
                        origin: pt,
                        size: size(px(2.), px(lh)),
                    },
                    hex_color(kd::ACCENT_HOVER),
                ));
                win.paint_quad(quad(
                    Bounds {
                        origin: point(pt.x - px(8.), pt.y + px(lh)),
                        size: size(px(16.), px(16.)),
                    },
                    px(8.),
                    hex_color(kd::ACCENT),
                    gpui::Edges::all(px(1.5)),
                    hex_color(kd::FG),
                    BorderStyle::Solid,
                ));
            };
            if let Some((sp, slh)) = snap.start_handle {
                paint_handle(window, sp, slh);
            }
            if let Some((ep, elh)) = snap.end_handle {
                paint_handle(window, ep, elh);
            }
        });

        let ed_lp = self.editor.clone();
        let cap_ent = self.editor.clone();
        let lp_bounds = snap.bounds;
        window.on_mouse_event(move |ev: &LongPressEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let _ = ed_lp.update(cx, |ed, cx| {
                if ed.interaction_blocked {
                    return;
                }
                let in_menu = ed
                    .context_menu_bounds
                    .borrow()
                    .is_some_and(|b| b.contains(&ev.position));
                if in_menu {
                    return;
                }
                match ev.phase {
                    TouchPhase::Started => {
                        if window.default_prevented() || !lp_bounds.contains(&ev.start_position) {
                            return;
                        }
                        window.capture_long_press(&cap_ent);
                        window.prevent_default();
                        cx.stop_propagation();
                        if let Some(hit) = ed.hit_test_byte_at_point(ev.start_position) {
                            ed.doc.select_word_at(hit.raw_byte);
                            if ed.doc.has_selection() {
                                let r = ed.doc.selection_range();
                                ed.touch_selecting = true;
                                ed.touch_anchor_range = Some(r);
                                ed.context_menu_open = true;
                                ed.context_menu_anchor = Some(ev.start_position);
                            } else if !ed.read_only {
                                ed.doc.set_cursor(hit.raw_byte, false);
                                ed.touch_selecting = false;
                                ed.touch_anchor_range = None;
                                ed.context_menu_open = true;
                                ed.context_menu_anchor = Some(ev.start_position);
                            }
                            cx.notify();
                        }
                    }
                    TouchPhase::Moved => {
                        if !ed.touch_selecting {
                            return;
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                        if let Some(hit) = ed.hit_test_byte_at_point(ev.position)
                            && let Some(anchor) = &ed.touch_anchor_range
                        {
                            let (a, b) = if hit.raw_byte < anchor.start {
                                (anchor.end, hit.raw_byte)
                            } else if hit.raw_byte > anchor.end {
                                (anchor.start, hit.raw_byte)
                            } else {
                                (anchor.start, anchor.end)
                            };
                            ed.doc.set_selection(a, b);
                            ed.context_menu_anchor = Some(ev.position);
                            cx.notify();
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        if ed.touch_selecting {
                            ed.touch_selecting = false;
                            ed.touch_anchor_range = None;
                            cx.notify();
                        }
                    }
                }
            });
        });

        let ed_td = self.editor.clone();
        window.on_mouse_event(move |ev: &TouchDragEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let _ = ed_td.update(cx, |ed, cx| {
                if ed.interaction_blocked || !ed.doc.has_selection() {
                    return;
                }
                let in_menu = ed
                    .context_menu_bounds
                    .borrow()
                    .is_some_and(|b| b.contains(&ev.position));
                if in_menu && ed.dragging_handle.is_none() {
                    return;
                }
                match ev.phase {
                    TouchPhase::Started => {
                        if window.default_prevented() {
                            return;
                        }
                        let positions = *ed.sel_handle_positions.borrow();
                        let Some((s_pt, e_pt)) = positions else {
                            return;
                        };
                        let r_sq = 30.0_f32 * 30.0;
                        let near = |pt_opt: Option<Point<Pixels>>| -> Option<f32> {
                            let p = pt_opt?;
                            let dx = f32::from(ev.start_position.x - p.x);
                            let dy = f32::from(ev.start_position.y - p.y);
                            let d2 = dx * dx + dy * dy;
                            (d2 <= r_sq).then_some(d2)
                        };
                        let chosen = match (near(s_pt), near(e_pt)) {
                            (Some(a), Some(b)) => Some(if a <= b {
                                SelectionHandle::Start
                            } else {
                                SelectionHandle::End
                            }),
                            (Some(_), None) => Some(SelectionHandle::Start),
                            (None, Some(_)) => Some(SelectionHandle::End),
                            (None, None) => None,
                        };
                        if let Some(h) = chosen {
                            ed.dragging_handle = Some(h);
                            window.prevent_default();
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }
                    TouchPhase::Moved => {
                        let Some(h) = ed.dragging_handle else {
                            return;
                        };
                        window.prevent_default();
                        cx.stop_propagation();
                        let test_pos = point(ev.position.x, ev.position.y - px(12.));
                        if let Some(hit) = ed.hit_test_byte_at_point(test_pos) {
                            let cur = ed.doc.selection_range();
                            match h {
                                SelectionHandle::Start => {
                                    if hit.raw_byte < cur.end {
                                        ed.doc.set_selection(cur.end, hit.raw_byte);
                                    } else if hit.raw_byte > cur.end {
                                        ed.doc.set_selection(cur.end, hit.raw_byte);
                                        ed.dragging_handle = Some(SelectionHandle::End);
                                    }
                                }
                                SelectionHandle::End => {
                                    if hit.raw_byte > cur.start {
                                        ed.doc.set_selection(cur.start, hit.raw_byte);
                                    } else if hit.raw_byte < cur.start {
                                        ed.doc.set_selection(cur.start, hit.raw_byte);
                                        ed.dragging_handle = Some(SelectionHandle::Start);
                                    }
                                }
                            }
                            ed.context_menu_open = true;
                            cx.notify();
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        if ed.dragging_handle.take().is_some() {
                            ed.context_menu_open = ed.doc.has_selection();
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::compute_context_menu_y;

    #[test]
    fn context_menu_never_overlaps_selection_or_handles() {
        // Mid-screen selection: strictly above sel_top with >= 16px clearance
        for menu_h in [46.0_f32, 86.0_f32] {
            let sel_top = 260.0;
            let sel_bottom = 284.0;
            let y = compute_context_menu_y(sel_top, sel_bottom, menu_h, 720.0);
            assert!(
                y + menu_h <= sel_top - 16.0 + 1e-3,
                "expected menu bottom ({}) <= {} for menu_h={menu_h}",
                y + menu_h,
                sel_top - 16.0
            );
        }

        // Top-of-screen selection: placed strictly below selection + handles (>= 32px clearance)
        let sel_top = 20.0;
        let sel_bottom = 44.0;
        let y_below = compute_context_menu_y(sel_top, sel_bottom, 46.0, 720.0);
        assert!(y_below >= sel_bottom + 32.0 - 1e-3);

        // Near-full-screen selection: clamped inside viewport [8.0, vp_h - menu_h - 8.0]
        let y_full = compute_context_menu_y(10.0, 705.0, 46.0, 720.0);
        assert!(y_full >= 8.0 && y_full <= 720.0 - 46.0 - 8.0);
    }
}
