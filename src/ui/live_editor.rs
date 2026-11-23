use crate::live_md::{
    DocLine, EditKind, InlineStyle, LineBlockContext, LineHeightTable, LinkTarget, LiveDocBuffer,
    PrefixVisual, ProjectedLine, TableAlign, clamp_char_boundary, list_indent_px_from_cols,
    parse_table_alignments, project_line, project_table_cells, table_cell_at_local_byte,
    utf8_to_utf16_offset, utf16_to_utf8_offset,
};
use crate::markdown::{
    ImageAlign, StyledImageSpec, compute_image_display_size, parse_image_spec,
    read_image_dimensions,
};
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use crate::vault::VaultIndex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarHandle};
use gpui_kit::{
    AnyElement, App, BorderStyle, Bounds, ClipboardEntry, ClipboardItem, ContentMask, Context,
    Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, Font, FontStyle, FontWeight, GlobalElementId, HitboxBehavior,
    InspectorElementId, InteractiveElement as _, IntoElement, KeyDownEvent, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement, Pixels, Point, Render,
    ScrollEvent, SharedString, Size, StrikethroughStyle, Style, Styled, StyledImage as _,
    TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, actions, div, fill,
    img, point, prelude::FluentBuilder as _, px, quad, relative, size,
};

pub const LIVE_EDITOR_CONTEXT: &str = "VKarmaLiveEditor";

actions!(vkarma_live_editor, [LiveEditorTab, LiveEditorShiftTab]);
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

pub enum LiveEditorEvent {
    Changed,
    OpenLink {
        target: LinkTarget,
        in_right_split: bool,
    },
    PasteImage(ClipboardItem),
}

struct LiveScrollState {
    scroll_y: f32,
    total_height: f32,
    viewport_bounds: Bounds<Pixels>,
    drag_total_height: Option<f32>,
}

#[derive(Clone)]
pub struct LiveEditorScrollbarHandle {
    state: Rc<RefCell<LiveScrollState>>,
}

impl LiveEditorScrollbarHandle {
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

impl ScrollbarHandle for LiveEditorScrollbarHandle {
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

const TABLE_CELL_PAD_X: f32 = 12.0;
const TABLE_FONT_SIZE: f32 = 14.0;
const TABLE_LINE_HEIGHT: f32 = 21.0;
const TABLE_TOOLBAR_RESERVE: f32 = 30.0;

#[derive(Clone)]
struct TableBlockLayout {
    col_widths: Vec<f32>,
    col_xs: Vec<f32>,
    alignments: Vec<TableAlign>,
    num_cols: usize,
    total_width: f32,
}

struct CachedTableCell {
    inner_range: Option<Range<usize>>,
    raw_offset: usize,
    projected: ProjectedLine,
    shaped: Option<WrappedLine>,
    x_rel: f32,
    width: f32,
    align: TableAlign,
}

struct CachedTableRow {
    cells: Vec<CachedTableCell>,
    active_col: Option<usize>,
    table_width: f32,
    cell_pad_y: f32,
    frame_top_margin: f32,
    frame_bottom_margin: f32,
    is_visual_last: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeHandleCorner {
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
    RightEdge,
    LeftEdge,
}

#[derive(Debug, Clone)]
struct ActiveImageResize {
    line_ix: usize,
    corner: ResizeHandleCorner,
    start_mouse: Point<Pixels>,
    start_width: f32,
    aspect_ratio: f32,
    preview_width: f32,
    preview_height: f32,
    align: ImageAlign,
    spec: StyledImageSpec,
    max_width: f32,
}

#[derive(Debug, Clone, Copy)]
struct FloatExclusion {
    y_end: f32,
    side: ImageAlign,
    width: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct LineFloatLayout {
    left_inset: f32,
    right_inset: f32,
    clear_top_spacer: f32,
}

struct CachedLine {
    doc_version: u64,
    cursor_key: Option<(usize, usize)>,
    wrap_width: f32,
    raw_len: usize,
    table_active: bool,
    delim_expanded: bool,
    extra_left_inset: f32,
    extra_right_inset: f32,
    clear_top_spacer: f32,
    img_dims_key: Option<(u32, u32)>,
    ctx: LineBlockContext,
    projected: ProjectedLine,
    shaped: Option<WrappedLine>,
    table_row: Option<CachedTableRow>,
    line_height: f32,
    total_height: f32,
    y_top: f32,
    text_x_rel: f32,
    text_y_rel: f32,
    checkbox_rect: Option<Bounds<Pixels>>,
}

impl CachedLine {
    fn num_wrap_rows(&self) -> usize {
        self.shaped
            .as_ref()
            .map_or(1, |wl| wl.wrap_boundaries.len() + 1)
    }
}

fn gpui_text_align(align: TableAlign) -> TextAlign {
    match align {
        TableAlign::Left => TextAlign::Left,
        TableAlign::Center => TextAlign::Center,
        TableAlign::Right => TextAlign::Right,
    }
}

fn compute_table_layout(
    doc: &LiveDocBuffer,
    start_li: usize,
    end_li: usize,
    col_width: f32,
) -> TableBlockLayout {
    let clamped_end = end_li.min(doc.lines.len().saturating_sub(1));
    let delim_li = start_li + 1;
    let alignments = doc
        .lines
        .get(delim_li)
        .map(|dl| parse_table_alignments(&doc.text[dl.byte_start..dl.byte_end]))
        .unwrap_or_default();

    let mut num_cols = alignments.len().max(1);
    let mut max_chars: Vec<usize> = vec![3; num_cols];
    let scan_end = clamped_end.min(start_li + 500);

    for li in start_li..=scan_end {
        if li == delim_li {
            continue;
        }
        let Some(dl) = doc.lines.get(li) else {
            continue;
        };
        let raw = &doc.text[dl.byte_start..dl.byte_end];
        let (cells, _) = project_table_cells(raw, li == start_li, num_cols, None);
        if cells.len() > num_cols {
            num_cols = cells.len();
            max_chars.resize(num_cols, 3);
        }
        let header_bonus = if li == start_li { 2 } else { 0 };
        for (cix, cell) in cells.iter().enumerate() {
            let ch_count = cell.projected.display_text.chars().count() + header_bonus;
            max_chars[cix] = max_chars[cix].max(ch_count).max(3);
        }
    }

    let mut col_alignments = alignments;
    col_alignments.resize(num_cols, TableAlign::Left);

    let min_w = (col_width / num_cols as f32).min(64.0).max(24.0);
    let pref_w: Vec<f32> = max_chars
        .iter()
        .map(|&ch| (ch as f32 * 8.4 + 2.0 * TABLE_CELL_PAD_X).max(min_w))
        .collect();
    let sum_pref: f32 = pref_w.iter().sum();

    let mut col_widths = vec![min_w; num_cols];
    if sum_pref <= col_width && sum_pref > 0.0 {
        let slack = col_width - sum_pref;
        for c in 0..num_cols {
            col_widths[c] = pref_w[c] + slack * (pref_w[c] / sum_pref);
        }
    } else {
        let remain = (col_width - min_w * num_cols as f32).max(0.0);
        let weights: Vec<f32> = pref_w
            .iter()
            .map(|&pw| (pw - min_w).max(1.0).powf(0.75))
            .collect();
        let sum_w: f32 = weights.iter().sum();
        if sum_w > 0.0 {
            for c in 0..num_cols {
                col_widths[c] = min_w + remain * (weights[c] / sum_w);
            }
        }
    }

    let mut col_xs = Vec::with_capacity(num_cols);
    let mut cur_x = 0.0f32;
    for &w in &col_widths {
        col_xs.push(cur_x);
        cur_x += w;
    }
    if let (Some(last_w), Some(last_x)) = (col_widths.last_mut(), col_xs.last()) {
        *last_w = (col_width - *last_x).max(20.0);
    }

    TableBlockLayout {
        col_widths,
        col_xs,
        alignments: col_alignments,
        num_cols,
        total_width: col_width,
    }
}

fn table_state_for_line(
    doc: &LiveDocBuffer,
    ctx: &LineBlockContext,
) -> (Option<(usize, usize)>, bool, bool) {
    if let LineBlockContext::Table {
        table_start_line,
        table_end_line,
        ..
    } = ctx
    {
        let delim_li = table_start_line + 1;
        let delim_expanded = doc
            .lines
            .get(delim_li)
            .is_some_and(|ddl| cursor_range_for_line(doc, ddl, true).is_some());
        let sel = doc.selection_range();
        let table_active = doc
            .lines
            .get(*table_start_line)
            .zip(doc.lines.get(*table_end_line))
            .is_some_and(|(s_dl, e_dl)| sel.end >= s_dl.byte_start && sel.start <= e_dl.byte_end);
        (
            Some((*table_start_line, *table_end_line)),
            table_active,
            delim_expanded,
        )
    } else {
        (None, false, false)
    }
}

struct ColumnGeometry {
    col_left_rel: f32,
    col_width: f32,
    top_pad: f32,
}

fn compute_column_geometry(vp_width: f32, centered: bool, zen_mode: bool) -> ColumnGeometry {
    let col_max = super::column_max_width(centered, zen_mode);
    let horiz_pad = if col_max.is_some() { 32.0 } else { 20.0 };
    let avail = (vp_width - 2.0 * horiz_pad).max(160.0);
    let col_width = col_max.map_or(avail, |w| avail.min(w));
    let col_left_rel = if col_max.is_some() {
        ((vp_width - col_width) * 0.5).max(horiz_pad)
    } else {
        horiz_pad
    };
    let top_pad = if zen_mode { 56.0 } else { 20.0 };
    ColumnGeometry {
        col_left_rel,
        col_width,
        top_pad,
    }
}

fn line_typography(ctx: &LineBlockContext) -> (f32, f32, f32, f32) {
    match ctx {
        LineBlockContext::Heading { level: 1, .. } => (27.0, 35.0, 14.0, 6.0),
        LineBlockContext::Heading { level: 2, .. } => (20.0, 27.0, 11.0, 4.0),
        LineBlockContext::Heading { level: 3, .. } => (17.0, 23.0, 9.0, 3.0),
        LineBlockContext::Heading { .. } => (15.5, 22.0, 8.0, 2.0),
        LineBlockContext::CodeFence {
            is_first, is_last, ..
        } => {
            let pt = if *is_first { 8.0 } else { 2.0 };
            let pb = if *is_last { 8.0 } else { 2.0 };
            (13.5, 20.5, pt, pb)
        }
        LineBlockContext::Frontmatter {
            is_first, is_last, ..
        } => {
            let pt = if *is_first { 8.0 } else { 2.0 };
            let pb = if *is_last { 8.0 } else { 2.0 };
            (13.0, 20.0, pt, pb)
        }
        LineBlockContext::Callout {
            is_first, is_last, ..
        } => {
            let pt = if *is_first { 8.0 } else { 2.5 };
            let pb = if *is_last { 8.0 } else { 2.5 };
            (15.0, 24.0, pt, pb)
        }
        LineBlockContext::Table { .. } | LineBlockContext::MdxBlock { .. } => {
            (13.5, 21.0, 2.0, 2.0)
        }
        LineBlockContext::ImageEmbed { .. } => (13.0, 20.0, 2.0, 2.0),
        LineBlockContext::SceneBreak => (15.0, 24.0, 0.0, 0.0),
        LineBlockContext::Blank => (15.0, 24.75, 0.0, 0.0),
        _ => (15.0, 24.75, 2.0, 2.0),
    }
}

fn callout_badge_width(kind: &str) -> f32 {
    (kind.len() as f32 * 6.8 + 14.0).max(40.0)
}

fn line_text_indent_x(ctx: &LineBlockContext, prefix_visual: &PrefixVisual) -> f32 {
    match ctx {
        LineBlockContext::TaskItem { indent_cols, .. } => {
            let base = list_indent_px_from_cols(*indent_cols);
            if matches!(prefix_visual, PrefixVisual::Checkbox { .. }) {
                base + 24.0
            } else {
                0.0
            }
        }
        LineBlockContext::ListItem {
            indent_cols,
            ordered,
            ..
        } => {
            let base = list_indent_px_from_cols(*indent_cols);
            if !*ordered && matches!(prefix_visual, PrefixVisual::Bullet { .. }) {
                base + 18.0
            } else if *ordered {
                base
            } else {
                0.0
            }
        }
        LineBlockContext::Blockquote { depth, .. } => (*depth as f32).min(4.0) * 14.0,
        LineBlockContext::Callout { .. } => match prefix_visual {
            PrefixVisual::CalloutBadge { kind } => 16.0 + callout_badge_width(kind) + 8.0,
            _ => 16.0,
        },
        LineBlockContext::CodeFence { .. } | LineBlockContext::Frontmatter { .. } => 14.0,
        _ => 0.0,
    }
}

fn resolve_image_file(target: &str, doc_path: &Path, vault: &VaultIndex) -> Option<PathBuf> {
    if target.starts_with("http://") || target.starts_with("https://") {
        return None;
    }
    if let Some(found) = vault.resolve_asset(target, doc_path) {
        return Some(found);
    }
    let rel = doc_path.parent()?.join(target);
    rel.is_file().then_some(rel)
}

fn build_text_runs(
    proj: &ProjectedLine,
    ctx: &LineBlockContext,
    prose_font: &Font,
    mono_font: &Font,
) -> Vec<TextRun> {
    proj.spans
        .iter()
        .map(|sp| {
            let st: InlineStyle = sp.style;
            let mut font = if st.monospace
                || matches!(
                    ctx,
                    LineBlockContext::CodeFence { .. }
                        | LineBlockContext::Frontmatter { .. }
                        | LineBlockContext::Table { is_delim: true, .. }
                )
            {
                mono_font.clone()
            } else {
                prose_font.clone()
            };
            if st.bold || st.heading_level > 0 {
                font.weight = FontWeight::SEMIBOLD;
                if st.bold && st.heading_level == 0 {
                    font.weight = FontWeight::BOLD;
                }
            }
            if st.italic {
                font.style = FontStyle::Italic;
            }

            let color = if st.syntax_muted {
                hex_color(kd::FG_MUTED)
            } else if st.code {
                hex_color(kd::ORANGE)
            } else if st.wikilink {
                hex_color(kd::AQUA)
            } else if st.link {
                hex_color(kd::ACCENT)
            } else if st.callout_title {
                hex_color(kd::ACCENT)
            } else if st.bullet_marker {
                hex_color(kd::ORANGE)
            } else if st.checked_task || st.strikethrough {
                hex_color(kd::FG_MUTED)
            } else if st.heading_level == 1 {
                hex_color(kd::ACCENT)
            } else if st.heading_level == 2 {
                hex_color(kd::AQUA)
            } else if st.heading_level == 3 {
                hex_color(kd::GREEN)
            } else if st.heading_level >= 4 {
                hex_color(kd::VIOLET)
            } else if st.dim {
                hex_color(kd::FG_SUBTLE)
            } else {
                hex_color(kd::FG)
            };

            let background_color = if st.highlight {
                Some(hex_alpha(0xc4b28a44))
            } else if st.code && !st.syntax_muted {
                Some(hex_alpha(0x28262be0))
            } else {
                None
            };

            let underline = if st.link && !st.syntax_muted {
                Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(color),
                    wavy: false,
                })
            } else {
                None
            };

            let strikethrough = if st.strikethrough && !st.syntax_muted {
                Some(StrikethroughStyle {
                    thickness: px(1.2),
                    color: Some(hex_color(kd::FG_MUTED)),
                })
            } else {
                None
            };

            TextRun {
                len: sp.range.len(),
                font,
                color,
                background_color,
                underline,
                strikethrough,
            }
        })
        .collect()
}

fn cursor_range_for_line(
    doc: &LiveDocBuffer,
    dl: &DocLine,
    caret_only_if_collapsed: bool,
) -> Option<Range<usize>> {
    let sel = doc.selection_range();
    let (s, e) = if caret_only_if_collapsed && sel.start == sel.end {
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

fn row_right_x(shaped: Option<&WrappedLine>, row: usize, line_height: Pixels) -> f32 {
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
    let _ = line_height;
    (row_end_x - row_start_x).max(0.0)
}

fn row_align_offset_x(
    shaped: Option<&WrappedLine>,
    row: usize,
    align: TextAlign,
    align_width: f32,
    line_height: Pixels,
) -> f32 {
    match align {
        TextAlign::Left => 0.0,
        TextAlign::Center => {
            ((align_width - row_right_x(shaped, row, line_height)) * 0.5).max(0.0)
        }
        TextAlign::Right => (align_width - row_right_x(shaped, row, line_height)).max(0.0),
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
            px(c.line_height),
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

#[allow(clippy::too_many_arguments)]
fn shape_cached_line(
    dl: &DocLine,
    raw_line: &str,
    cursor_local: Option<Range<usize>>,
    col_width: f32,
    doc_version: u64,
    table_layout: Option<&TableBlockLayout>,
    table_active: bool,
    delim_expanded: bool,
    float_layout: LineFloatLayout,
    resolved_img: Option<(f32, f32, bool)>,
    prose_font: &Font,
    mono_font: &Font,
    window: &Window,
) -> CachedLine {
    let cursor_key = cursor_local.as_ref().map(|r| (r.start, r.end));

    if let LineBlockContext::Table {
        is_header,
        is_delim,
        is_first,
        is_last,
        table_start_line,
        table_end_line,
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
                    cursor_key,
                    wrap_width: col_width,
                    raw_len: raw_line.len(),
                    table_active,
                    delim_expanded,
                    extra_left_inset: 0.0,
                    extra_right_inset: 0.0,
                    clear_top_spacer: 0.0,
                    img_dims_key: None,
                    ctx: dl.ctx.clone(),
                    projected: empty_proj,
                    shaped: None,
                    table_row: None,
                    line_height: TABLE_LINE_HEIGHT,
                    total_height: 0.0,
                    y_top: 0.0,
                    text_x_rel: 12.0,
                    text_y_rel: 0.0,
                    checkbox_rect: None,
                };
            }
            let projected = project_line(raw_line, &dl.ctx, cursor_local);
            let avail_w = (col_width - 24.0).max(120.0);
            let shaped = if projected.display_text.is_empty() {
                None
            } else {
                let runs = build_text_runs(&projected, &dl.ctx, prose_font, mono_font);
                let shared: SharedString = projected.display_text.clone().into();
                window
                    .text_system()
                    .shape_text(shared, px(13.5), &runs, Some(px(avail_w)), None)
                    .ok()
                    .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) })
            };
            let wrap_rows = shaped
                .as_ref()
                .map_or(1, |wl| wl.wrap_boundaries.len() + 1) as f32;
            let frame_bot = if *is_last {
                if table_active {
                    TABLE_TOOLBAR_RESERVE
                } else {
                    8.0
                }
            } else {
                0.0
            };
            let pad_bot = if *is_last { 4.0 + frame_bot } else { 4.0 };
            let total_height = 4.0 + wrap_rows * TABLE_LINE_HEIGHT + pad_bot;
            return CachedLine {
                doc_version,
                cursor_key,
                wrap_width: col_width,
                raw_len: raw_line.len(),
                table_active,
                delim_expanded,
                extra_left_inset: 0.0,
                extra_right_inset: 0.0,
                clear_top_spacer: 0.0,
                img_dims_key: None,
                ctx: dl.ctx.clone(),
                projected,
                shaped,
                table_row: None,
                line_height: TABLE_LINE_HEIGHT,
                total_height,
                y_top: 0.0,
                text_x_rel: 12.0,
                text_y_rel: 4.0,
                checkbox_rect: None,
            };
        }

        if let Some(tbl) = table_layout {
            let (proj_cells, active_col) =
                project_table_cells(raw_line, *is_header, tbl.num_cols, cursor_local);
            let mut cached_cells = Vec::with_capacity(tbl.num_cols);
            let mut max_wrap_rows = 1usize;

            for (cix, pcell) in proj_cells.into_iter().enumerate() {
                let x_rel = tbl.col_xs.get(cix).copied().unwrap_or(0.0);
                let width = tbl.col_widths.get(cix).copied().unwrap_or(64.0);
                let align = tbl.alignments.get(cix).copied().unwrap_or(TableAlign::Left);
                let avail_cell_w = (width - 2.0 * TABLE_CELL_PAD_X).max(20.0);
                let shaped = if pcell.projected.display_text.is_empty() {
                    None
                } else {
                    let runs = build_text_runs(&pcell.projected, &dl.ctx, prose_font, mono_font);
                    let shared: SharedString = pcell.projected.display_text.clone().into();
                    window
                        .text_system()
                        .shape_text(
                            shared,
                            px(TABLE_FONT_SIZE),
                            &runs,
                            Some(px(avail_cell_w)),
                            None,
                        )
                        .ok()
                        .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) })
                };
                let rows = shaped
                    .as_ref()
                    .map_or(1, |wl| wl.wrap_boundaries.len() + 1);
                max_wrap_rows = max_wrap_rows.max(rows);
                cached_cells.push(CachedTableCell {
                    inner_range: pcell.inner_range,
                    raw_offset: pcell.raw_offset,
                    projected: pcell.projected,
                    shaped,
                    x_rel,
                    width,
                    align,
                });
            }

            let is_visual_last =
                *is_last || (*is_header && *table_end_line == *table_start_line + 1 && !delim_expanded);
            let clear_top = if *is_first {
                float_layout.clear_top_spacer
            } else {
                0.0
            };
            let frame_top_margin = (if *is_first { 6.0 } else { 0.0 }) + clear_top;
            let frame_bottom_margin = if is_visual_last {
                if table_active {
                    TABLE_TOOLBAR_RESERVE
                } else {
                    8.0
                }
            } else {
                0.0
            };
            let cell_pad_y = if *is_header { 8.0 } else { 6.5 };
            let total_height = frame_top_margin
                + cell_pad_y * 2.0
                + max_wrap_rows as f32 * TABLE_LINE_HEIGHT
                + frame_bottom_margin;

            return CachedLine {
                doc_version,
                cursor_key,
                wrap_width: col_width,
                raw_len: raw_line.len(),
                table_active,
                delim_expanded,
                extra_left_inset: 0.0,
                extra_right_inset: 0.0,
                clear_top_spacer: clear_top,
                img_dims_key: None,
                ctx: dl.ctx.clone(),
                projected: ProjectedLine {
                    display_text: String::new(),
                    spans: Vec::new(),
                    segments: Vec::new(),
                    links: Vec::new(),
                    prefix_visual: PrefixVisual::None,
                    raw_len: raw_line.len(),
                },
                shaped: None,
                table_row: Some(CachedTableRow {
                    cells: cached_cells,
                    active_col,
                    table_width: tbl.total_width,
                    cell_pad_y,
                    frame_top_margin,
                    frame_bottom_margin,
                    is_visual_last,
                }),
                line_height: TABLE_LINE_HEIGHT,
                total_height,
                y_top: 0.0,
                text_x_rel: TABLE_CELL_PAD_X,
                text_y_rel: frame_top_margin + cell_pad_y,
                checkbox_rect: None,
            };
        }
    }

    let projected = project_line(raw_line, &dl.ctx, cursor_local);
    let (font_size_f, lh_f, base_pt, base_pb) = line_typography(&dl.ctx);
    let indent_x = float_layout.left_inset + line_text_indent_x(&dl.ctx, &projected.prefix_visual);
    let right_pad = if matches!(
        dl.ctx,
        LineBlockContext::CodeFence { .. }
            | LineBlockContext::Frontmatter { .. }
            | LineBlockContext::Callout { .. }
    ) {
        14.0
    } else {
        4.0
    };
    let avail_text_w = (col_width - indent_x - float_layout.right_inset - right_pad).max(120.0);

    let shaped = if projected.display_text.is_empty() {
        None
    } else {
        let runs = build_text_runs(&projected, &dl.ctx, prose_font, mono_font);
        let shared: SharedString = projected.display_text.clone().into();
        window
            .text_system()
            .shape_text(shared, px(font_size_f), &runs, Some(px(avail_text_w)), None)
            .ok()
            .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) })
    };

    let wrap_rows = shaped
        .as_ref()
        .map_or(1, |wl| wl.wrap_boundaries.len() + 1) as f32;

    let (total_height, img_dims_key) = match &dl.ctx {
        LineBlockContext::ImageEmbed { alt, .. } => {
            let (img_w, img_h, is_floated) = resolved_img.unwrap_or_else(|| {
                let s = parse_image_spec(alt);
                let max_w = if s.is_floated() {
                    (col_width - 140.0).max(150.0)
                } else {
                    (col_width - 16.0).max(120.0)
                };
                let (w, h) = compute_image_display_size(&s, None, max_w);
                (w, h, s.is_floated())
            });
            let text_h = if projected.display_text.is_empty() {
                0.0
            } else {
                wrap_rows * lh_f + 4.0
            };
            let total = if is_floated {
                float_layout.clear_top_spacer + text_h + 4.0
            } else {
                float_layout.clear_top_spacer + text_h + img_h + 14.0
            };
            (total, Some((img_w.round() as u32, img_h.round() as u32)))
        }
        LineBlockContext::Blank | LineBlockContext::SceneBreak => {
            (float_layout.clear_top_spacer + lh_f, None)
        }
        _ => (
            float_layout.clear_top_spacer + base_pt + wrap_rows * lh_f + base_pb,
            None,
        ),
    };

    CachedLine {
        doc_version,
        cursor_key,
        wrap_width: col_width,
        raw_len: raw_line.len(),
        table_active,
        delim_expanded,
        extra_left_inset: float_layout.left_inset,
        extra_right_inset: float_layout.right_inset,
        clear_top_spacer: float_layout.clear_top_spacer,
        img_dims_key,
        ctx: dl.ctx.clone(),
        projected,
        shaped,
        table_row: None,
        line_height: lh_f,
        total_height,
        y_top: 0.0,
        text_x_rel: indent_x,
        text_y_rel: float_layout.clear_top_spacer + base_pt,
        checkbox_rect: None,
    }
}

struct HitTestResult {
    raw_byte: usize,
    line_ix: usize,
    clicked_link: Option<LinkTarget>,
    clicked_checkbox: bool,
    phantom_table_cell: Option<(usize, usize)>,
}

/// Obsidian-style unified native GPUI Live Markdown editor.
pub struct LiveMarkdownEditor {
    focus_handle: FocusHandle,
    pub doc: LiveDocBuffer,
    doc_path: PathBuf,
    vault: Arc<VaultIndex>,
    centered: bool,
    zen_mode: bool,
    scroll: Rc<RefCell<LiveScrollState>>,
    scrollbar: LiveEditorScrollbarHandle,
    height_table: LineHeightTable,
    layout_cache: HashMap<usize, Rc<CachedLine>>,
    image_dims_cache: HashMap<PathBuf, Option<(f32, f32)>>,
    selected_image_line: Option<usize>,
    active_image_resize: Option<ActiveImageResize>,
    max_float_y_end: f32,
    last_col_geo: Option<(f32, f32, f32)>,
    preferred_x: Option<f32>,
    dragging: bool,
    caret_visible: bool,
    caret_blink_epoch: u64,
}

impl EventEmitter<LiveEditorEvent> for LiveMarkdownEditor {}

impl Focusable for LiveMarkdownEditor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl LiveMarkdownEditor {
    pub fn new(
        raw: String,
        doc_path: PathBuf,
        vault: Arc<VaultIndex>,
        is_mdx: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let doc = LiveDocBuffer::new(raw, is_mdx);
        let height_table = LineHeightTable::from_lines(&doc.lines, &doc.text, 760.0);
        let (scrollbar, scroll) = LiveEditorScrollbarHandle::new();
        let mut editor = Self {
            focus_handle: cx.focus_handle(),
            doc,
            doc_path,
            vault,
            centered: true,
            zen_mode: false,
            scroll,
            scrollbar,
            height_table,
            layout_cache: HashMap::new(),
            image_dims_cache: HashMap::new(),
            selected_image_line: None,
            active_image_resize: None,
            max_float_y_end: 0.0,
            last_col_geo: None,
            preferred_x: None,
            dragging: false,
            caret_visible: true,
            caret_blink_epoch: 0,
        };
        editor.start_blink_loop(window, cx);
        editor
    }

    pub fn text(&self) -> &str {
        &self.doc.text
    }

    pub fn cursor_line_col(&self) -> (usize, usize) {
        self.doc.cursor_line_col()
    }

    pub fn set_doc_path(&mut self, path: PathBuf) {
        self.doc_path = path;
    }

    /// Updates centering/Zen/vault context from the parent view during render without notifying.
    pub fn sync_render_context(&mut self, centered: bool, zen_mode: bool, vault: Arc<VaultIndex>) {
        self.centered = centered;
        self.zen_mode = zen_mode;
        self.vault = vault;
    }

    pub fn set_text(
        &mut self,
        raw: String,
        line_0based: usize,
        col_chars: usize,
        cx: &mut Context<Self>,
    ) {
        self.doc.reset_text(raw, line_0based, col_chars);
        self.layout_cache.clear();
        if self.image_dims_cache.len() > 128 {
            self.image_dims_cache.clear();
        }
        self.selected_image_line = None;
        self.active_image_resize = None;
        self.max_float_y_end = 0.0;
        let col_w = self.last_col_geo.map_or(760.0, |g| g.1);
        self.height_table = LineHeightTable::from_lines(&self.doc.lines, &self.doc.text, col_w);
        self.preferred_x = None;
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.notify();
    }

    pub fn insert_text_at_caret(&mut self, s: &str, cx: &mut Context<Self>) {
        self.selected_image_line = None;
        self.doc.insert_text(s);
        self.sync_height_table_after_edit();
        self.preferred_x = None;
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.emit(LiveEditorEvent::Changed);
        cx.notify();
    }

    pub fn jump_to_line(
        &mut self,
        line_0based: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_image_line = None;
        let b = self.doc.byte_from_line_col(line_0based, 0);
        self.doc.set_cursor(b, false);
        self.preferred_x = None;
        let li = line_0based.min(self.doc.lines.len().saturating_sub(1));
        let target_y = self.height_table.y_for_line(li);
        {
            let mut st = self.scroll.borrow_mut();
            let top_pad = if self.zen_mode { 56.0 } else { 20.0 };
            st.scroll_y = (top_pad + target_y - 40.0).max(0.0);
            LiveEditorScrollbarHandle::clamp_scroll(&mut st);
        }
        self.focus_handle.focus(window, cx);
        self.reset_caret_blink();
        cx.notify();
    }

    fn validate_selected_image(&mut self) {
        if let Some(ix) = self.selected_image_line {
            let valid = self
                .doc
                .lines
                .get(ix)
                .is_some_and(|dl| matches!(dl.ctx, LineBlockContext::ImageEmbed { .. }));
            if !valid {
                self.selected_image_line = None;
            }
        }
    }

    fn sync_height_table_after_edit(&mut self) {
        let col_w = self.last_col_geo.map_or(760.0, |g| g.1);
        self.height_table
            .sync_lines(&self.doc.lines, &self.doc.text, col_w);
        self.validate_selected_image();
    }

    fn natural_image_dims(&mut self, target: &str) -> Option<(f32, f32)> {
        let path = resolve_image_file(target, &self.doc_path, &self.vault)?;
        if let Some(&cached) = self.image_dims_cache.get(&path) {
            return cached;
        }
        let dims = read_image_dimensions(&path);
        self.image_dims_cache.insert(path, dims);
        dims
    }

    fn resolve_image_box(
        &mut self,
        ix: usize,
        target: &str,
        alt: &str,
        col_width: f32,
    ) -> (f32, f32, StyledImageSpec) {
        if let Some(active) = &self.active_image_resize
            && active.line_ix == ix
        {
            return (
                active.preview_width,
                active.preview_height,
                active.spec.clone(),
            );
        }
        let spec = parse_image_spec(alt);
        let max_w = if spec.is_floated() {
            (col_width - 140.0).max(150.0)
        } else {
            (col_width - 16.0).max(120.0)
        };
        let nat = self.natural_image_dims(target);
        let (w, h) = compute_image_display_size(&spec, nat, max_w);
        (w, h, spec)
    }

    fn update_image_spec_action(
        &mut self,
        line_ix: usize,
        spec: StyledImageSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.doc.update_image_embed_spec(line_ix, &spec) {
            self.selected_image_line = Some(line_ix);
            self.layout_cache.clear();
            self.sync_height_table_after_edit();
            self.preferred_x = None;
            self.reset_caret_blink();
            self.focus_handle.focus(window, cx);
            cx.emit(LiveEditorEvent::Changed);
            cx.notify();
        }
    }

    fn prepare_layout_for_bounds(
        &mut self,
        bounds: Bounds<Pixels>,
        padding_y: f32,
        window: &Window,
        cx: &App,
    ) -> (ColumnGeometry, f32, Vec<usize>, bool) {
        let vp_w = f32::from(bounds.size.width);
        let vp_h = f32::from(bounds.size.height);
        let geo = compute_column_geometry(vp_w, self.centered, self.zen_mode);
        self.last_col_geo = Some((geo.col_left_rel, geo.col_width, geo.top_pad));

        let total_lines = self.doc.lines.len();
        if total_lines == 0 {
            let st = self.scroll.borrow();
            return (geo, st.scroll_y, Vec::new(), false);
        }

        let scroll_y = {
            let mut st = self.scroll.borrow_mut();
            st.viewport_bounds = bounds;
            st.scroll_y
        };

        let rel_start_y = (scroll_y - geo.top_pad - padding_y).max(0.0);
        let rel_end_y = (scroll_y + vp_h - geo.top_pad + padding_y).max(0.0);
        let first_li = self.height_table.line_for_y(rel_start_y);
        let last_li = self
            .height_table
            .line_for_y(rel_end_y)
            .min(total_lines - 1);
        let scan_start = first_li.saturating_sub(64);

        let (prose_font, mono_font) = self.resolved_fonts(window, cx);
        let mut cur_y = self.height_table.y_for_line(scan_start);
        let mut active_floats: Vec<FloatExclusion> = Vec::new();
        let mut max_float_y_end = 0.0f32;
        let mut table_layouts: HashMap<usize, TableBlockLayout> = HashMap::new();
        let mut visible_indices = Vec::with_capacity(last_li.saturating_sub(first_li) + 1);
        let mut any_image_visible = false;

        for ix in scan_start..=last_li {
            active_floats.retain(|f| f.y_end > cur_y + 1.0);

            let dl = self.doc.lines[ix].clone();
            let clears_float = matches!(
                &dl.ctx,
                LineBlockContext::Heading { level: 1 | 2, .. }
                    | LineBlockContext::Table { .. }
                    | LineBlockContext::CodeFence { .. }
                    | LineBlockContext::Frontmatter { .. }
                    | LineBlockContext::MdxBlock { .. }
                    | LineBlockContext::ImageEmbed { .. }
            );

            let float_layout = if clears_float {
                if !active_floats.is_empty() {
                    let max_end = active_floats
                        .iter()
                        .map(|f| f.y_end)
                        .fold(cur_y, f32::max);
                    let spacer = (max_end - cur_y + 8.0).max(0.0);
                    active_floats.clear();
                    LineFloatLayout {
                        left_inset: 0.0,
                        right_inset: 0.0,
                        clear_top_spacer: spacer,
                    }
                } else {
                    LineFloatLayout::default()
                }
            } else {
                let mut left_inset = 0.0f32;
                let mut right_inset = 0.0f32;
                for f in &active_floats {
                    match f.side {
                        ImageAlign::Left => left_inset = left_inset.max(f.width),
                        ImageAlign::Right => right_inset = right_inset.max(f.width),
                        ImageAlign::Center => {}
                    }
                }
                let max_insets = (geo.col_width - 140.0).max(0.0);
                if left_inset + right_inset > max_insets && left_inset + right_inset > 0.0 {
                    let scale = max_insets / (left_inset + right_inset);
                    left_inset *= scale;
                    right_inset *= scale;
                }
                LineFloatLayout {
                    left_inset,
                    right_inset,
                    clear_top_spacer: 0.0,
                }
            };

            let resolved_img_spec = if let LineBlockContext::ImageEmbed { target, alt } = &dl.ctx {
                if ix >= first_li {
                    any_image_visible = true;
                }
                Some(self.resolve_image_box(ix, target, alt, geo.col_width))
            } else {
                None
            };
            let resolved_img = resolved_img_spec
                .as_ref()
                .map(|(w, h, s)| (*w, *h, s.is_floated()));
            let expected_img_key = resolved_img
                .map(|(w, h, _)| (w.round() as u32, h.round() as u32));

            let raw_line = &self.doc.text[dl.byte_start..dl.byte_end];
            let cursor_local = cursor_range_for_line(&self.doc, &dl, true);
            let cursor_key = cursor_local.as_ref().map(|r| (r.start, r.end));
            let (table_span, table_active, delim_expanded) =
                table_state_for_line(&self.doc, &dl.ctx);

            let hit = self.layout_cache.get(&ix).is_some_and(|c| {
                c.doc_version == self.doc.version
                    && c.cursor_key == cursor_key
                    && (c.wrap_width - geo.col_width).abs() < 0.5
                    && c.raw_len == raw_line.len()
                    && c.table_active == table_active
                    && c.delim_expanded == delim_expanded
                    && (c.extra_left_inset - float_layout.left_inset).abs() < 0.5
                    && (c.extra_right_inset - float_layout.right_inset).abs() < 0.5
                    && (c.clear_top_spacer - float_layout.clear_top_spacer).abs() < 0.5
                    && c.img_dims_key == expected_img_key
            });

            let total_h = if hit {
                let c = &self.layout_cache[&ix];
                let h = c.total_height;
                self.height_table.set_height(ix, h);
                h
            } else {
                let tbl_ref = table_span.map(|(s, e)| {
                    &*table_layouts
                        .entry(s)
                        .or_insert_with(|| compute_table_layout(&self.doc, s, e, geo.col_width))
                });
                let cached = shape_cached_line(
                    &dl,
                    raw_line,
                    cursor_local,
                    geo.col_width,
                    self.doc.version,
                    tbl_ref,
                    table_active,
                    delim_expanded,
                    float_layout,
                    resolved_img,
                    &prose_font,
                    &mono_font,
                    window,
                );
                let h = cached.total_height;
                self.height_table.set_height(ix, h);
                self.layout_cache.insert(ix, Rc::new(cached));
                h
            };

            if let Some((img_w, img_h, ref spec)) = resolved_img_spec
                && spec.is_floated()
            {
                let text_h = (total_h - float_layout.clear_top_spacer - 4.0).max(0.0);
                let img_y_start = cur_y + float_layout.clear_top_spacer + text_h + 6.0;
                let img_y_end = img_y_start + img_h;
                max_float_y_end = max_float_y_end.max(img_y_end);
                active_floats.push(FloatExclusion {
                    y_end: img_y_end,
                    side: spec.align,
                    width: img_w + 18.0,
                });
                any_image_visible = true;
            }

            cur_y += total_h;
            if ix >= first_li {
                visible_indices.push(ix);
            }
        }

        self.max_float_y_end = max_float_y_end;
        let content_total_h = self.height_table.total_height().max(max_float_y_end + 12.0);
        {
            let mut st = self.scroll.borrow_mut();
            st.total_height = geo.top_pad + content_total_h + (vp_h * 0.45).max(160.0);
            LiveEditorScrollbarHandle::clamp_scroll(&mut st);
        }
        let settled_scroll_y = self.scroll.borrow().scroll_y;
        (geo, settled_scroll_y, visible_indices, any_image_visible)
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

    fn ensure_caret_visible(&mut self) {
        let li = self.doc.line_index_for_byte(self.doc.head);
        let y_top = self.height_table.y_for_line(li);
        let line_h = self
            .layout_cache
            .get(&li)
            .map_or(28.0, |c| c.total_height);
        let top_pad = if self.zen_mode { 56.0 } else { 20.0 };
        let abs_top = top_pad + y_top;
        let abs_bottom = abs_top + line_h;

        let mut st = self.scroll.borrow_mut();
        let vp_h = f32::from(st.viewport_bounds.size.height);
        if vp_h <= 20.0 {
            return;
        }
        if abs_top < st.scroll_y + 12.0 {
            st.scroll_y = (abs_top - 16.0).max(0.0);
        } else if abs_bottom > st.scroll_y + vp_h - 16.0 {
            st.scroll_y = (abs_bottom - vp_h + 24.0).max(0.0);
        }
        LiveEditorScrollbarHandle::clamp_scroll(&mut st);
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
        window: &Window,
        cx: &App,
    ) -> Option<Rc<CachedLine>> {
        let dl = self.doc.lines.get(li)?.clone();
        let col_w = self.last_col_geo.map_or(760.0, |g| g.1);
        let cursor_local = cursor_range_for_line(&self.doc, &dl, true);
        let key = cursor_local.as_ref().map(|r| (r.start, r.end));
        let raw_len = dl.byte_end - dl.byte_start;
        let (table_span, table_active, delim_expanded) = table_state_for_line(&self.doc, &dl.ctx);

        let hit = self.layout_cache.get(&li).is_some_and(|c| {
            c.doc_version == self.doc.version
                && c.cursor_key == key
                && (c.wrap_width - col_w).abs() < 0.5
                && c.raw_len == raw_len
                && c.table_active == table_active
                && c.delim_expanded == delim_expanded
        });
        if !hit {
            let prev_float = self
                .layout_cache
                .get(&li)
                .map(|c| LineFloatLayout {
                    left_inset: c.extra_left_inset,
                    right_inset: c.extra_right_inset,
                    clear_top_spacer: c.clear_top_spacer,
                })
                .unwrap_or_default();
            let resolved_img = if let LineBlockContext::ImageEmbed { target, alt } = &dl.ctx {
                let (w, h, s) = self.resolve_image_box(li, target, alt, col_w);
                Some((w, h, s.is_floated()))
            } else {
                None
            };
            let tbl = table_span.map(|(s, e)| compute_table_layout(&self.doc, s, e, col_w));
            let (prose, mono) = self.resolved_fonts(window, cx);
            let raw_line = &self.doc.text[dl.byte_start..dl.byte_end];
            let cached = shape_cached_line(
                &dl,
                raw_line,
                cursor_local,
                col_w,
                self.doc.version,
                tbl.as_ref(),
                table_active,
                delim_expanded,
                prev_float,
                resolved_img,
                &prose,
                &mono,
                window,
            );
            self.height_table.set_height(li, cached.total_height);
            self.layout_cache.insert(li, Rc::new(cached));
        }
        self.layout_cache.get(&li).cloned()
    }

    fn hit_test_byte_at_point(&mut self, pos: Point<Pixels>) -> Option<HitTestResult> {
        let st = self.scroll.borrow();
        let bounds = st.viewport_bounds;
        if bounds.size.width <= px(1.) || self.doc.lines.is_empty() {
            return None;
        }
        let doc_y = (f32::from(pos.y - bounds.origin.y) + st.scroll_y).max(0.0);
        let (col_left_rel, col_width, top_pad) = self.last_col_geo.unwrap_or_else(|| {
            let g = compute_column_geometry(f32::from(bounds.size.width), self.centered, self.zen_mode);
            (g.col_left_rel, g.col_width, g.top_pad)
        });
        let rel_doc_y = (doc_y - top_pad).max(0.0);
        let scroll_y = st.scroll_y;
        drop(st);

        if rel_doc_y >= self.height_table.total_height() {
            return Some(HitTestResult {
                raw_byte: self.doc.text.len(),
                line_ix: self.doc.lines.len() - 1,
                clicked_link: None,
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
                clicked_link: None,
                clicked_checkbox: false,
                phantom_table_cell: None,
            });
        };

        // Check checkbox click first.
        if let Some(cb_rect) = cached.checkbox_rect {
            let expanded = Bounds {
                origin: point(cb_rect.origin.x - px(4.), cb_rect.origin.y - px(4.)),
                size: size(cb_rect.size.width + px(8.), cb_rect.size.height + px(8.)),
            };
            if expanded.contains(&pos) {
                return Some(HitTestResult {
                    raw_byte: dl.byte_start,
                    line_ix: li,
                    clicked_link: None,
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
                    clicked_link: None,
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
                    clicked_link: None,
                    clicked_checkbox: false,
                    phantom_table_cell: None,
                });
            };
            let num_rows = wl.wrap_boundaries.len() + 1;
            let cell_text_top = bounds.origin.y
                + px(top_pad + y_line - scroll_y + tr.frame_top_margin + tr.cell_pad_y);
            let cell_text_left = col_left + px(cell.x_rel + TABLE_CELL_PAD_X);
            let cell_inner_w = (cell.width - 2.0 * TABLE_CELL_PAD_X).max(20.0);
            let max_local_y = (num_rows as f32 * cached.line_height - 0.5).max(0.0);
            let local_y = f32::from(pos.y - cell_text_top).clamp(0.0, max_local_y);
            let wrap_row = ((local_y / cached.line_height).floor() as usize)
                .min(num_rows.saturating_sub(1));
            let align_dx = row_align_offset_x(
                Some(wl),
                wrap_row,
                gpui_text_align(cell.align),
                cell_inner_w,
                px(cached.line_height),
            );
            let local_x = (f32::from(pos.x - cell_text_left) - align_dx).max(0.0);
            let disp_idx = match wl
                .closest_index_for_position(point(px(local_x), px(local_y)), px(cached.line_height))
            {
                Ok(i) | Err(i) => i,
            }
            .min(cell.projected.display_text.len());

            let mut clicked_link = None;
            if let Ok(exact_i) =
                wl.index_for_position(point(px(local_x), px(local_y)), px(cached.line_height))
            {
                for link in &cell.projected.links {
                    if link.concealed && exact_i >= link.disp_range.start && exact_i < link.disp_range.end {
                        clicked_link = Some(link.target.clone());
                        break;
                    }
                }
            }
            let local_raw = cell.raw_offset + cell.projected.display_to_raw(cell_raw, disp_idx);
            let abs_raw = clamp_char_boundary(&self.doc.text, dl.byte_start + local_raw);
            return Some(HitTestResult {
                raw_byte: abs_raw,
                line_ix: li,
                clicked_link,
                clicked_checkbox: false,
                phantom_table_cell: None,
            });
        }

        let text_left = col_left + px(cached.text_x_rel);
        let text_top = bounds.origin.y + px(top_pad + y_line - scroll_y + cached.text_y_rel);

        let Some(wl) = cached.shaped.as_ref() else {
            return Some(HitTestResult {
                raw_byte: dl.byte_start,
                line_ix: li,
                clicked_link: None,
                clicked_checkbox: false,
                phantom_table_cell: None,
            });
        };

        let num_rows = wl.wrap_boundaries.len() + 1;
        let max_local_y = (num_rows as f32 * cached.line_height - 0.5).max(0.0);
        let local_x = f32::from(pos.x - text_left).max(0.0);
        let local_y = f32::from(pos.y - text_top).clamp(0.0, max_local_y);
        let _ = col_width;

        let disp_idx = match wl
            .closest_index_for_position(point(px(local_x), px(local_y)), px(cached.line_height))
        {
            Ok(i) | Err(i) => i,
        }
        .min(cached.projected.display_text.len());

        // Check link click only if user clicked strictly inside a concealed link glyph span.
        let mut clicked_link = None;
        if let Ok(exact_i) =
            wl.index_for_position(point(px(local_x), px(local_y)), px(cached.line_height))
        {
            for link in &cached.projected.links {
                if link.concealed
                    && exact_i >= link.disp_range.start
                    && exact_i < link.disp_range.end
                {
                    clicked_link = Some(link.target.clone());
                    break;
                }
            }
        }

        let local_raw = cached.projected.display_to_raw(raw_line, disp_idx);
        let abs_raw = clamp_char_boundary(&self.doc.text, dl.byte_start + local_raw);
        Some(HitTestResult {
            raw_byte: abs_raw,
            line_ix: li,
            clicked_link,
            clicked_checkbox: false,
            phantom_table_cell: None,
        })
    }

    fn move_vertical(
        &mut self,
        delta: isize,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .selected_image_line
            .is_some_and(|img_li| self.doc.line_index_for_byte(self.doc.head) != img_li)
        {
            self.selected_image_line = None;
        }
        if self.doc.lines.is_empty() {
            return;
        }
        self.doc.break_undo_coalescing();
        if !extend && self.doc.has_selection() {
            let sel = self.doc.selection_range();
            let target_edge = if delta < 0 { sel.start } else { sel.end };
            self.doc.set_cursor(target_edge, false);
        }

        let li = self.doc.line_index_for_byte(self.doc.head);
        let dl = self.doc.lines[li].clone();
        let local_head = self
            .doc
            .head
            .saturating_sub(dl.byte_start)
            .min(dl.byte_end - dl.byte_start);
        let (cur_x, vis_row, num_rows, cur_col_ix) =
            if let Some(c) = self.ensure_line_cached(li, window, cx) {
                let (vx, _, _) = raw_offset_to_visual_point(&c, local_head);
                if let Some(tr) = &c.table_row {
                    let cix = find_table_cell_for_local_raw(tr, local_head).unwrap_or(0);
                    let cell = &tr.cells[cix];
                    let cell_local = local_head
                        .saturating_sub(cell.raw_offset)
                        .min(cell.projected.raw_len);
                    let disp_idx = cell.projected.raw_to_display(cell_local);
                    let pt = disp_position_for_index(
                        cell.shaped.as_ref(),
                        cell.projected.display_text.len(),
                        disp_idx,
                        px(c.line_height),
                    );
                    let row = (f32::from(pt.y) / c.line_height).round().max(0.0) as usize;
                    let rows = cell
                        .shaped
                        .as_ref()
                        .map_or(1, |wl| wl.wrap_boundaries.len() + 1);
                    (vx, row, rows, Some(cix))
                } else {
                    let disp_idx = c.projected.raw_to_display(local_head);
                    let pt = disp_position_for_index(
                        c.shaped.as_ref(),
                        c.projected.display_text.len(),
                        disp_idx,
                        px(c.line_height),
                    );
                    let row = (f32::from(pt.y) / c.line_height).round().max(0.0) as usize;
                    (vx, row, c.num_wrap_rows(), None)
                }
            } else {
                (0.0, 0, 1, None)
            };

        let target_x = *self.preferred_x.get_or_insert(cur_x);

        let (mut target_li, target_row, same_cell_col) = if delta < 0 {
            if vis_row > 0 {
                (li, Some(vis_row - 1), cur_col_ix)
            } else if li > 0 {
                (li - 1, None, None)
            } else {
                self.doc.set_cursor(0, extend);
                self.ensure_caret_visible();
                cx.notify();
                return;
            }
        } else if vis_row + 1 < num_rows {
            (li, Some(vis_row + 1), cur_col_ix)
        } else if li + 1 < self.doc.lines.len() {
            (li + 1, Some(0), None)
        } else {
            self.doc.set_cursor(self.doc.text.len(), extend);
            self.ensure_caret_visible();
            cx.notify();
            return;
        };

        // Skip collapsed GFM table delimiter rows when stepping vertically.
        if same_cell_col.is_none()
            && matches!(
                self.doc.lines[target_li].ctx,
                LineBlockContext::Table { is_delim: true, .. }
            )
        {
            if delta < 0 {
                if target_li > 0 {
                    target_li -= 1;
                } else {
                    self.doc.set_cursor(0, extend);
                    self.ensure_caret_visible();
                    cx.notify();
                    return;
                }
            } else if target_li + 1 < self.doc.lines.len() {
                target_li += 1;
            } else {
                self.doc.set_cursor(self.doc.text.len(), extend);
                self.ensure_caret_visible();
                cx.notify();
                return;
            }
        }

        let target_start = self.doc.lines[target_li].byte_start;
        let target_raw = self.doc.line_str(target_li).to_string();
        let new_abs = if let Some(c) = self.ensure_line_cached(target_li, window, cx) {
            if let Some(tr) = &c.table_row {
                let mut col_ix = same_cell_col.unwrap_or_else(|| {
                    tr.cells
                        .iter()
                        .position(|cell| target_x < cell.x_rel + cell.width)
                        .unwrap_or_else(|| tr.cells.len().saturating_sub(1))
                });
                if tr.cells[col_ix].inner_range.is_none() {
                    col_ix = tr
                        .cells
                        .iter()
                        .rposition(|cell| cell.inner_range.is_some())
                        .unwrap_or(0);
                }
                let cell = &tr.cells[col_ix];
                let slice_end = (cell.raw_offset + cell.projected.raw_len).min(target_raw.len());
                let cell_raw = &target_raw[cell.raw_offset..slice_end];
                if let Some(wl) = cell.shaped.as_ref() {
                    let rows = wl.wrap_boundaries.len() + 1;
                    let row = target_row
                        .unwrap_or_else(|| rows.saturating_sub(1))
                        .min(rows - 1);
                    let inner_w = (cell.width - 2.0 * TABLE_CELL_PAD_X).max(20.0);
                    let align_dx = row_align_offset_x(
                        Some(wl),
                        row,
                        gpui_text_align(cell.align),
                        inner_w,
                        px(c.line_height),
                    );
                    let local_x = (target_x - (cell.x_rel + TABLE_CELL_PAD_X) - align_dx).max(0.0);
                    let y = row as f32 * c.line_height + c.line_height * 0.5;
                    let disp_idx = match wl.closest_index_for_position(
                        point(px(local_x), px(y)),
                        px(c.line_height),
                    ) {
                        Ok(i) | Err(i) => i,
                    }
                    .min(cell.projected.display_text.len());
                    let local_raw = cell.raw_offset + cell.projected.display_to_raw(cell_raw, disp_idx);
                    target_start + local_raw
                } else {
                    target_start + cell.raw_offset + cell.projected.display_to_raw(cell_raw, 0)
                }
            } else if let Some(wl) = c.shaped.as_ref() {
                let rows = wl.wrap_boundaries.len() + 1;
                let row = target_row
                    .unwrap_or_else(|| rows.saturating_sub(1))
                    .min(rows - 1);
                let local_x = (target_x - c.text_x_rel).max(0.0);
                let y = row as f32 * c.line_height + c.line_height * 0.5;
                let disp_idx = match wl
                    .closest_index_for_position(point(px(local_x), px(y)), px(c.line_height))
                {
                    Ok(i) | Err(i) => i,
                }
                .min(c.projected.display_text.len());
                let local_raw = c.projected.display_to_raw(&target_raw, disp_idx);
                target_start + local_raw
            } else {
                target_start
            }
        } else {
            target_start
        };

        self.doc.set_cursor(new_abs, extend);
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.notify();
    }

    fn move_by_page(&mut self, dir: isize, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let vp_h = f32::from(self.scroll.borrow().viewport_bounds.size.height).max(200.0);
        let cur_li = self.doc.line_index_for_byte(self.doc.head);
        let cur_y = self.height_table.y_for_line(cur_li);
        let next_y = (cur_y + dir as f32 * (vp_h * 0.85)).max(0.0);
        let next_li = self.height_table.line_for_y(next_y);
        {
            let mut st = self.scroll.borrow_mut();
            st.scroll_y = (st.scroll_y + dir as f32 * vp_h * 0.85).max(0.0);
            LiveEditorScrollbarHandle::clamp_scroll(&mut st);
        }
        let target_byte = self.doc.lines.get(next_li).map_or(0, |l| l.byte_start);
        self.doc.set_cursor(target_byte, extend);
        let _ = window;
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.notify();
    }

    fn handle_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        let ctrl = ev.keystroke.modifiers.control || ev.keystroke.modifiers.platform;
        let alt = ev.keystroke.modifiers.alt;
        let shift = ev.keystroke.modifiers.shift;

        match key {
            "left" if !alt => {
                cx.stop_propagation();
                self.preferred_x = None;
                self.doc.move_left(ctrl, shift);
                self.ensure_caret_visible();
                self.reset_caret_blink();
                cx.notify();
            }
            "right" if !alt => {
                cx.stop_propagation();
                self.preferred_x = None;
                self.doc.move_right(ctrl, shift);
                self.ensure_caret_visible();
                self.reset_caret_blink();
                cx.notify();
            }
            "up" if !alt && !ctrl => {
                cx.stop_propagation();
                self.move_vertical(-1, shift, window, cx);
            }
            "down" if !alt && !ctrl => {
                cx.stop_propagation();
                self.move_vertical(1, shift, window, cx);
            }
            "home" if !alt => {
                cx.stop_propagation();
                self.preferred_x = None;
                self.doc.break_undo_coalescing();
                if ctrl {
                    self.doc.set_cursor(0, shift);
                } else {
                    self.doc.move_line_start(shift);
                }
                self.ensure_caret_visible();
                self.reset_caret_blink();
                cx.notify();
            }
            "end" if !alt => {
                cx.stop_propagation();
                self.preferred_x = None;
                self.doc.break_undo_coalescing();
                if ctrl {
                    self.doc.set_cursor(self.doc.text.len(), shift);
                } else {
                    self.doc.move_line_end(shift);
                }
                self.ensure_caret_visible();
                self.reset_caret_blink();
                cx.notify();
            }
            "pageup" if !alt && !ctrl => {
                cx.stop_propagation();
                self.move_by_page(-1, shift, window, cx);
            }
            "pagedown" if !alt && !ctrl => {
                cx.stop_propagation();
                self.move_by_page(1, shift, window, cx);
            }
            "backspace" if !alt => {
                cx.stop_propagation();
                self.preferred_x = None;
                if let Some(img_li) = self.selected_image_line
                    && self.doc.line_index_for_byte(self.doc.head) != img_li
                {
                    if self.doc.delete_line(img_li) {
                        self.selected_image_line = None;
                        self.layout_cache.clear();
                        self.sync_height_table_after_edit();
                        self.ensure_caret_visible();
                        self.reset_caret_blink();
                        cx.emit(LiveEditorEvent::Changed);
                        cx.notify();
                    }
                    return;
                }
                self.doc.backspace(ctrl);
                self.sync_height_table_after_edit();
                self.ensure_caret_visible();
                self.reset_caret_blink();
                cx.emit(LiveEditorEvent::Changed);
                cx.notify();
            }
            "delete" if !alt => {
                cx.stop_propagation();
                self.preferred_x = None;
                if let Some(img_li) = self.selected_image_line
                    && self.doc.line_index_for_byte(self.doc.head) != img_li
                {
                    if self.doc.delete_line(img_li) {
                        self.selected_image_line = None;
                        self.layout_cache.clear();
                        self.sync_height_table_after_edit();
                        self.ensure_caret_visible();
                        self.reset_caret_blink();
                        cx.emit(LiveEditorEvent::Changed);
                        cx.notify();
                    }
                    return;
                }
                self.doc.delete(ctrl);
                self.sync_height_table_after_edit();
                self.ensure_caret_visible();
                self.reset_caret_blink();
                cx.emit(LiveEditorEvent::Changed);
                cx.notify();
            }
            "enter" if !alt => {
                cx.stop_propagation();
                self.preferred_x = None;
                if let Some(img_li) = self.selected_image_line
                    && self.doc.line_index_for_byte(self.doc.head) != img_li
                {
                    self.selected_image_line = None;
                    if img_li + 1 < self.doc.lines.len() {
                        let next_start = self.doc.lines[img_li + 1].byte_start;
                        self.doc.set_cursor(next_start, false);
                    } else if let Some(dl) = self.doc.lines.get(img_li) {
                        let end = dl.byte_end;
                        self.doc.replace_range(end..end, "\n", EditKind::Atomic);
                        self.sync_height_table_after_edit();
                        cx.emit(LiveEditorEvent::Changed);
                    }
                    self.ensure_caret_visible();
                    self.reset_caret_blink();
                    cx.notify();
                    return;
                }
                if ctrl {
                    let li = self.doc.line_index_for_byte(self.doc.head);
                    if self.doc.toggle_task_on_line(li) {
                        self.sync_height_table_after_edit();
                        self.reset_caret_blink();
                        cx.emit(LiveEditorEvent::Changed);
                        cx.notify();
                    }
                    return;
                }
                self.doc.smart_enter();
                self.sync_height_table_after_edit();
                self.ensure_caret_visible();
                self.reset_caret_blink();
                cx.emit(LiveEditorEvent::Changed);
                cx.notify();
            }
            "tab" if !ctrl && !alt => {
                cx.stop_propagation();
                self.handle_tab_key(shift, cx);
            }
            "escape" => {
                if self.active_image_resize.take().is_some() {
                    cx.stop_propagation();
                    self.dragging = false;
                    cx.notify();
                    return;
                }
                if self.selected_image_line.take().is_some() {
                    cx.stop_propagation();
                    self.reset_caret_blink();
                    cx.notify();
                    return;
                }
                if self.doc.has_selection() {
                    cx.stop_propagation();
                    self.doc.set_cursor(self.doc.head, false);
                    self.reset_caret_blink();
                    cx.notify();
                }
            }
            "a" if ctrl && !alt && !shift => {
                cx.stop_propagation();
                self.doc.select_all();
                cx.notify();
            }
            "c" if ctrl && !alt && !shift => {
                cx.stop_propagation();
                if self.doc.has_selection() {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        self.doc.selected_text().to_string(),
                    ));
                }
            }
            "x" if ctrl && !alt && !shift => {
                cx.stop_propagation();
                if self.doc.has_selection() {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        self.doc.selected_text().to_string(),
                    ));
                    let sel = self.doc.selection_range();
                    self.doc.replace_range(sel, "", EditKind::Atomic);
                    self.sync_height_table_after_edit();
                    self.ensure_caret_visible();
                    self.reset_caret_blink();
                    cx.emit(LiveEditorEvent::Changed);
                    cx.notify();
                }
            }
            "v" if ctrl && !alt && !shift => {
                cx.stop_propagation();
                if let Some(item) = cx.read_from_clipboard() {
                    let has_image = item
                        .entries()
                        .iter()
                        .any(|e| matches!(e, ClipboardEntry::Image(_)));
                    if has_image {
                        cx.emit(LiveEditorEvent::PasteImage(item));
                    } else if let Some(txt) = item.text() {
                        self.doc.insert_text(&txt);
                        self.sync_height_table_after_edit();
                        self.preferred_x = None;
                        self.ensure_caret_visible();
                        self.reset_caret_blink();
                        cx.emit(LiveEditorEvent::Changed);
                        cx.notify();
                    }
                }
            }
            "z" if ctrl && !alt => {
                cx.stop_propagation();
                let changed = if shift {
                    self.doc.redo()
                } else {
                    self.doc.undo()
                };
                if changed {
                    self.sync_height_table_after_edit();
                    self.preferred_x = None;
                    self.ensure_caret_visible();
                    self.reset_caret_blink();
                    cx.emit(LiveEditorEvent::Changed);
                    cx.notify();
                }
            }
            "y" if ctrl && !alt && !shift => {
                cx.stop_propagation();
                if self.doc.redo() {
                    self.sync_height_table_after_edit();
                    self.preferred_x = None;
                    self.ensure_caret_visible();
                    self.reset_caret_blink();
                    cx.emit(LiveEditorEvent::Changed);
                    cx.notify();
                }
            }
            "i" if ctrl && !alt && !shift => {
                cx.stop_propagation();
                self.doc.toggle_wrap("*");
                self.sync_height_table_after_edit();
                self.reset_caret_blink();
                cx.emit(LiveEditorEvent::Changed);
                cx.notify();
            }
            "k" if ctrl && !alt && !shift => {
                cx.stop_propagation();
                self.doc.insert_or_wrap_link();
                self.sync_height_table_after_edit();
                self.reset_caret_blink();
                cx.emit(LiveEditorEvent::Changed);
                cx.notify();
            }
            _ => {}
        }
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
        let s = utf8_to_utf16_offset(&self.doc.text, sel.start);
        let e = utf8_to_utf16_offset(&self.doc.text, sel.end);
        Some(UTF16Selection {
            range: s..e,
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
        let has_image = item
            .entries()
            .iter()
            .any(|e| matches!(e, ClipboardEntry::Image(_)));
        if has_image {
            cx.emit(LiveEditorEvent::PasteImage(item));
            return;
        }
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
        if text.is_empty() && range_utf16.is_none() && !self.doc.has_selection() {
            return;
        }
        if self
            .selected_image_line
            .is_some_and(|img_li| self.doc.line_index_for_byte(self.doc.head) != img_li)
        {
            self.selected_image_line = None;
        }
        self.doc.marked_range = None;
        if let Some(r16) = range_utf16 {
            let s = utf16_to_utf8_offset(&self.doc.text, r16.start);
            let e = utf16_to_utf8_offset(&self.doc.text, r16.end);
            self.doc.replace_range(s..e, text, EditKind::Atomic);
        } else {
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
        let dl = self.doc.lines.get(li)?;
        let st = self.scroll.borrow();
        let (col_left_rel, _, top_pad) = self.last_col_geo?;
        let cached = self.layout_cache.get(&li)?;
        let local_raw = pos_u8.saturating_sub(dl.byte_start);
        let (vx, vy, lh) = raw_offset_to_visual_point(cached, local_raw);
        let origin_x = element_bounds.origin.x + px(col_left_rel + vx);
        let origin_y =
            element_bounds.origin.y + px(top_pad + self.height_table.y_for_line(li) - st.scroll_y + vy);
        Some(Bounds {
            origin: point(origin_x, origin_y),
            size: size(px(2.), px(lh)),
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

impl LiveMarkdownEditor {
    fn run_table_action<F>(&mut self, window: &mut Window, cx: &mut Context<Self>, action: F)
    where
        F: FnOnce(&mut LiveDocBuffer) -> bool,
    {
        if action(&mut self.doc) {
            self.sync_height_table_after_edit();
            self.preferred_x = None;
            self.ensure_caret_visible();
            self.reset_caret_blink();
            self.focus_handle.focus(window, cx);
            cx.emit(LiveEditorEvent::Changed);
            cx.notify();
        }
    }

    fn commit_active_image_resize(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(resize) = self.active_image_resize.take() else {
            return false;
        };
        let mut new_spec = resize.spec;
        let target_w = resize.preview_width.round().max(48.0);
        if (target_w - resize.start_width.round()).abs() >= 1.0 || new_spec.width.is_none() {
            new_spec.width = Some(target_w);
            new_spec.height = None;
            if self.doc.update_image_embed_spec(resize.line_ix, &new_spec) {
                self.layout_cache.clear();
                self.sync_height_table_after_edit();
                cx.emit(LiveEditorEvent::Changed);
            }
        }
        self.selected_image_line = Some(resize.line_ix);
        true
    }

    fn handle_tab_key(&mut self, shift: bool, cx: &mut Context<Self>) {
        if self
            .selected_image_line
            .is_some_and(|img_li| self.doc.line_index_for_byte(self.doc.head) != img_li)
        {
            self.selected_image_line = None;
        }
        self.preferred_x = None;
        let prev_ver = self.doc.version;
        if !self.doc.table_tab_move(shift) {
            self.doc.indent_lines(shift);
        }
        if self.doc.version != prev_ver {
            self.sync_height_table_after_edit();
            cx.emit(LiveEditorEvent::Changed);
        }
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.notify();
    }
}

fn image_tool_pill(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    active: bool,
    on_click: impl Fn(&mut LiveMarkdownEditor, &mut Window, &mut Context<LiveMarkdownEditor>) + 'static,
    ed: Entity<LiveMarkdownEditor>,
) -> AnyElement {
    let text_col = if active {
        hex_color(kd::ACCENT)
    } else {
        hex_color(kd::FG_SUBTLE)
    };
    let bg_col = if active {
        accent_alpha(0x28)
    } else {
        gpui_kit::transparent_black()
    };
    div()
        .id(id.into())
        .h(px(20.))
        .px_1p5()
        .rounded(px(4.))
        .bg(bg_col)
        .cursor_pointer()
        .flex()
        .items_center()
        .text_size(px(11.))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::MEDIUM
        })
        .text_color(text_col)
        .hover(move |s| {
            s.bg(if active {
                accent_alpha(0x36)
            } else {
                hex_color(kd::BG_PANEL)
            })
            .text_color(hex_color(kd::FG))
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            ed.update(cx, |this, cx| {
                on_click(this, window, cx);
            });
        })
        .child(label.into())
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn image_resize_handle(
    ix: usize,
    corner: ResizeHandleCorner,
    img_w: f32,
    img_h: f32,
    align: ImageAlign,
    spec: StyledImageSpec,
    max_width: f32,
    ed: Entity<LiveMarkdownEditor>,
) -> AnyElement {
    let is_edge = matches!(
        corner,
        ResizeHandleCorner::LeftEdge | ResizeHandleCorner::RightEdge
    );
    let id: SharedString = format!("live-img-handle-{ix}-{corner:?}").into();
    let mut handle = div().id(id).absolute();
    handle = match corner {
        ResizeHandleCorner::LeftEdge | ResizeHandleCorner::RightEdge => handle.cursor_ew_resize(),
        ResizeHandleCorner::TopLeft | ResizeHandleCorner::BottomRight => {
            handle.cursor_nwse_resize()
        }
        ResizeHandleCorner::TopRight | ResizeHandleCorner::BottomLeft => {
            handle.cursor_nesw_resize()
        }
    };

    if is_edge {
        let pill_h = 30.0_f32.min((img_h - 30.0).max(10.0));
        let top_off = ((img_h - pill_h) * 0.5).max(14.0);
        handle = handle
            .w(px(5.))
            .h(px(pill_h))
            .top(px(top_off))
            .rounded(px(2.5))
            .bg(hex_color(kd::ACCENT))
            .border_1()
            .border_color(hex_color(kd::BG_MAIN))
            .shadow_sm();
        handle = match corner {
            ResizeHandleCorner::LeftEdge => handle.left(px(5.)),
            _ => handle.right(px(5.)),
        };
    } else {
        handle = handle
            .w(px(11.))
            .h(px(11.))
            .rounded(px(5.5))
            .bg(hex_color(kd::BG_MAIN))
            .border_2()
            .border_color(hex_color(kd::ACCENT))
            .shadow_sm();
        handle = match corner {
            ResizeHandleCorner::TopLeft => handle.top(px(4.)).left(px(4.)),
            ResizeHandleCorner::TopRight => handle.top(px(4.)).right(px(4.)),
            ResizeHandleCorner::BottomLeft => handle.bottom(px(4.)).left(px(4.)),
            _ => handle.bottom(px(4.)).right(px(4.)),
        };
    }

    handle
        .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
            cx.stop_propagation();
            ed.update(cx, |this, cx| {
                this.active_image_resize = Some(ActiveImageResize {
                    line_ix: ix,
                    corner,
                    start_mouse: ev.position,
                    start_width: img_w,
                    aspect_ratio: (img_w / img_h.max(1.0)).clamp(0.15, 6.5),
                    preview_width: img_w,
                    preview_height: img_h,
                    align,
                    spec: spec.clone(),
                    max_width,
                });
                this.selected_image_line = Some(ix);
                this.dragging = false;
                this.focus_handle.focus(window, cx);
                cx.notify();
            });
        })
        .into_any_element()
}

fn table_tool_pill(
    id: &'static str,
    label: impl Into<SharedString>,
    accent: bool,
    danger: bool,
    ed: Entity<LiveMarkdownEditor>,
    on_press: impl Fn(&mut LiveDocBuffer) -> bool + 'static,
) -> AnyElement {
    let text_col = if danger {
        hex_color(kd::RED)
    } else if accent {
        hex_color(kd::ACCENT)
    } else {
        hex_color(kd::FG_SUBTLE)
    };
    div()
        .id(id)
        .h(px(18.))
        .px_1p5()
        .rounded(px(4.))
        .cursor_pointer()
        .flex()
        .items_center()
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(text_col)
        .hover(move |s| {
            s.bg(hex_color(kd::BG_PANEL)).text_color(if danger {
                hex_color(kd::RED)
            } else {
                hex_color(kd::FG)
            })
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            ed.update(cx, |this, cx| {
                this.run_table_action(window, cx, &on_press);
            });
        })
        .child(label.into())
        .into_any_element()
}

impl Render for LiveMarkdownEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scroll_state = self.scroll.borrow();
        let vp_bounds = scroll_state.viewport_bounds;
        let vp_w = f32::from(vp_bounds.size.width);
        let vp_h = f32::from(vp_bounds.size.height);
        drop(scroll_state);

        // Build visible ImageEmbed overlays & active-table micro-toolbar on the same frame.
        let mut image_overlays: Vec<AnyElement> = Vec::new();
        let mut table_toolbar: Option<AnyElement> = None;
        if vp_w > 20.0 && vp_h > 20.0 && !self.doc.lines.is_empty() {
            let (col_geo, scroll_y, _, _) =
                self.prepare_layout_for_bounds(vp_bounds, 360.0, window, cx);
            let rel_start_y = (scroll_y - col_geo.top_pad - 360.0).max(0.0);
            let rel_end_y = (scroll_y + vp_h - col_geo.top_pad + 360.0).max(0.0);
            let first_li = self.height_table.line_for_y(rel_start_y);
            let last_li = self
                .height_table
                .line_for_y(rel_end_y)
                .min(self.doc.lines.len() - 1);
            let overlay_start = first_li.saturating_sub(48);
            let ed_handle = cx.entity();

            for ix in overlay_start..=last_li {
                let Some((target, alt, line_start_byte, cursor_present)) =
                    self.doc.lines.get(ix).and_then(|dl| {
                        if let LineBlockContext::ImageEmbed { target, alt } = &dl.ctx {
                            let cur = cursor_range_for_line(&self.doc, dl, true).is_some();
                            Some((target.clone(), alt.clone(), dl.byte_start, cur))
                        } else {
                            None
                        }
                    })
                else {
                    continue;
                };

                let y_line = self.height_table.y_for_line(ix);
                let (clear_top_spacer, text_h) = if let Some(cached) = self.layout_cache.get(&ix) {
                    let th = if cursor_present && !cached.projected.display_text.is_empty() {
                        let rows = cached
                            .shaped
                            .as_ref()
                            .map_or(1, |wl| wl.wrap_boundaries.len() + 1) as f32;
                        rows * cached.line_height + 4.0
                    } else {
                        0.0
                    };
                    (cached.clear_top_spacer, th)
                } else {
                    (0.0, if cursor_present { 24.0 } else { 0.0 })
                };

                let (img_w, img_h, spec) =
                    self.resolve_image_box(ix, &target, &alt, col_geo.col_width);
                let img_top = col_geo.top_pad + y_line + clear_top_spacer - scroll_y + text_h + 6.0;
                if img_top + img_h < -40.0 || img_top > vp_h + 40.0 {
                    continue;
                }

                let img_left = match spec.align {
                    ImageAlign::Left => col_geo.col_left_rel,
                    ImageAlign::Center => {
                        col_geo.col_left_rel + ((col_geo.col_width - img_w) * 0.5).max(0.0)
                    }
                    ImageAlign::Right => {
                        col_geo.col_left_rel + (col_geo.col_width - img_w).max(0.0)
                    }
                };

                let is_resizing = self
                    .active_image_resize
                    .as_ref()
                    .is_some_and(|r| r.line_ix == ix);
                let is_selected = self.selected_image_line == Some(ix) || is_resizing;
                let max_drag_w = if spec.is_floated() {
                    (col_geo.col_width - 140.0).max(150.0)
                } else {
                    (col_geo.col_width - 16.0).max(120.0)
                };
                let radius = if spec.rounded { px(8.) } else { px(0.) };

                let resolved = resolve_image_file(&target, &self.doc_path, &self.vault);
                let target_name = target.clone();
                let mut img_frame = div()
                    .size_full()
                    .rounded(radius)
                    .overflow_hidden()
                    .when(spec.border && !is_selected, |d| {
                        d.border_1()
                            .border_color(hex_color(kd::BORDER))
                            .bg(hex_alpha(0x0e0c0f80))
                            .p_1()
                    })
                    .child(if let Some(path) = resolved {
                        img(path)
                            .size_full()
                            .object_fit(ObjectFit::Contain)
                            .rounded(radius)
                            .into_any_element()
                    } else {
                        div()
                            .size_full()
                            .rounded(radius)
                            .bg(hex_color(kd::BG_SIDEBAR))
                            .border_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .text_color(hex_color(kd::FG_MUTED))
                            .child(format!("Image not found: {target_name}"))
                            .into_any_element()
                    });

                if is_selected {
                    img_frame = img_frame
                        .border_2()
                        .border_color(hex_color(kd::ACCENT));
                }

                let mut ov = div()
                    .id(SharedString::from(format!("live-img-{ix}")))
                    .absolute()
                    .top(px(img_top))
                    .left(px(img_left))
                    .w(px(img_w))
                    .h(px(img_h))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.dragging = false;
                            this.selected_image_line = Some(ix);
                            if ev.click_count >= 2 {
                                this.doc.set_cursor(line_start_byte, false);
                                this.preferred_x = None;
                            }
                            this.focus_handle.focus(window, cx);
                            this.reset_caret_blink();
                            cx.notify();
                        }),
                    )
                    .child(img_frame);

                if !is_resizing && let Some(caption) = spec.caption.clone() {
                    ov = ov.child(
                        div()
                            .absolute()
                            .bottom(px(6.))
                            .left(px(8.))
                            .max_w(px((img_w - 16.0).max(40.0)))
                            .px_1p5()
                            .py_0p5()
                            .rounded(px(4.))
                            .bg(hex_alpha(0x0c0b0eb8))
                            .text_size(px(11.))
                            .italic()
                            .text_color(hex_color(kd::FG_SUBTLE))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(caption),
                    );
                }

                if is_selected {
                    for corner in [
                        ResizeHandleCorner::TopLeft,
                        ResizeHandleCorner::TopRight,
                        ResizeHandleCorner::BottomLeft,
                        ResizeHandleCorner::BottomRight,
                    ] {
                        ov = ov.child(image_resize_handle(
                            ix,
                            corner,
                            img_w,
                            img_h,
                            spec.align,
                            spec.clone(),
                            max_drag_w,
                            ed_handle.clone(),
                        ));
                    }
                    if img_h >= 68.0 {
                        for corner in [ResizeHandleCorner::LeftEdge, ResizeHandleCorner::RightEdge] {
                            ov = ov.child(image_resize_handle(
                                ix,
                                corner,
                                img_w,
                                img_h,
                                spec.align,
                                spec.clone(),
                                max_drag_w,
                                ed_handle.clone(),
                            ));
                        }
                    }
                }

                if is_resizing {
                    ov = ov.child(
                        div()
                            .absolute()
                            .bottom(px(10.))
                            .left(px(0.))
                            .w(px(img_w))
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded(px(4.))
                                    .bg(hex_alpha(0x0c0b0ed8))
                                    .border_1()
                                    .border_color(hex_color(kd::ACCENT))
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(hex_color(kd::FG))
                                    .child(format!(
                                        "{} × {} px",
                                        img_w.round() as u32,
                                        img_h.round() as u32
                                    )),
                            ),
                    );
                }

                image_overlays.push(ov.into_any_element());

                if is_selected && !is_resizing {
                    let toolbar_w = 242.0_f32;
                    let tb_left = (img_left + (img_w - toolbar_w) * 0.5)
                        .clamp(8.0, (vp_w - toolbar_w - 8.0).max(8.0));
                    let tb_top = if img_top >= 36.0 {
                        img_top - 32.0
                    } else {
                        img_top + 8.0
                    };

                    let spec_l = {
                        let mut s = spec.clone();
                        s.align = ImageAlign::Left;
                        s
                    };
                    let spec_c = {
                        let mut s = spec.clone();
                        s.align = ImageAlign::Center;
                        s.wrap = false;
                        s
                    };
                    let spec_r = {
                        let mut s = spec.clone();
                        s.align = ImageAlign::Right;
                        s
                    };
                    let spec_wrap = {
                        let mut s = spec.clone();
                        if s.align == ImageAlign::Center {
                            s.align = ImageAlign::Left;
                            s.wrap = true;
                        } else {
                            s.wrap = !s.wrap;
                        }
                        s
                    };
                    let spec_border = {
                        let mut s = spec.clone();
                        s.border = !s.border;
                        s
                    };
                    let spec_reset = {
                        let mut s = spec.clone();
                        s.width = None;
                        s.height = None;
                        s
                    };

                    let tb = div()
                        .id(SharedString::from(format!("live-img-tb-{ix}")))
                        .absolute()
                        .top(px(tb_top))
                        .left(px(tb_left))
                        .h(px(26.))
                        .px_1()
                        .gap_0p5()
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .bg(hex_color(kd::BG_SIDEBAR))
                        .border_1()
                        .border_color(hex_color(kd::BORDER))
                        .shadow_md()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .children([
                            image_tool_pill(
                                SharedString::from(format!("img-align-l-{ix}")),
                                "L",
                                spec.align == ImageAlign::Left,
                                move |ed, win, cx| {
                                    ed.update_image_spec_action(ix, spec_l.clone(), win, cx);
                                },
                                ed_handle.clone(),
                            ),
                            image_tool_pill(
                                SharedString::from(format!("img-align-c-{ix}")),
                                "C",
                                spec.align == ImageAlign::Center,
                                move |ed, win, cx| {
                                    ed.update_image_spec_action(ix, spec_c.clone(), win, cx);
                                },
                                ed_handle.clone(),
                            ),
                            image_tool_pill(
                                SharedString::from(format!("img-align-r-{ix}")),
                                "R",
                                spec.align == ImageAlign::Right,
                                move |ed, win, cx| {
                                    ed.update_image_spec_action(ix, spec_r.clone(), win, cx);
                                },
                                ed_handle.clone(),
                            ),
                            div()
                                .w(px(1.))
                                .h(px(14.))
                                .mx_0p5()
                                .bg(hex_color(kd::BORDER_SUBTLE))
                                .into_any_element(),
                            image_tool_pill(
                                SharedString::from(format!("img-wrap-{ix}")),
                                "Wrap",
                                spec.is_floated(),
                                move |ed, win, cx| {
                                    ed.update_image_spec_action(ix, spec_wrap.clone(), win, cx);
                                },
                                ed_handle.clone(),
                            ),
                            image_tool_pill(
                                SharedString::from(format!("img-frame-{ix}")),
                                "Frame",
                                spec.border,
                                move |ed, win, cx| {
                                    ed.update_image_spec_action(ix, spec_border.clone(), win, cx);
                                },
                                ed_handle.clone(),
                            ),
                            image_tool_pill(
                                SharedString::from(format!("img-reset-{ix}")),
                                "1:1",
                                false,
                                move |ed, win, cx| {
                                    ed.update_image_spec_action(ix, spec_reset.clone(), win, cx);
                                },
                                ed_handle.clone(),
                            ),
                            image_tool_pill(
                                SharedString::from(format!("img-raw-{ix}")),
                                "</>",
                                cursor_present,
                                move |ed, win, cx| {
                                    ed.doc.set_cursor(line_start_byte, false);
                                    ed.selected_image_line = Some(ix);
                                    ed.preferred_x = None;
                                    ed.focus_handle.focus(win, cx);
                                    ed.reset_caret_blink();
                                    cx.notify();
                                },
                                ed_handle.clone(),
                            ),
                        ]);
                    image_overlays.push(tb.into_any_element());
                }
            }

            let caret_li = self.doc.line_index_for_byte(self.doc.head);
            if let Some(caret_dl) = self.doc.lines.get(caret_li)
                && let LineBlockContext::Table {
                    is_header,
                    is_delim,
                    table_start_line,
                    table_end_line,
                    ..
                } = caret_dl.ctx
            {
                let delim_li = table_start_line + 1;
                let delim_expanded = is_delim
                    || self
                        .doc
                        .lines
                        .get(delim_li)
                        .is_some_and(|ddl| cursor_range_for_line(&self.doc, ddl, true).is_some());
                let visual_last_li = if table_end_line == delim_li && !delim_expanded {
                    table_start_line
                } else {
                    table_end_line
                };
                let y_last = self.height_table.y_for_line(visual_last_li);
                let frame_h_last = if let Some(cached) = self
                    .layout_cache
                    .get(&visual_last_li)
                    .filter(|c| c.doc_version == self.doc.version)
                {
                    let bot_margin = if let Some(tr) = &cached.table_row {
                        tr.frame_bottom_margin
                    } else if visual_last_li == delim_li && cached.total_height > 0.5 {
                        if cached.table_active {
                            TABLE_TOOLBAR_RESERVE
                        } else {
                            8.0
                        }
                    } else {
                        0.0
                    };
                    (cached.total_height - bot_margin).max(0.0)
                } else {
                    let raw_last = self.doc.line_str(visual_last_li);
                    let est = crate::live_md::estimate_line_height(
                        raw_last,
                        &self.doc.lines[visual_last_li].ctx,
                        col_geo.col_width,
                    );
                    (est - if visual_last_li == delim_li { 0.0 } else { 8.0 }).max(0.0)
                };
                let bar_top = col_geo.top_pad + y_last + frame_h_last - scroll_y + 4.0;
                if bar_top > -28.0 && bar_top < vp_h + 4.0 {
                    let raw_caret = self.doc.line_str(caret_li);
                    let local_head = self
                        .doc
                        .head
                        .saturating_sub(caret_dl.byte_start)
                        .min(raw_caret.len());
                    let (_, col_ix) = table_cell_at_local_byte(raw_caret, local_head);
                    let num_cols = self
                        .doc
                        .table_column_count(table_start_line, table_end_line);
                    let aligns = parse_table_alignments(self.doc.line_str(delim_li));
                    let align_label = match aligns.get(col_ix).copied().unwrap_or(TableAlign::Left) {
                        TableAlign::Left => "Align: L",
                        TableAlign::Center => "Align: C",
                        TableAlign::Right => "Align: R",
                    };
                    let ed = cx.entity();
                    let mut buttons: Vec<AnyElement> = vec![
                        table_tool_pill("tbl-add-row", "+ Row", true, false, ed.clone(), move |doc| {
                            doc.table_insert_row_below(caret_li)
                        }),
                        table_tool_pill("tbl-add-col", "+ Col", false, false, ed.clone(), move |doc| {
                            doc.table_insert_col_after(caret_li, col_ix)
                        }),
                        table_tool_pill("tbl-align", align_label, false, false, ed.clone(), move |doc| {
                            doc.table_cycle_col_align(caret_li, col_ix)
                        }),
                    ];
                    if !is_header && !is_delim {
                        buttons.push(table_tool_pill(
                            "tbl-del-row",
                            "− Row",
                            false,
                            true,
                            ed.clone(),
                            move |doc| doc.table_delete_row(caret_li),
                        ));
                    }
                    if num_cols > 1 {
                        buttons.push(table_tool_pill(
                            "tbl-del-col",
                            "− Col",
                            false,
                            true,
                            ed,
                            move |doc| doc.table_delete_col(caret_li, col_ix),
                        ));
                    }
                    table_toolbar = Some(
                        div()
                            .absolute()
                            .top(px(bar_top))
                            .left(px(col_geo.col_left_rel))
                            .w(px(col_geo.col_width))
                            .flex()
                            .justify_end()
                            .child(
                                div()
                                    .h(px(22.))
                                    .px_1()
                                    .gap_0p5()
                                    .flex()
                                    .items_center()
                                    .rounded(px(6.))
                                    .bg(hex_color(kd::BG_SIDEBAR))
                                    .border_1()
                                    .border_color(hex_color(kd::BORDER))
                                    .shadow_sm()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .children(buttons),
                            )
                            .into_any_element(),
                    );
                }
            }
        }

        div()
            .id(SharedString::from(format!("live-md-editor-{:?}", cx.entity_id())))
            .relative()
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .cursor_text()
            .when_some(
                self.active_image_resize.as_ref().map(|r| r.corner),
                |d, corner| match corner {
                    ResizeHandleCorner::LeftEdge | ResizeHandleCorner::RightEdge => {
                        d.cursor_ew_resize()
                    }
                    ResizeHandleCorner::TopLeft | ResizeHandleCorner::BottomRight => {
                        d.cursor_nwse_resize()
                    }
                    ResizeHandleCorner::TopRight | ResizeHandleCorner::BottomLeft => {
                        d.cursor_nesw_resize()
                    }
                },
            )
            .track_focus(&self.focus_handle)
            .key_context(LIVE_EDITOR_CONTEXT)
            .on_action(cx.listener(|this, _: &LiveEditorTab, _window, cx| {
                cx.stop_propagation();
                this.handle_tab_key(false, cx);
            }))
            .on_action(cx.listener(|this, _: &LiveEditorShiftTab, _window, cx| {
                cx.stop_propagation();
                this.handle_tab_key(true, cx);
            }))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                this.handle_key_down(ev, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.focus_handle.focus(window, cx);
                    this.doc.break_undo_coalescing();
                    this.selected_image_line = None;
                    this.active_image_resize = None;
                    let Some(hit) = this.hit_test_byte_at_point(ev.position) else {
                        cx.notify();
                        return;
                    };

                    if let Some((row_li, col_ix)) = hit.phantom_table_cell {
                        this.doc.table_ensure_cell(row_li, col_ix);
                        this.sync_height_table_after_edit();
                        this.preferred_x = None;
                        this.reset_caret_blink();
                        cx.emit(LiveEditorEvent::Changed);
                        cx.notify();
                        return;
                    }

                    if hit.clicked_checkbox {
                        if this.doc.toggle_task_on_line(hit.line_ix) {
                            this.sync_height_table_after_edit();
                            this.reset_caret_blink();
                            cx.emit(LiveEditorEvent::Changed);
                            cx.notify();
                        }
                        return;
                    }

                    // Follow wikilink / URL unless Alt is held to edit its text directly.
                    if ev.click_count == 1 && !ev.modifiers.alt
                        && let Some(target) = hit.clicked_link
                    {
                        cx.emit(LiveEditorEvent::OpenLink {
                            target,
                            in_right_split: ev.modifiers.control,
                        });
                        return;
                    }

                    this.preferred_x = None;
                    match ev.click_count {
                        2 => {
                            this.doc.select_word_at(hit.raw_byte);
                        }
                        n if n >= 3 => {
                            this.doc.select_line_at(hit.raw_byte);
                        }
                        _ => {
                            this.doc.set_cursor(hit.raw_byte, ev.modifiers.shift);
                            this.dragging = true;
                        }
                    }
                    this.reset_caret_blink();
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _window, cx| {
                if let Some(mut resize) = this.active_image_resize.take() {
                    if !ev.dragging() {
                        this.active_image_resize = Some(resize);
                        if this.commit_active_image_resize(cx) {
                            cx.notify();
                        }
                        return;
                    }
                    let dx = f32::from(ev.position.x - resize.start_mouse.x);
                    let dy = f32::from(ev.position.y - resize.start_mouse.y);
                    let center_mult = if resize.align == ImageAlign::Center {
                        2.0
                    } else {
                        1.0
                    };
                    let delta_w = match resize.corner {
                        ResizeHandleCorner::RightEdge => dx * center_mult,
                        ResizeHandleCorner::LeftEdge => -dx * center_mult,
                        ResizeHandleCorner::BottomRight => {
                            if dx.abs() >= dy.abs() * 0.85 {
                                dx * center_mult
                            } else {
                                dy * resize.aspect_ratio * center_mult
                            }
                        }
                        ResizeHandleCorner::TopRight => {
                            if dx.abs() >= dy.abs() * 0.85 {
                                dx * center_mult
                            } else {
                                -dy * resize.aspect_ratio * center_mult
                            }
                        }
                        ResizeHandleCorner::BottomLeft => {
                            if dx.abs() >= dy.abs() * 0.85 {
                                -dx * center_mult
                            } else {
                                dy * resize.aspect_ratio * center_mult
                            }
                        }
                        ResizeHandleCorner::TopLeft => {
                            if dx.abs() >= dy.abs() * 0.85 {
                                -dx * center_mult
                            } else {
                                -dy * resize.aspect_ratio * center_mult
                            }
                        }
                    };
                    let min_w = (48.0_f32)
                        .max(36.0 * resize.aspect_ratio)
                        .min(resize.max_width);
                    let eff_max = resize
                        .max_width
                        .min(900.0 * resize.aspect_ratio)
                        .max(min_w);
                    let new_w = (resize.start_width + delta_w).clamp(min_w, eff_max);
                    let new_h = (new_w / resize.aspect_ratio).clamp(36.0, 900.0);
                    resize.preview_width = new_w;
                    resize.preview_height = new_h;
                    this.active_image_resize = Some(resize);
                    cx.notify();
                    return;
                }

                if !this.dragging || !ev.dragging() {
                    if this.dragging {
                        this.dragging = false;
                    }
                    return;
                }
                if let Some(hit) = this.hit_test_byte_at_point(ev.position) {
                    if hit.raw_byte != this.doc.head {
                        this.doc.set_cursor(hit.raw_byte, true);
                        this.ensure_caret_visible();
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _window, cx| {
                    this.dragging = false;
                    if this.commit_active_image_resize(cx) {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _window, cx| {
                    this.dragging = false;
                    if this.commit_active_image_resize(cx) {
                        cx.notify();
                    }
                }),
            )
            .on_scroll(cx.listener(|this, ev: &ScrollEvent, _window, cx| {
                let dy = f32::from(ev.delta.pixel_delta(px(22.)).y);
                if dy.abs() > 0.01 {
                    let mut st = this.scroll.borrow_mut();
                    let old_y = st.scroll_y;
                    st.scroll_y -= dy;
                    LiveEditorScrollbarHandle::clamp_scroll(&mut st);
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
            .children(image_overlays)
            .children(table_toolbar)
            .child(Scrollbar::vertical(&self.scrollbar))
    }
}

struct LiveCanvasElement {
    editor: Entity<LiveMarkdownEditor>,
}

struct PaintSnapshot {
    bounds: Bounds<Pixels>,
    focus_handle: FocusHandle,
    col_left_rel: f32,
    col_width: f32,
    top_pad: f32,
    scroll_y: f32,
    paint_rows: Vec<(DocLine, Rc<CachedLine>)>,
    caret_cached: Option<Rc<CachedLine>>,
    caret_disp_x_y: Option<(f32, f32, f32)>,
    selection_range: Range<usize>,
    caret_visible: bool,
    focused: bool,
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

#[allow(clippy::too_many_arguments)]
fn paint_wrapped_selection_rects(
    shaped: Option<&WrappedLine>,
    projected: &ProjectedLine,
    raw_offset: usize,
    raw_len: usize,
    ls: usize,
    le: usize,
    inc_trailing_pad: bool,
    text_x: Pixels,
    text_y: Pixels,
    line_height_f: f32,
    align: TextAlign,
    align_width: f32,
    sel_bg: gpui_kit::Hsla,
    window: &mut Window,
) {
    let cs = ls.saturating_sub(raw_offset).min(raw_len);
    let ce = le.saturating_sub(raw_offset).min(raw_len);
    let ds = projected.raw_to_display(cs);
    let de = projected.raw_to_display(ce);
    let disp_len = projected.display_text.len();
    let lh = px(line_height_f);
    let p0 = disp_position_for_index(shaped, disp_len, ds, lh);
    let p1 = disp_position_for_index(shaped, disp_len, de, lh);
    let r0 = (f32::from(p0.y) / line_height_f).round().max(0.0) as usize;
    let r1 = (f32::from(p1.y) / line_height_f).round().max(0.0) as usize;
    let nl_pad = if inc_trailing_pad { 8.0 } else { 0.0 };

    if r0 == r1 {
        let align_dx = row_align_offset_x(shaped, r0, align, align_width, lh);
        let x0 = align_dx + f32::from(p0.x);
        let x1 = align_dx + f32::from(p1.x) + nl_pad;
        if x1 > x0 {
            window.paint_quad(fill(
                Bounds {
                    origin: point(text_x + px(x0), text_y + p0.y),
                    size: size(px(x1 - x0), lh),
                },
                sel_bg,
            ));
        }
    } else {
        for r in r0..=r1 {
            let align_dx = row_align_offset_x(shaped, r, align, align_width, lh);
            let rx0 = align_dx + if r == r0 { f32::from(p0.x) } else { 0.0 };
            let rx1 = align_dx
                + if r == r1 {
                    f32::from(p1.x) + nl_pad
                } else {
                    row_right_x(shaped, r, lh).max(8.0)
                };
            if rx1 > rx0 {
                window.paint_quad(fill(
                    Bounds {
                        origin: point(text_x + px(rx0), text_y + px(r as f32 * line_height_f)),
                        size: size(px(rx1 - rx0), lh),
                    },
                    sel_bg,
                ));
            }
        }
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
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let _ = hitbox;
        let vp_w = f32::from(bounds.size.width);
        let vp_h = f32::from(bounds.size.height);
        if vp_w <= 4.0 || vp_h <= 4.0 {
            return None;
        }

        let ed_entity = self.editor.clone();
        let (snap, schedule_overlay_sync) = ed_entity.update(cx, |ed, cx| {
            let focused = ed.focus_handle.is_focused(window);
            if ed.doc.lines.is_empty() {
                return (None, false);
            }
            let had_empty_bounds = ed.scroll.borrow().viewport_bounds.size.width <= px(20.);
            let (geo, scroll_y, visible_indices, any_img) =
                ed.prepare_layout_for_bounds(bounds, 240.0, window, cx);
            let first_li = visible_indices.first().copied().unwrap_or(0);
            let last_li = visible_indices.last().copied().unwrap_or(0);

            let mut paint_rows = Vec::with_capacity(visible_indices.len());
            for &ix in &visible_indices {
                let y_line = ed.height_table.y_for_line(ix);
                let screen_top = f32::from(bounds.origin.y) + geo.top_pad + y_line - scroll_y;
                if let Some(rc_c) = ed.layout_cache.get_mut(&ix) {
                    if let Some(c) = Rc::get_mut(rc_c) {
                        c.y_top = y_line;
                        c.checkbox_rect = if let PrefixVisual::Checkbox { indent_cols, .. } =
                            c.projected.prefix_visual
                        {
                            let cb_x = f32::from(bounds.origin.x)
                                + geo.col_left_rel
                                + c.extra_left_inset
                                + list_indent_px_from_cols(indent_cols)
                                + 2.0;
                            let cb_y = screen_top + c.text_y_rel + (c.line_height - 14.0) * 0.5;
                            Some(Bounds {
                                origin: point(px(cb_x), px(cb_y)),
                                size: size(px(14.), px(14.)),
                            })
                        } else {
                            None
                        };
                    }
                    paint_rows.push((ed.doc.lines[ix].clone(), Rc::clone(rc_c)));
                }
            }

            // Prune distant layout cache entries when huge docs scroll around.
            if ed.layout_cache.len() > 240 {
                let keep_min = first_li.saturating_sub(64);
                let keep_max = last_li + 60;
                ed.layout_cache
                    .retain(|&ix, _| ix >= keep_min && ix <= keep_max);
            }

            let caret_li = ed.doc.line_index_for_byte(ed.doc.head);
            let caret_cached = if visible_indices.contains(&caret_li) {
                ed.layout_cache.get(&caret_li).cloned()
            } else {
                None
            };
            let caret_disp_x_y = caret_cached.as_ref().map(|c| {
                let dl = &ed.doc.lines[caret_li];
                let local_head = ed
                    .doc
                    .head
                    .saturating_sub(dl.byte_start)
                    .min(dl.byte_end - dl.byte_start);
                raw_offset_to_visual_point(c, local_head)
            });

            let (prose_font, mono_font) = ed.resolved_fonts(window, cx);
            let mut prose_bold_font = prose_font;
            prose_bold_font.weight = FontWeight::SEMIBOLD;

            let snap = PaintSnapshot {
                bounds,
                focus_handle: ed.focus_handle.clone(),
                col_left_rel: geo.col_left_rel,
                col_width: geo.col_width,
                top_pad: geo.top_pad,
                scroll_y,
                paint_rows,
                caret_cached,
                caret_disp_x_y,
                selection_range: ed.doc.selection_range(),
                caret_visible: ed.caret_visible,
                focused,
                marked_range: ed.doc.marked_range.clone(),
                mono_font,
                prose_bold_font,
            };
            (Some(snap), had_empty_bounds && any_img)
        });

        if schedule_overlay_sync {
            let ed_next = self.editor.clone();
            window.on_next_frame(move |_, cx| {
                let _ = ed_next.update(cx, |_, cx| cx.notify());
            });
        }

        snap
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

        window.handle_input(
            &snap.focus_handle,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );

        let sel_bg = cx.theme().selection;
        let col_left = snap.bounds.origin.x + px(snap.col_left_rel);
        let col_w = px(snap.col_width);

        window.with_content_mask(Some(ContentMask { bounds: snap.bounds }), |window| {
            // Pass 1: Block cards, accent bars, rules, checkboxes, bullets, and selection rects.
            for (dl, c) in &snap.paint_rows {
                if c.total_height <= 0.5 {
                    continue;
                }
                let top_y = snap.bounds.origin.y + px(snap.top_pad + c.y_top - snap.scroll_y);
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
                        gpui_kit::Corners {
                            top_left: r_top,
                            top_right: r_top,
                            bottom_right: r_bot,
                            bottom_left: r_bot,
                        },
                        bg_col,
                        gpui_kit::Edges {
                            top: if is_first { px(1.) } else { px(0.) },
                            right: px(1.),
                            bottom: if tr.is_visual_last { px(1.) } else { px(0.) },
                            left: px(1.),
                        },
                        hex_color(kd::BORDER),
                        BorderStyle::Solid,
                    ));

                    if !tr.is_visual_last {
                        let sep_h = if is_header { px(1.5) } else { px(1.) };
                        let sep_col = if is_header {
                            hex_color(kd::BORDER)
                        } else {
                            hex_color(kd::BORDER_SUBTLE)
                        };
                        window.paint_quad(fill(
                            Bounds {
                                origin: point(col_left + px(1.), row_top + frame_h - sep_h),
                                size: size(row_w - px(2.), sep_h),
                            },
                            sep_col,
                        ));
                    }

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
                            let inset_l = if cix == 0 { 1.0 } else { 0.5 };
                            let inset_r = if cix + 1 == tr.cells.len() { 1.0 } else { 0.0 };
                            let inset_t = if is_first { 1.0 } else { 0.0 };
                            let inset_b = if tr.is_visual_last {
                                1.0
                            } else if is_header {
                                1.5
                            } else {
                                1.0
                            };
                            window.paint_quad(quad(
                                Bounds {
                                    origin: point(
                                        col_left + px(cell.x_rel + inset_l),
                                        row_top + px(inset_t),
                                    ),
                                    size: size(
                                        px((cell.width - inset_l - inset_r).max(2.0)),
                                        (frame_h - px(inset_t + inset_b)).max(px(2.)),
                                    ),
                                },
                                px(2.),
                                accent_alpha(0x14),
                                gpui_kit::Edges::all(px(1.5)),
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

                                if let Some(mr) = &snap.marked_range
                                    && mr.start < mr.end
                                    && mr.start <= dl.byte_end
                                    && mr.end >= dl.byte_start
                                    && cell.inner_range.is_some()
                                {
                                    let ms = mr.start.saturating_sub(dl.byte_start).min(raw_len);
                                    let me = mr.end.saturating_sub(dl.byte_start).min(raw_len);
                                    let cell_raw_end = cell.raw_offset + cell.projected.raw_len;
                                    if ms <= cell_raw_end && me >= cell.raw_offset {
                                        let ds = cell.projected.raw_to_display(
                                            ms.saturating_sub(cell.raw_offset)
                                                .min(cell.projected.raw_len),
                                        );
                                        let de = cell.projected.raw_to_display(
                                            me.saturating_sub(cell.raw_offset)
                                                .min(cell.projected.raw_len),
                                        );
                                        let disp_len = cell.projected.display_text.len();
                                        let lh = px(c.line_height);
                                        let p0 = disp_position_for_index(
                                            cell.shaped.as_ref(),
                                            disp_len,
                                            ds,
                                            lh,
                                        );
                                        let p1 = disp_position_for_index(
                                            cell.shaped.as_ref(),
                                            disp_len,
                                            de,
                                            lh,
                                        );
                                        if (f32::from(p1.x) - f32::from(p0.x)) > 1.0
                                            && p0.y == p1.y
                                        {
                                            let row = (f32::from(p0.y) / c.line_height)
                                                .round()
                                                .max(0.0)
                                                as usize;
                                            let align_dx = row_align_offset_x(
                                                cell.shaped.as_ref(),
                                                row,
                                                cell_align,
                                                cell_inner_w,
                                                lh,
                                            );
                                            let uy = cell_text_y + p0.y + lh - px(2.);
                                            window.paint_quad(fill(
                                                Bounds {
                                                    origin: point(
                                                        cell_text_x + px(align_dx) + p0.x,
                                                        uy,
                                                    ),
                                                    size: size(p1.x - p0.x, px(1.5)),
                                                },
                                                hex_color(kd::YELLOW),
                                            ));
                                        }
                                    }
                                }
                            },
                        );
                    }
                    continue;
                }

                let clear_y = px(c.clear_top_spacer);
                let block_left = col_left + px(c.extra_left_inset);
                let block_w =
                    px((snap.col_width - c.extra_left_inset - c.extra_right_inset).max(120.0));
                let content_top = top_y + clear_y;
                let content_h = (row_h - clear_y).max(px(1.));

                // Expanded table delimiter band or non-table block backgrounds & accents
                match &c.ctx {
                    LineBlockContext::Table {
                        is_delim: true,
                        is_last,
                        ..
                    } => {
                        let bot_margin = if *is_last {
                            if c.table_active {
                                TABLE_TOOLBAR_RESERVE
                            } else {
                                8.0
                            }
                        } else {
                            0.0
                        };
                        let frame_h = (row_h - px(bot_margin)).max(px(1.));
                        let r_bot = if *is_last { px(6.) } else { px(0.) };
                        let card_h = if *is_last {
                            frame_h
                        } else {
                            frame_h + px(0.5)
                        };
                        window.paint_quad(quad(
                            Bounds {
                                origin: point(col_left, top_y),
                                size: size(col_w, card_h),
                            },
                            gpui_kit::Corners {
                                top_left: px(0.),
                                top_right: px(0.),
                                bottom_right: r_bot,
                                bottom_left: r_bot,
                            },
                            hex_color(kd::BG_DEEP),
                            gpui_kit::Edges {
                                top: px(0.),
                                right: px(1.),
                                bottom: if *is_last { px(1.) } else { px(0.) },
                                left: px(1.),
                            },
                            hex_color(kd::BORDER),
                            BorderStyle::Solid,
                        ));
                        if !*is_last {
                            window.paint_quad(fill(
                                Bounds {
                                    origin: point(
                                        col_left + px(1.),
                                        top_y + frame_h - px(1.),
                                    ),
                                    size: size(col_w - px(2.), px(1.)),
                                },
                                hex_color(kd::BORDER_SUBTLE),
                            ));
                        }
                    }
                    LineBlockContext::CodeFence {
                        is_first, is_last, ..
                    } => {
                        let card_bounds = Bounds {
                            origin: point(col_left, content_top),
                            size: size(col_w, content_h + px(0.5)),
                        };
                        let r_top = if *is_first { px(6.) } else { px(0.) };
                        let r_bot = if *is_last { px(6.) } else { px(0.) };
                        window.paint_quad(quad(
                            card_bounds,
                            gpui_kit::Corners {
                                top_left: r_top,
                                top_right: r_top,
                                bottom_right: r_bot,
                                bottom_left: r_bot,
                            },
                            hex_color(kd::BG_DEEP),
                            gpui_kit::Edges::default(),
                            gpui_kit::transparent_black(),
                            BorderStyle::Solid,
                        ));
                    }
                    LineBlockContext::Frontmatter { .. } => {
                        let card_bounds = Bounds {
                            origin: point(col_left, content_top),
                            size: size(col_w, content_h + px(0.5)),
                        };
                        window.paint_quad(fill(card_bounds, hex_color(kd::BG_SIDEBAR)));
                    }
                    LineBlockContext::Callout {
                        is_first, is_last, ..
                    } => {
                        let card_bounds = Bounds {
                            origin: point(block_left, content_top),
                            size: size(block_w, content_h + px(0.5)),
                        };
                        let r_top = if *is_first { px(6.) } else { px(0.) };
                        let r_bot = if *is_last { px(6.) } else { px(0.) };
                        window.paint_quad(quad(
                            card_bounds,
                            gpui_kit::Corners {
                                top_left: r_top,
                                top_right: r_top,
                                bottom_right: r_bot,
                                bottom_left: r_bot,
                            },
                            accent_alpha(0x18),
                            gpui_kit::Edges::default(),
                            gpui_kit::transparent_black(),
                            BorderStyle::Solid,
                        ));
                        let bar_bounds = Bounds {
                            origin: point(block_left, content_top),
                            size: size(px(3.), content_h + px(0.5)),
                        };
                        window.paint_quad(fill(bar_bounds, hex_color(kd::ACCENT)));
                    }
                    LineBlockContext::Blockquote { depth, .. } => {
                        for d in 0..(*depth).min(4) {
                            let bx = block_left + px(d as f32 * 14.0 + 2.0);
                            let bar = Bounds {
                                origin: point(bx, content_top + px(1.)),
                                size: size(px(3.), content_h),
                            };
                            window.paint_quad(fill(bar, hex_color(kd::ACCENT)));
                        }
                    }
                    _ => {}
                }

                // Prefix visuals (bullet, checkbox, divider, code fence lang chip, callout badge)
                match &c.projected.prefix_visual {
                    PrefixVisual::SceneBreakRule => {
                        let rule_y = content_top + content_h * 0.5;
                        window.paint_quad(fill(
                            Bounds {
                                origin: point(block_left + px(8.), rule_y),
                                size: size(block_w - px(16.), px(1.)),
                            },
                            hex_color(kd::BORDER_SUBTLE),
                        ));
                    }
                    PrefixVisual::Bullet { indent_cols } => {
                        let dot_x = block_left
                            + px(list_indent_px_from_cols(*indent_cols) + 7.0);
                        let dot_y = top_y + px(c.text_y_rel + c.line_height * 0.5 - 2.5);
                        window.paint_quad(quad(
                            Bounds {
                                origin: point(dot_x, dot_y),
                                size: size(px(5.), px(5.)),
                            },
                            px(3.),
                            hex_color(kd::ORANGE),
                            gpui_kit::Edges::default(),
                            gpui_kit::transparent_black(),
                            BorderStyle::Solid,
                        ));
                    }
                    PrefixVisual::Checkbox { checked, .. } => {
                        if let Some(cb_rect) = c.checkbox_rect {
                            let bg = if *checked {
                                hex_color(kd::GREEN)
                            } else {
                                hex_color(kd::BG_DEEP)
                            };
                            let border_c = if *checked {
                                hex_color(kd::GREEN)
                            } else {
                                hex_color(kd::FG_MUTED)
                            };
                            window.paint_quad(quad(
                                cb_rect,
                                px(3.),
                                bg,
                                gpui_kit::Edges::all(px(1.2)),
                                border_c,
                                BorderStyle::Solid,
                            ));
                            if *checked {
                                window.paint_quad(fill(
                                    Bounds {
                                        origin: point(
                                            cb_rect.origin.x + px(3.5),
                                            cb_rect.origin.y + px(3.5),
                                        ),
                                        size: size(px(7.), px(7.)),
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
                        let chip_origin = point(col_left + px(12.), content_top + px(3.));
                        let chip_bg = Bounds {
                            origin: chip_origin,
                            size: size(px(chip_w), px(15.)),
                        };
                        window.paint_quad(quad(
                            chip_bg,
                            px(4.),
                            hex_color(kd::BG_PANEL),
                            gpui_kit::Edges::default(),
                            gpui_kit::transparent_black(),
                            BorderStyle::Solid,
                        ));
                        let label: SharedString = lang_str.clone().into();
                        let run = TextRun {
                            len: label.len(),
                            font: snap.mono_font.clone(),
                            color: hex_color(kd::FG_MUTED),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shaped = window
                            .text_system()
                            .shape_line(label, px(10.5), &[run], None);
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
                        let pill_w = callout_badge_width(kind);
                        let pill_origin = point(
                            block_left + px(16.),
                            top_y + px(c.text_y_rel + (c.line_height - 18.0) * 0.5),
                        );
                        window.paint_quad(quad(
                            Bounds {
                                origin: pill_origin,
                                size: size(px(pill_w), px(18.)),
                            },
                            px(4.),
                            accent_alpha(0x2e),
                            gpui_kit::Edges::default(),
                            gpui_kit::transparent_black(),
                            BorderStyle::Solid,
                        ));
                        let upper: SharedString = kind.to_ascii_uppercase().into();
                        let run = TextRun {
                            len: upper.len(),
                            font: snap.prose_bold_font.clone(),
                            color: hex_color(kd::ACCENT),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shaped = window
                            .text_system()
                            .shape_line(upper, px(11.), &[run], None);
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
                        (snap.col_width - c.extra_left_inset - c.extra_right_inset).max(120.0),
                        sel_bg,
                        window,
                    );
                }

                // Inline code & highlight run backgrounds + shaped text
                if let Some(wl) = c.shaped.as_ref() {
                    let origin = point(col_left + px(c.text_x_rel), top_y + px(c.text_y_rel));
                    let lh = px(c.line_height);
                    let _ = wl.paint_background(origin, lh, TextAlign::Left, None, window, cx);
                    let _ = wl.paint(origin, lh, TextAlign::Left, None, window, cx);
                }

                // IME marked-text underline
                if let Some(mr) = &snap.marked_range
                    && mr.start < mr.end
                    && mr.start <= dl.byte_end
                    && mr.end >= dl.byte_start
                {
                    let raw_len = dl.byte_end - dl.byte_start;
                    let ds = c.projected.raw_to_display(mr.start.saturating_sub(dl.byte_start).min(raw_len));
                    let de = c.projected.raw_to_display(mr.end.saturating_sub(dl.byte_start).min(raw_len));
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

            // Pass 2: Blinking Caret
            if snap.focused && snap.caret_visible
                && let Some((vx, vy, lh)) = snap.caret_disp_x_y
                && let Some(c) = snap.caret_cached.as_ref()
            {
                let caret_x = col_left + px(vx);
                let caret_y =
                    snap.bounds.origin.y + px(snap.top_pad + c.y_top - snap.scroll_y + vy + 1.0);
                window.paint_quad(fill(
                    Bounds {
                        origin: point(caret_x, caret_y),
                        size: size(px(2.), px((lh - 2.0).max(12.0))),
                    },
                    hex_color(kd::ACCENT),
                ));
            }
        });
    }
}
