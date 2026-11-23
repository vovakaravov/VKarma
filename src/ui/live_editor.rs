use crate::live_md::{
    DocLine, EditKind, InlineStyle, LineBlockContext, LineHeightTable, LinkTarget, LiveDocBuffer,
    PrefixVisual, ProjectedLine, clamp_char_boundary, list_indent_px_from_cols, project_line,
    utf8_to_utf16_offset, utf16_to_utf8_offset,
};
use crate::markdown::{ImageAlign, parse_image_spec};
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
    TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, div, fill, img, point,
    px, quad, relative, size,
};
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

struct CachedLine {
    doc_version: u64,
    cursor_key: Option<(usize, usize)>,
    wrap_width: f32,
    raw_len: usize,
    ctx: LineBlockContext,
    projected: ProjectedLine,
    shaped: Option<WrappedLine>,
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

fn image_embed_dimensions(alt: &str, col_width: f32) -> (f32, f32, ImageAlign) {
    let spec = parse_image_spec(alt);
    let max_w = (col_width - 16.0).max(120.0);
    let w = spec.width.unwrap_or(520.0).min(max_w).max(64.0);
    let h = spec.height.unwrap_or((w * 0.56).min(340.0)).clamp(64.0, 620.0);
    (w, h, spec.align)
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
            let mut font = if st.monospace || matches!(ctx, LineBlockContext::CodeFence { .. } | LineBlockContext::Frontmatter { .. } | LineBlockContext::Table { .. }) {
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
                Some(hex_alpha(0x241d33e0))
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

fn shape_cached_line(
    dl: &DocLine,
    raw_line: &str,
    cursor_local: Option<Range<usize>>,
    col_width: f32,
    doc_version: u64,
    prose_font: &Font,
    mono_font: &Font,
    window: &Window,
) -> CachedLine {
    let cursor_key = cursor_local.as_ref().map(|r| (r.start, r.end));
    let projected = project_line(raw_line, &dl.ctx, cursor_local);
    let (font_size_f, lh_f, base_pt, base_pb) = line_typography(&dl.ctx);
    let indent_x = line_text_indent_x(&dl.ctx, &projected.prefix_visual);
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
    let avail_text_w = (col_width - indent_x - right_pad).max(120.0);

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

    let total_height = match &dl.ctx {
        LineBlockContext::ImageEmbed { alt, .. } => {
            let text_h = if projected.display_text.is_empty() {
                0.0
            } else {
                wrap_rows * lh_f + 4.0
            };
            let (_, img_h, _) = image_embed_dimensions(alt, col_width);
            text_h + img_h + 14.0
        }
        LineBlockContext::Blank | LineBlockContext::SceneBreak => lh_f,
        _ => base_pt + wrap_rows * lh_f + base_pb,
    };

    CachedLine {
        doc_version,
        cursor_key,
        wrap_width: col_width,
        raw_len: raw_line.len(),
        ctx: dl.ctx.clone(),
        projected,
        shaped,
        line_height: lh_f,
        total_height,
        y_top: 0.0,
        text_x_rel: indent_x,
        text_y_rel: base_pt,
        checkbox_rect: None,
    }
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
        let col_w = self.last_col_geo.map_or(760.0, |g| g.1);
        self.height_table = LineHeightTable::from_lines(&self.doc.lines, &self.doc.text, col_w);
        self.preferred_x = None;
        self.ensure_caret_visible();
        self.reset_caret_blink();
        cx.notify();
    }

    pub fn insert_text_at_caret(&mut self, s: &str, cx: &mut Context<Self>) {
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

    fn sync_height_table_after_edit(&mut self) {
        let col_w = self.last_col_geo.map_or(760.0, |g| g.1);
        self.height_table
            .sync_lines(&self.doc.lines, &self.doc.text, col_w);
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
        let raw_line = &self.doc.text[dl.byte_start..dl.byte_end];

        let hit = self.layout_cache.get(&li).is_some_and(|c| {
            c.doc_version == self.doc.version
                && c.cursor_key == key
                && (c.wrap_width - col_w).abs() < 0.5
                && c.raw_len == raw_line.len()
        });
        if !hit {
            let (prose, mono) = self.resolved_fonts(window, cx);
            let cached = shape_cached_line(
                &dl,
                raw_line,
                cursor_local,
                col_w,
                self.doc.version,
                &prose,
                &mono,
                window,
            );
            self.height_table.set_height(li, cached.total_height);
            self.layout_cache.insert(li, Rc::new(cached));
        }
        self.layout_cache.get(&li).cloned()
    }

    fn hit_test_byte_at_point(&mut self, pos: Point<Pixels>) -> Option<(usize, usize, Option<LinkTarget>, bool)> {
        let st = self.scroll.borrow();
        let bounds = st.viewport_bounds;
        if bounds.size.width <= px(1.) || self.doc.lines.is_empty() {
            return None;
        }
        let doc_y = (f32::from(pos.y - bounds.origin.y) + st.scroll_y).max(0.0);
        let (_, col_width, top_pad) = self.last_col_geo.unwrap_or_else(|| {
            let g = compute_column_geometry(f32::from(bounds.size.width), self.centered, self.zen_mode);
            (g.col_left_rel, g.col_width, g.top_pad)
        });
        let rel_doc_y = (doc_y - top_pad).max(0.0);
        drop(st);

        if rel_doc_y >= self.height_table.total_height() {
            return Some((self.doc.text.len(), self.doc.lines.len() - 1, None, false));
        }

        let li = self
            .height_table
            .line_for_y(rel_doc_y)
            .min(self.doc.lines.len() - 1);
        let dl = &self.doc.lines[li];
        let raw_line = &self.doc.text[dl.byte_start..dl.byte_end];

        let Some(cached) = self.layout_cache.get(&li) else {
            return Some((dl.byte_start, li, None, false));
        };

        // Check checkbox click first.
        if let Some(cb_rect) = cached.checkbox_rect {
            let expanded = Bounds {
                origin: point(cb_rect.origin.x - px(4.), cb_rect.origin.y - px(4.)),
                size: size(cb_rect.size.width + px(8.), cb_rect.size.height + px(8.)),
            };
            if expanded.contains(&pos) {
                return Some((dl.byte_start, li, None, true));
            }
        }

        let col_left = bounds.origin.x
            + px(self.last_col_geo.map_or(32.0, |g| g.0));
        let text_left = col_left + px(cached.text_x_rel);
        let text_top = bounds.origin.y
            + px(top_pad + self.height_table.y_for_line(li) - self.scroll.borrow().scroll_y
                + cached.text_y_rel);

        let Some(wl) = cached.shaped.as_ref() else {
            return Some((dl.byte_start, li, None, false));
        };

        let num_rows = wl.wrap_boundaries.len() + 1;
        let max_local_y = (num_rows as f32 * cached.line_height - 0.5).max(0.0);
        let local_x = f32::from(pos.x - text_left).max(0.0);
        let local_y = f32::from(pos.y - text_top).clamp(0.0, max_local_y);
        let _ = col_width;

        let disp_idx = match wl.closest_index_for_position(point(px(local_x), px(local_y)), px(cached.line_height)) {
            Ok(i) | Err(i) => i,
        }
        .min(cached.projected.display_text.len());

        // Check link click only if user clicked strictly inside a concealed link glyph span.
        let mut clicked_link = None;
        if let Ok(exact_i) = wl.index_for_position(point(px(local_x), px(local_y)), px(cached.line_height)) {
            for link in &cached.projected.links {
                if link.concealed && exact_i >= link.disp_range.start && exact_i < link.disp_range.end {
                    clicked_link = Some(link.target.clone());
                    break;
                }
            }
        }

        let local_raw = cached.projected.display_to_raw(raw_line, disp_idx);
        let abs_raw = clamp_char_boundary(&self.doc.text, dl.byte_start + local_raw);
        Some((abs_raw, li, clicked_link, false))
    }

    fn move_vertical(
        &mut self,
        delta: isize,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let local_head = self.doc.head.saturating_sub(dl.byte_start).min(dl.byte_end - dl.byte_start);
        let (cur_x, vis_row, num_rows, lh) = if let Some(c) = self.ensure_line_cached(li, window, cx) {
            let disp_idx = c.projected.raw_to_display(local_head);
            let pt = disp_position_for_index(
                c.shaped.as_ref(),
                c.projected.display_text.len(),
                disp_idx,
                px(c.line_height),
            );
            let row = (f32::from(pt.y) / c.line_height).round().max(0.0) as usize;
            (f32::from(pt.x), row, c.num_wrap_rows(), c.line_height)
        } else {
            (0.0, 0, 1, 24.75)
        };

        let target_x = *self.preferred_x.get_or_insert(cur_x);

        let (target_li, target_row) = if delta < 0 {
            if vis_row > 0 {
                (li, Some(vis_row - 1))
            } else if li > 0 {
                (li - 1, None)
            } else {
                self.doc.set_cursor(0, extend);
                self.ensure_caret_visible();
                cx.notify();
                return;
            }
        } else if vis_row + 1 < num_rows {
            (li, Some(vis_row + 1))
        } else if li + 1 < self.doc.lines.len() {
            (li + 1, Some(0))
        } else {
            self.doc.set_cursor(self.doc.text.len(), extend);
            self.ensure_caret_visible();
            cx.notify();
            return;
        };

        let _ = lh;
        let target_start = self.doc.lines[target_li].byte_start;
        let target_raw = self.doc.line_str(target_li).to_string();
        let new_abs = if let Some(c) = self.ensure_line_cached(target_li, window, cx) {
            if let Some(wl) = c.shaped.as_ref() {
                let rows = wl.wrap_boundaries.len() + 1;
                let row = target_row.unwrap_or_else(|| rows.saturating_sub(1)).min(rows - 1);
                let y = row as f32 * c.line_height + c.line_height * 0.5;
                let disp_idx = match wl.closest_index_for_position(point(px(target_x), px(y)), px(c.line_height)) {
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
                self.preferred_x = None;
                self.doc.indent_lines(shift);
                self.sync_height_table_after_edit();
                self.ensure_caret_visible();
                self.reset_caret_blink();
                cx.emit(LiveEditorEvent::Changed);
                cx.notify();
            }
            "escape" if self.doc.has_selection() => {
                cx.stop_propagation();
                self.doc.set_cursor(self.doc.head, false);
                self.reset_caret_blink();
                cx.notify();
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
        let disp = cached.projected.raw_to_display(local_raw);
        let rel_pt = disp_position_for_index(
            cached.shaped.as_ref(),
            cached.projected.display_text.len(),
            disp,
            px(cached.line_height),
        );
        let origin_x = element_bounds.origin.x + px(col_left_rel + cached.text_x_rel) + rel_pt.x;
        let origin_y = element_bounds.origin.y
            + px(top_pad + self.height_table.y_for_line(li) - st.scroll_y + cached.text_y_rel)
            + rel_pt.y;
        Some(Bounds {
            origin: point(origin_x, origin_y),
            size: size(px(2.), px(cached.line_height)),
        })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let (raw_byte, _, _, _) = self.hit_test_byte_at_point(point)?;
        Some(utf8_to_utf16_offset(&self.doc.text, raw_byte))
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

impl Render for LiveMarkdownEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scroll_state = self.scroll.borrow();
        let vp_bounds = scroll_state.viewport_bounds;
        let vp_w = f32::from(vp_bounds.size.width);
        let vp_h = f32::from(vp_bounds.size.height);
        let scroll_y = scroll_state.scroll_y;
        drop(scroll_state);

        // Build visible ImageEmbed overlays on the same frame from deterministic heights.
        let mut image_overlays: Vec<AnyElement> = Vec::new();
        if vp_w > 20.0 && vp_h > 20.0 && !self.doc.lines.is_empty() {
            let col_geo = compute_column_geometry(vp_w, self.centered, self.zen_mode);
            let rel_start_y = (scroll_y - col_geo.top_pad - 360.0).max(0.0);
            let rel_end_y = (scroll_y + vp_h - col_geo.top_pad + 360.0).max(0.0);
            let first_li = self.height_table.line_for_y(rel_start_y);
            let last_li = self
                .height_table
                .line_for_y(rel_end_y)
                .min(self.doc.lines.len() - 1);

            for ix in first_li..=last_li {
                let dl = &self.doc.lines[ix];
                if let LineBlockContext::ImageEmbed { target, alt } = &dl.ctx {
                    let y_line = self.height_table.y_for_line(ix);
                    let cursor_present = cursor_range_for_line(&self.doc, dl, true).is_some();
                    let text_h = if cursor_present { 24.0 } else { 0.0 };
                    let img_top = col_geo.top_pad + y_line - scroll_y + text_h + 6.0;
                    let (img_w, img_h, align) = image_embed_dimensions(alt, col_geo.col_width);
                    let img_left = match align {
                        ImageAlign::Left => col_geo.col_left_rel,
                        ImageAlign::Center => {
                            col_geo.col_left_rel + ((col_geo.col_width - img_w) * 0.5).max(0.0)
                        }
                        ImageAlign::Right => {
                            col_geo.col_left_rel + (col_geo.col_width - img_w).max(0.0)
                        }
                    };
                    let line_start_byte = dl.byte_start;
                    let resolved = resolve_image_file(target, &self.doc_path, &self.vault);
                    let target_name = target.clone();
                    let ov = div()
                        .id(SharedString::from(format!("live-img-{ix}")))
                        .absolute()
                        .top(px(img_top))
                        .left(px(img_left))
                        .w(px(img_w))
                        .h(px(img_h))
                        .rounded(px(8.))
                        .overflow_hidden()
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.doc.set_cursor(line_start_byte, false);
                                this.preferred_x = None;
                                this.focus_handle.focus(window, cx);
                                this.reset_caret_blink();
                                cx.notify();
                            }),
                        )
                        .child(if let Some(path) = resolved {
                            img(path)
                                .w(px(img_w))
                                .h(px(img_h))
                                .object_fit(ObjectFit::Contain)
                                .rounded(px(8.))
                                .into_any_element()
                        } else {
                            div()
                                .size_full()
                                .rounded(px(8.))
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
                        })
                        .into_any_element();
                    image_overlays.push(ov);
                }
            }
        }

        div()
            .id(SharedString::from(format!("live-md-editor-{:?}", cx.entity_id())))
            .relative()
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .cursor_text()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                this.handle_key_down(ev, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.focus_handle.focus(window, cx);
                    this.doc.break_undo_coalescing();
                    let Some((byte_off, li, clicked_link, clicked_checkbox)) =
                        this.hit_test_byte_at_point(ev.position)
                    else {
                        return;
                    };

                    if clicked_checkbox {
                        if this.doc.toggle_task_on_line(li) {
                            this.sync_height_table_after_edit();
                            this.reset_caret_blink();
                            cx.emit(LiveEditorEvent::Changed);
                            cx.notify();
                        }
                        return;
                    }

                    // Follow wikilink / URL unless Alt is held to edit its text directly.
                    if ev.click_count == 1 && !ev.modifiers.alt
                        && let Some(target) = clicked_link
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
                            this.doc.select_word_at(byte_off);
                        }
                        n if n >= 3 => {
                            this.doc.select_line_at(byte_off);
                        }
                        _ => {
                            this.doc.set_cursor(byte_off, ev.modifiers.shift);
                            this.dragging = true;
                        }
                    }
                    this.reset_caret_blink();
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _window, cx| {
                if !this.dragging || !ev.dragging() {
                    if this.dragging {
                        this.dragging = false;
                    }
                    return;
                }
                if let Some((byte_off, _, _, _)) = this.hit_test_byte_at_point(ev.position) {
                    if byte_off != this.doc.head {
                        this.doc.set_cursor(byte_off, true);
                        this.ensure_caret_visible();
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _window, _cx| {
                    this.dragging = false;
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _window, _cx| {
                    this.dragging = false;
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
        let snap = ed_entity.update(cx, |ed, cx| {
            let focused = ed.focus_handle.is_focused(window);
            let geo = compute_column_geometry(vp_w, ed.centered, ed.zen_mode);
            ed.last_col_geo = Some((geo.col_left_rel, geo.col_width, geo.top_pad));
            let (prose_font, mono_font) = ed.resolved_fonts(window, cx);

            let scroll_y = {
                let mut st = ed.scroll.borrow_mut();
                st.viewport_bounds = bounds;
                st.scroll_y
            };

            let rel_start_y = (scroll_y - geo.top_pad - 240.0).max(0.0);
            let rel_end_y = (scroll_y + vp_h - geo.top_pad + 240.0).max(0.0);
            let total_lines = ed.doc.lines.len();
            if total_lines == 0 {
                return None;
            }
            let first_li = ed.height_table.line_for_y(rel_start_y);
            let last_li = ed
                .height_table
                .line_for_y(rel_end_y)
                .min(total_lines - 1);

            let mut visible_indices = Vec::with_capacity(last_li.saturating_sub(first_li) + 1);
            for ix in first_li..=last_li {
                let dl = &ed.doc.lines[ix];
                let raw_line = &ed.doc.text[dl.byte_start..dl.byte_end];
                let cursor_local = cursor_range_for_line(&ed.doc, dl, true);
                let cursor_key = cursor_local.as_ref().map(|r| (r.start, r.end));

                let hit = ed.layout_cache.get(&ix).is_some_and(|c| {
                    c.doc_version == ed.doc.version
                        && c.cursor_key == cursor_key
                        && (c.wrap_width - geo.col_width).abs() < 0.5
                        && c.raw_len == raw_line.len()
                });
                if !hit {
                    let cached = shape_cached_line(
                        dl,
                        raw_line,
                        cursor_local,
                        geo.col_width,
                        ed.doc.version,
                        &prose_font,
                        &mono_font,
                        window,
                    );
                    ed.height_table.set_height(ix, cached.total_height);
                    ed.layout_cache.insert(ix, Rc::new(cached));
                }
                visible_indices.push(ix);
            }

            // Second pass: lock y_top & checkbox hit rects after heights settled.
            let content_total_h = ed.height_table.total_height();
            {
                let mut st = ed.scroll.borrow_mut();
                st.total_height = geo.top_pad + content_total_h + (vp_h * 0.45).max(160.0);
                LiveEditorScrollbarHandle::clamp_scroll(&mut st);
            }
            let scroll_y = ed.scroll.borrow().scroll_y;

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
                let keep_min = first_li.saturating_sub(60);
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
                let disp_ix = c.projected.raw_to_display(local_head);
                let pt = disp_position_for_index(
                    c.shaped.as_ref(),
                    c.projected.display_text.len(),
                    disp_ix,
                    px(c.line_height),
                );
                (f32::from(pt.x), f32::from(pt.y), c.line_height)
            });

            let mut prose_bold_font = prose_font.clone();
            prose_bold_font.weight = FontWeight::SEMIBOLD;

            Some(PaintSnapshot {
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
            })
        });

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
                let top_y = snap.bounds.origin.y + px(snap.top_pad + c.y_top - snap.scroll_y);
                let row_h = px(c.total_height);

                // Block background & accents
                match &c.ctx {
                    LineBlockContext::CodeFence {
                        is_first, is_last, ..
                    } => {
                        let card_bounds = Bounds {
                            origin: point(col_left, top_y),
                            size: size(col_w, row_h + px(0.5)),
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
                            origin: point(col_left, top_y),
                            size: size(col_w, row_h + px(0.5)),
                        };
                        window.paint_quad(fill(card_bounds, hex_color(kd::BG_SIDEBAR)));
                    }
                    LineBlockContext::Callout {
                        is_first, is_last, ..
                    } => {
                        let card_bounds = Bounds {
                            origin: point(col_left, top_y),
                            size: size(col_w, row_h + px(0.5)),
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
                            origin: point(col_left, top_y),
                            size: size(px(3.), row_h + px(0.5)),
                        };
                        window.paint_quad(fill(bar_bounds, hex_color(kd::ACCENT)));
                    }
                    LineBlockContext::Blockquote { depth, .. } => {
                        for d in 0..(*depth).min(4) {
                            let bx = col_left + px(d as f32 * 14.0 + 2.0);
                            let bar = Bounds {
                                origin: point(bx, top_y + px(1.)),
                                size: size(px(3.), row_h),
                            };
                            window.paint_quad(fill(bar, hex_color(kd::ACCENT)));
                        }
                    }
                    _ => {}
                }

                // Prefix visuals (bullet, checkbox, divider, code fence lang chip, callout badge)
                match &c.projected.prefix_visual {
                    PrefixVisual::SceneBreakRule => {
                        let rule_y = top_y + row_h * 0.5;
                        window.paint_quad(fill(
                            Bounds {
                                origin: point(col_left + px(8.), rule_y),
                                size: size(col_w - px(16.), px(1.)),
                            },
                            hex_color(kd::BORDER_SUBTLE),
                        ));
                    }
                    PrefixVisual::Bullet { indent_cols } => {
                        let dot_x = col_left
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
                        let chip_origin = point(col_left + px(12.), top_y + px(3.));
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
                            col_left + px(16.),
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
                    let ds = c.projected.raw_to_display(ls);
                    let de = c.projected.raw_to_display(le);
                    let disp_len = c.projected.display_text.len();
                    let lh = px(c.line_height);
                    let text_x = col_left + px(c.text_x_rel);
                    let text_y = top_y + px(c.text_y_rel);
                    let p0 = disp_position_for_index(c.shaped.as_ref(), disp_len, ds, lh);
                    let p1 = disp_position_for_index(c.shaped.as_ref(), disp_len, de, lh);
                    let r0 = (f32::from(p0.y) / c.line_height).round().max(0.0) as usize;
                    let r1 = (f32::from(p1.y) / c.line_height).round().max(0.0) as usize;
                    let nl_pad = if inc_nl { 8.0 } else { 0.0 };

                    if r0 == r1 {
                        let x0 = f32::from(p0.x);
                        let x1 = f32::from(p1.x) + nl_pad;
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
                            let rx0 = if r == r0 { f32::from(p0.x) } else { 0.0 };
                            let rx1 = if r == r1 {
                                f32::from(p1.x) + nl_pad
                            } else {
                                row_right_x(c.shaped.as_ref(), r, lh).max(8.0)
                            };
                            if rx1 > rx0 {
                                window.paint_quad(fill(
                                    Bounds {
                                        origin: point(
                                            text_x + px(rx0),
                                            text_y + px(r as f32 * c.line_height),
                                        ),
                                        size: size(px(rx1 - rx0), lh),
                                    },
                                    sel_bg,
                                ));
                            }
                        }
                    }
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
                && let Some((rel_x, rel_y, lh)) = snap.caret_disp_x_y
                && let Some(c) = snap.caret_cached.as_ref()
            {
                let caret_x = col_left + px(c.text_x_rel + rel_x);
                let caret_y = snap.bounds.origin.y
                    + px(snap.top_pad + c.y_top - snap.scroll_y + c.text_y_rel + rel_y + 1.0);
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
