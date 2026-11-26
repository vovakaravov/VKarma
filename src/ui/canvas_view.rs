use super::find_bar::{
    EditorFindBar, FIND_INPUT_W, find_count_badge, floating_find_card, wrap_editor_with_find,
};
use super::{PaneSide, TabKind, VKarmaApp};
use crate::formats::canvas::{CanvasDoc, CanvasNode, canvas_color_u32, edge_endpoints};
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use crate::vault::{VaultIndex, is_image_extension};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, CursorStyle, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement,
    PathBuilder, Pixels, Point, Render, ScrollEvent, SharedString,
    StatefulInteractiveElement as _, Styled, StyledImage as _, Subscription, TextAlign, TextRun,
    Window, canvas, div, fill, img, point, prelude::FluentBuilder as _, px, size,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

pub enum CanvasViewEvent {
    Changed,
    OpenFile(PathBuf),
}

#[derive(Clone)]
enum CanvasDrag {
    Pan {
        start_mouse: Point<Pixels>,
        orig_pan_x: f32,
        orig_pan_y: f32,
    },
    MoveNode {
        node_id: String,
        start_mouse: Point<Pixels>,
        orig_x: f64,
        orig_y: f64,
        moved: bool,
    },
    ResizeNode {
        node_id: String,
        start_mouse: Point<Pixels>,
        orig_w: f64,
        orig_h: f64,
    },
}

struct NodeEditState {
    node_id: String,
    editor: Entity<EditorState>,
}

pub struct CanvasView {
    pub doc: CanvasDoc,
    vault: Arc<VaultIndex>,
    doc_path: PathBuf,
    focus_handle: FocusHandle,
    pan_x: f32,
    pan_y: f32,
    zoom: f32,
    board_geo: Rc<Cell<(f32, f32, f32, f32)>>,
    selected_node: Option<String>,
    selected_edge: Option<String>,
    connect_mode: bool,
    connect_from: Option<String>,
    source_mode: bool,
    source_editor: Entity<EditorState>,
    source_find_bar: Entity<EditorFindBar>,
    _source_sub: Subscription,
    editing_node: Option<NodeEditState>,
    drag: Option<CanvasDrag>,
    initial_fit_pending: bool,
    find_open: bool,
    find_input: Entity<InputState>,
    _find_sub: Option<Subscription>,
    find_query: String,
    find_matches: Vec<String>,
    find_active: usize,
    find_needs_focus: bool,
}

impl EventEmitter<CanvasViewEvent> for CanvasView {}

impl Focusable for CanvasView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl CanvasView {
    pub fn new(
        doc: CanvasDoc,
        raw_source: String,
        doc_path: PathBuf,
        vault: Arc<VaultIndex>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let source_editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .searchable(false)
                .language("json")
                .soft_wrap(false)
                .line_number(true)
                .default_value(raw_source)
        });
        let source_find_bar = cx.new(|cx| EditorFindBar::new(source_editor.clone(), window, cx));
        let source_sub = cx.subscribe(&source_editor, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) && this.source_mode {
                cx.emit(CanvasViewEvent::Changed);
            }
        });

        Self {
            doc,
            vault,
            doc_path,
            focus_handle: cx.focus_handle(),
            pan_x: 80.0,
            pan_y: 80.0,
            zoom: 1.0,
            board_geo: Rc::new(Cell::new((0.0, 0.0, 900.0, 600.0))),
            selected_node: None,
            selected_edge: None,
            connect_mode: false,
            connect_from: None,
            source_mode: false,
            source_editor,
            source_find_bar,
            _source_sub: source_sub,
            editing_node: None,
            drag: None,
            initial_fit_pending: true,
            find_open: false,
            find_input: cx.new(|cx| InputState::new(window, cx).placeholder("Find nodes…")),
            _find_sub: None,
            find_query: String::new(),
            find_matches: Vec::new(),
            find_active: 0,
            find_needs_focus: false,
        }
    }

    pub fn relocate_doc(&mut self, new_path: PathBuf, vault: Arc<VaultIndex>) {
        self.doc_path = new_path;
        self.vault = vault;
    }

    pub fn update_vault(&mut self, vault: Arc<VaultIndex>) {
        self.vault = vault;
    }

    pub fn to_json(&self, cx: &App) -> String {
        if self.source_mode {
            self.source_editor.read(cx).value().to_string()
        } else {
            self.doc.to_json_pretty()
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{} nodes · {} edges · {}%",
            self.doc.nodes.len(),
            self.doc.edges.len(),
            (self.zoom * 100.0).round() as i32
        )
    }

    pub fn toggle_source_mode(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.source_mode {
            let raw = self.source_editor.read(cx).value().to_string();
            let parsed = CanvasDoc::parse(&raw).map_err(|e| format!("{e}"))?;
            self.doc = parsed;
            self.selected_node = None;
            self.selected_edge = None;
            self.connect_from = None;
            self.source_mode = false;
            self.focus_handle.focus(window, cx);
        } else {
            self.commit_node_edit(cx);
            let json = self.doc.to_json_pretty();
            self.source_editor
                .update(cx, |s, cx| s.set_value(json, window, cx));
            self.source_mode = true;
            self.source_editor.focus_handle(cx).focus(window, cx);
        }
        cx.notify();
        Ok(())
    }

    pub fn add_card_at_center(&mut self, cx: &mut Context<Self>) {
        self.commit_node_edit(cx);
        let (_, _, vp_w, vp_h) = self.board_geo.get();
        let cx_world = ((vp_w * 0.5 - self.pan_x) / self.zoom) as f64 - 140.0;
        let cy_world = ((vp_h * 0.5 - self.pan_y) / self.zoom) as f64 - 70.0;
        let stagger = ((self.doc.nodes.len() % 5) as f64) * 24.0;
        let id = self
            .doc
            .add_text_node(cx_world + stagger, cy_world + stagger, "New Card".to_string());
        self.selected_node = Some(id);
        self.selected_edge = None;
        cx.emit(CanvasViewEvent::Changed);
        cx.notify();
    }

    pub fn add_group_at_center(&mut self, cx: &mut Context<Self>) {
        self.commit_node_edit(cx);
        let (_, _, vp_w, vp_h) = self.board_geo.get();
        let cx_world = ((vp_w * 0.5 - self.pan_x) / self.zoom) as f64 - 220.0;
        let cy_world = ((vp_h * 0.5 - self.pan_y) / self.zoom) as f64 - 150.0;
        let id = self
            .doc
            .add_group_node(cx_world, cy_world, "Group".to_string());
        self.selected_node = Some(id);
        self.selected_edge = None;
        cx.emit(CanvasViewEvent::Changed);
        cx.notify();
    }

    pub fn set_selected_color(&mut self, color: Option<&str>, cx: &mut Context<Self>) {
        let col = color.map(ToString::to_string);
        let mut changed = false;
        if let Some(ref nid) = self.selected_node
            && let Some(node) = self.doc.nodes.iter_mut().find(|n| &n.id == nid)
        {
            node.color = col;
            changed = true;
        } else if let Some(ref eid) = self.selected_edge
            && let Some(edge) = self.doc.edges.iter_mut().find(|e| &e.id == eid)
        {
            edge.color = col;
            changed = true;
        }
        if changed {
            cx.emit(CanvasViewEvent::Changed);
            cx.notify();
        }
    }

    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(nid) = self.selected_node.take() {
            if self
                .editing_node
                .as_ref()
                .is_some_and(|e| e.node_id == nid)
            {
                self.editing_node = None;
            }
            self.doc.remove_node(&nid);
            cx.emit(CanvasViewEvent::Changed);
            cx.notify();
        } else if let Some(eid) = self.selected_edge.take() {
            self.doc.remove_edge(&eid);
            cx.emit(CanvasViewEvent::Changed);
            cx.notify();
        }
    }

    pub fn fit_view(&mut self, cx: &mut Context<Self>) {
        let (_, _, vp_w, vp_h) = self.board_geo.get();
        if let Some((min_x, min_y, max_x, max_y)) = self.doc.bounding_box() {
            let bw = ((max_x - min_x) as f32).max(200.0);
            let bh = ((max_y - min_y) as f32).max(160.0);
            let pad = 64.0;
            let zx = (vp_w - pad).max(120.0) / bw;
            let zy = (vp_h - pad).max(120.0) / bh;
            self.zoom = zx.min(zy).clamp(0.25, 1.35);
            let mid_x = (min_x + max_x) as f32 * 0.5;
            let mid_y = (min_y + max_y) as f32 * 0.5;
            self.pan_x = vp_w * 0.5 - mid_x * self.zoom;
            self.pan_y = vp_h * 0.5 - mid_y * self.zoom;
        } else {
            self.zoom = 1.0;
            self.pan_x = 80.0;
            self.pan_y = 80.0;
        }
        self.initial_fit_pending = false;
        cx.notify();
    }

    pub fn adjust_zoom(&mut self, factor: f32, cx: &mut Context<Self>) {
        let (_, _, vp_w, vp_h) = self.board_geo.get();
        self.zoom_around(vp_w * 0.5, vp_h * 0.5, self.zoom * factor);
        cx.notify();
    }

    pub fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        let (_, _, vp_w, vp_h) = self.board_geo.get();
        self.zoom_around(vp_w * 0.5, vp_h * 0.5, 1.0);
        cx.notify();
    }

    pub fn is_find_open(&self, cx: &App) -> bool {
        if self.source_mode {
            self.source_find_bar.read(cx).is_open()
        } else {
            self.find_open
        }
    }

    fn ensure_find_subscription(&mut self, cx: &mut Context<Self>) {
        if self._find_sub.is_some() {
            return;
        }
        let sub = cx.subscribe(&self.find_input, |this, state, event, cx| {
            match event {
                InputEvent::Change => {
                    this.find_query = state.read(cx).value().to_string();
                    this.recompute_find(true);
                    cx.notify();
                }
                InputEvent::PressEnter {
                    secondary, shift, ..
                } => {
                    this.step_find(!(*shift || *secondary));
                    cx.notify();
                }
                _ => {}
            }
        });
        self._find_sub = Some(sub);
    }

    pub fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.source_mode {
            self.source_find_bar
                .update(cx, |b, cx| b.open(replace, window, cx));
            cx.notify();
            return;
        }
        self.ensure_find_subscription(cx);
        self.find_open = true;
        self.find_needs_focus = true;
        let query = self.find_input.read(cx).value().to_string();
        if query != self.find_query {
            self.find_query = query;
        }
        self.recompute_find(false);
        self.find_input.update(cx, |s, cx| {
            let len = s.value().len();
            s.focus(window, cx);
            if len > 0 {
                s.set_selected_range(0..len, cx);
            }
        });
        cx.notify();
    }

    pub fn close_find(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if self.source_mode {
            self.source_find_bar
                .update(cx, |b, cx| b.close(window, cx));
            cx.notify();
            return;
        }
        if !self.find_open {
            return;
        }
        self.find_open = false;
        self.find_needs_focus = false;
        if let Some(window) = window {
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    fn node_matches_query(node: &CanvasNode, q_lower: &str) -> bool {
        if q_lower.is_empty() {
            return false;
        }
        let contains = |opt: &Option<String>| {
            opt.as_deref()
                .is_some_and(|s| s.to_lowercase().contains(q_lower))
        };
        contains(&node.text)
            || contains(&node.label)
            || contains(&node.file)
            || contains(&node.url)
    }

    fn recompute_find(&mut self, jump: bool) {
        self.find_matches.clear();
        let q_lower = self.find_query.trim().to_lowercase();
        if q_lower.is_empty() {
            self.find_active = 0;
            return;
        }
        for node in &self.doc.nodes {
            if Self::node_matches_query(node, &q_lower) {
                self.find_matches.push(node.id.clone());
            }
        }
        if self.find_matches.is_empty() {
            self.find_active = 0;
            return;
        }
        if jump {
            self.find_active = 0;
            self.focus_active_match_node();
        } else if self.find_active >= self.find_matches.len() {
            self.find_active = 0;
        }
    }

    pub fn step_find(&mut self, forward: bool) {
        if self.find_matches.is_empty() {
            return;
        }
        let len = self.find_matches.len();
        self.find_active = if forward {
            (self.find_active + 1) % len
        } else {
            (self.find_active + len - 1) % len
        };
        self.focus_active_match_node();
    }

    fn focus_active_match_node(&mut self) {
        let Some(nid) = self.find_matches.get(self.find_active).cloned() else {
            return;
        };
        let Some(node) = self.doc.nodes.iter().find(|n| n.id == nid) else {
            return;
        };
        self.selected_node = Some(nid);
        self.selected_edge = None;
        let (_, _, vp_w, vp_h) = self.board_geo.get();
        let vp_w = if vp_w > 40.0 { vp_w } else { 800.0 };
        let vp_h = if vp_h > 40.0 { vp_h } else { 520.0 };
        let mid_x = (node.x + node.width * 0.5) as f32;
        let mid_y = (node.y + node.height * 0.5) as f32;
        self.pan_x = vp_w * 0.5 - mid_x * self.zoom;
        self.pan_y = vp_h * 0.5 - mid_y * self.zoom;
    }

    fn render_find_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.find_needs_focus {
            self.find_needs_focus = false;
            self.find_input.focus_handle(cx).focus(window, cx);
        }
        let total = self.find_matches.len();
        let raw_q = self.find_query.clone();
        let trimmed_non_empty = !raw_q.trim().is_empty();
        let count_text = if !trimmed_non_empty {
            String::new()
        } else if total == 0 {
            "0 / 0".to_string()
        } else {
            format!("{} / {}", self.find_active + 1, total)
        };
        let clear_btn = Button::new(SharedString::from(format!(
            "cnv-find-x-{:?}",
            cx.entity_id()
        )))
        .text()
        .xsmall()
        .tab_stop(false)
        .icon(IconName::Close)
        .tooltip("Clear")
        .on_click(cx.listener(|this, _, window, cx| {
            this.find_input.update(cx, |s, cx| s.set_value("", window, cx));
            this.find_query.clear();
            this.recompute_find(false);
            this.find_input.focus_handle(cx).focus(window, cx);
            cx.notify();
        }));

        floating_find_card(SharedString::from(format!("cnv-find-ov-{:?}", cx.entity_id())))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key.as_str() == "escape" {
                    cx.stop_propagation();
                    this.close_find(Some(window), cx);
                } else if ev.keystroke.key.as_str() == "f3" {
                    cx.stop_propagation();
                    this.step_find(!ev.keystroke.modifiers.shift);
                    cx.notify();
                }
            }))
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div().w(px(FIND_INPUT_W)).child(
                            Input::new(&self.find_input)
                                .xsmall()
                                .prefix(
                                    Icon::new(IconName::Search)
                                        .xsmall()
                                        .text_color(hex_color(kd::FG_MUTED)),
                                )
                                .when(!raw_q.is_empty(), |i| i.suffix(clear_btn)),
                        ),
                    )
                    .when(!count_text.is_empty(), |row| {
                        row.child(find_count_badge(count_text, trimmed_non_empty && total == 0))
                    })
                    .child(
                        Button::new(SharedString::from(format!(
                            "cnv-find-prev-{:?}",
                            cx.entity_id()
                        )))
                        .ghost()
                        .xsmall()
                        .icon(IconName::ChevronUp)
                        .tooltip("Previous Node (Shift+Enter)")
                        .disabled(total == 0)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.step_find(false);
                            cx.notify();
                        })),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "cnv-find-next-{:?}",
                            cx.entity_id()
                        )))
                        .ghost()
                        .xsmall()
                        .icon(IconName::ChevronDown)
                        .tooltip("Next Node (Enter)")
                        .disabled(total == 0)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.step_find(true);
                            cx.notify();
                        })),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "cnv-find-close-{:?}",
                            cx.entity_id()
                        )))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .tooltip("Close (Esc)")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_find(Some(window), cx);
                        })),
                    ),
            )
            .into_any_element()
    }

    fn zoom_around(&mut self, screen_x: f32, screen_y: f32, next_zoom: f32) {
        let clamped = next_zoom.clamp(0.2, 2.5);
        let world_x = (screen_x - self.pan_x) / self.zoom;
        let world_y = (screen_y - self.pan_y) / self.zoom;
        self.zoom = clamped;
        self.pan_x = screen_x - world_x * self.zoom;
        self.pan_y = screen_y - world_y * self.zoom;
    }

    fn start_node_edit(&mut self, node_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.doc.nodes.iter().find(|n| n.id == node_id) else {
            return;
        };
        let initial = match node.kind.as_str() {
            "group" => node.label.clone().unwrap_or_default(),
            "link" => node.url.clone().unwrap_or_default(),
            _ => node.text.clone().unwrap_or_default(),
        };
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .searchable(false)
                .soft_wrap(true)
                .default_value(initial)
        });
        editor.focus_handle(cx).focus(window, cx);
        self.editing_node = Some(NodeEditState { node_id, editor });
        cx.notify();
    }

    fn commit_node_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.editing_node.take() else {
            return;
        };
        let next_val = edit.editor.read(cx).value().to_string();
        if let Some(node) = self.doc.nodes.iter_mut().find(|n| n.id == edit.node_id) {
            match node.kind.as_str() {
                "group" => {
                    node.label = Some(next_val);
                    cx.emit(CanvasViewEvent::Changed);
                }
                "link" => {
                    node.url = Some(next_val);
                    cx.emit(CanvasViewEvent::Changed);
                }
                _ => {
                    if node.text.as_deref() != Some(next_val.as_str()) {
                        node.text = Some(next_val);
                        cx.emit(CanvasViewEvent::Changed);
                    }
                }
            }
        }
        cx.notify();
    }

    fn resolve_canvas_file(&self, raw: &str) -> Option<PathBuf> {
        if let Some(p) = self.vault.resolve_asset(raw, &self.doc_path) {
            return Some(p);
        }
        if let Some(p) = self.vault.resolve_wikilink(raw, &self.doc_path) {
            return Some(p);
        }
        let rel = self.vault.root.join(raw);
        rel.is_file().then_some(rel)
    }

    fn hit_test_edge(&self, rel_x: f32, rel_y: f32) -> Option<String> {
        let world_x = (rel_x - self.pan_x) / self.zoom;
        let world_y = (rel_y - self.pan_y) / self.zoom;
        let threshold_sq = (14.0 / self.zoom).powi(2);

        for edge in &self.doc.edges {
            let Some(from) = self.doc.nodes.iter().find(|n| n.id == edge.from_node) else {
                continue;
            };
            let Some(to) = self.doc.nodes.iter().find(|n| n.id == edge.to_node) else {
                continue;
            };
            let (p0, ca, cb, p3) = edge_endpoints(from, to, edge);
            for step in 0..=16 {
                let t = step as f32 / 16.0;
                let mt = 1.0 - t;
                let bx = mt * mt * mt * p0.0
                    + 3.0 * mt * mt * t * ca.0
                    + 3.0 * mt * t * t * cb.0
                    + t * t * t * p3.0;
                let by = mt * mt * mt * p0.1
                    + 3.0 * mt * mt * t * ca.1
                    + 3.0 * mt * t * t * cb.1
                    + t * t * t * p3.1;
                let dx = world_x - bx;
                let dy = world_y - by;
                if dx * dx + dy * dy <= threshold_sq {
                    return Some(edge.id.clone());
                }
            }
        }
        None
    }

    fn render_node_card(
        &self,
        node: &CanvasNode,
        vp_w: f32,
        vp_h: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let sx = self.pan_x + (node.x as f32) * self.zoom;
        let sy = self.pan_y + (node.y as f32) * self.zoom;
        let sw = ((node.width as f32) * self.zoom).max(40.0);
        let sh = ((node.height as f32) * self.zoom).max(24.0);

        if sx + sw < -40.0 || sy + sh < -40.0 || sx > vp_w + 40.0 || sy > vp_h + 40.0 {
            return None;
        }

        let is_group = node.kind == "group";
        let is_selected = self.selected_node.as_deref() == Some(node.id.as_str());
        let is_connect_src = self.connect_from.as_deref() == Some(node.id.as_str());
        let is_editing = self
            .editing_node
            .as_ref()
            .is_some_and(|e| e.node_id == node.id);

        let active_find_match = self.find_open
            && !self.find_query.trim().is_empty()
            && self
                .find_matches
                .get(self.find_active)
                .is_some_and(|id| id == &node.id);
        let any_find_match = self.find_open
            && !self.find_query.trim().is_empty()
            && self.find_matches.iter().any(|id| id == &node.id);

        let accent = canvas_color_u32(node.color.as_deref());
        let border_color = if is_connect_src {
            hex_color(kd::ORANGE)
        } else if active_find_match {
            hex_color(0xe8a849)
        } else if any_find_match {
            hex_color(0xc4b28a)
        } else if is_selected {
            hex_color(kd::ACCENT)
        } else if let Some(c) = accent {
            hex_color(c)
        } else {
            hex_color(kd::BORDER_SUBTLE)
        };

        let bg_color = if is_group {
            if let Some(c) = accent {
                hex_alpha((c << 8) | 0x18)
            } else {
                accent_alpha(0x14)
            }
        } else if let Some(c) = accent {
            hex_alpha((c << 8) | 0x1a)
        } else {
            hex_color(kd::BG_SIDEBAR)
        };

        let nid_down = node.id.clone();
        let nid_resize = node.id.clone();
        let node_x = node.x;
        let node_y = node.y;
        let node_w = node.width;
        let node_h = node.height;
        let nid_open_file = node.file.clone();

        let body_el: AnyElement = if is_editing {
            if let Some(ref ed) = self.editing_node {
                v_flex()
                    .size_full()
                    .gap_1()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .child(
                                Editor::new(&ed.editor)
                                    .size_full()
                                    .appearance(false)
                                    .bordered(false),
                            ),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_1()
                            .child(
                                Button::new(SharedString::from(format!("cnv-done-{}", node.id)))
                                    .primary()
                                    .xsmall()
                                    .icon(IconName::Check)
                                    .label("Done")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.commit_node_edit(cx);
                                        this.focus_handle.focus(window, cx);
                                    })),
                            ),
                    )
                    .into_any_element()
            } else {
                div().into_any_element()
            }
        } else {
            match node.kind.as_str() {
                "group" => div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(accent.map(hex_color).unwrap_or_else(|| hex_color(kd::AQUA)))
                    .child(node.label.clone().unwrap_or_else(|| "Group".to_string()))
                    .into_any_element(),
                "file" => {
                    let rel = node.file.clone().unwrap_or_else(|| "file".to_string());
                    let resolved = nid_open_file
                        .as_deref()
                        .and_then(|r| self.resolve_canvas_file(r));
                    let is_img = is_image_extension(&rel);
                    let open_path = resolved.clone();

                    v_flex()
                        .size_full()
                        .gap_1p5()
                        .child(
                            h_flex()
                                .justify_between()
                                .gap_1()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(hex_color(kd::ACCENT))
                                        .truncate()
                                        .child(rel.clone()),
                                )
                                .when_some(open_path, |this, path| {
                                    this.child(
                                        Button::new(SharedString::from(format!(
                                            "cnv-open-{}",
                                            node.id
                                        )))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::ExternalLink)
                                        .tooltip("Open file")
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(CanvasViewEvent::OpenFile(path.clone()));
                                        })),
                                    )
                                }),
                        )
                        .child(if is_img {
                            if let Some(p) = resolved {
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .rounded(px(4.))
                                    .overflow_hidden()
                                    .child(img(p).size_full().object_fit(ObjectFit::Contain))
                                    .into_any_element()
                            } else {
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child("Image not found in vault")
                                    .into_any_element()
                            }
                        } else {
                            let preview = resolved
                                .as_ref()
                                .and_then(|p| std::fs::read_to_string(p).ok())
                                .map(|txt| txt.lines().take(6).collect::<Vec<_>>().join("\n"))
                                .unwrap_or_else(|| "Vault note embed".to_string());
                            div()
                                .flex_1()
                                .min_h_0()
                                .overflow_hidden()
                                .text_xs()
                                .text_color(hex_color(kd::FG_SUBTLE))
                                .child(preview)
                                .into_any_element()
                        })
                        .into_any_element()
                }
                "link" => {
                    let url = node.url.clone().unwrap_or_default();
                    let url_copy = url.clone();
                    v_flex()
                        .size_full()
                        .justify_between()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(hex_color(kd::LINK))
                                .child(url),
                        )
                        .child(
                            h_flex().justify_end().child(
                                Button::new(SharedString::from(format!("cnv-link-{}", node.id)))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Copy)
                                    .label("Copy URL")
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            url_copy.clone(),
                                        ));
                                    })),
                            ),
                        )
                        .into_any_element()
                }
                _ => {
                    let txt = node.text.clone().unwrap_or_default();
                    let body = if txt.is_empty() {
                        "Double-click to edit…".to_string()
                    } else {
                        txt
                    };
                    div()
                        .size_full()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(if node.text.as_deref().unwrap_or("").is_empty() {
                            hex_color(kd::FG_MUTED)
                        } else {
                            hex_color(kd::FG)
                        })
                        .child(body)
                        .into_any_element()
                }
            }
        };

        let el = div()
            .id(SharedString::from(format!("cnv-node-{}", node.id)))
            .absolute()
            .left(px(sx))
            .top(px(sy))
            .w(px(sw))
            .h(px(sh))
            .rounded(px((8.0 * self.zoom).clamp(4.0, 12.0)))
            .bg(bg_color)
            .when(is_selected || is_connect_src || any_find_match, |s| s.border_2())
            .when(!is_selected && !is_connect_src && !any_find_match, |s| s.border_1())
            .border_color(border_color)
            .p(px((10.0 * self.zoom).clamp(5.0, 14.0)))
            .overflow_hidden()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.focus_handle.focus(window, cx);
                    this.selected_edge = None;

                    if this.connect_mode {
                        if let Some(from_id) = this.connect_from.take() {
                            if from_id != nid_down {
                                let eid = this.doc.add_edge(from_id, nid_down.clone());
                                this.selected_edge = Some(eid);
                                this.connect_mode = false;
                                cx.emit(CanvasViewEvent::Changed);
                            }
                        } else {
                            this.connect_from = Some(nid_down.clone());
                        }
                        this.selected_node = Some(nid_down.clone());
                        cx.notify();
                        return;
                    }

                    if ev.click_count >= 2 {
                        this.selected_node = Some(nid_down.clone());
                        this.start_node_edit(nid_down.clone(), window, cx);
                        return;
                    }

                    if this
                        .editing_node
                        .as_ref()
                        .is_some_and(|e| e.node_id != nid_down)
                    {
                        this.commit_node_edit(cx);
                    }

                    this.selected_node = Some(nid_down.clone());
                    this.drag = Some(CanvasDrag::MoveNode {
                        node_id: nid_down.clone(),
                        start_mouse: ev.position,
                        orig_x: node_x,
                        orig_y: node_y,
                        moved: false,
                    });
                    cx.notify();
                }),
            )
            .child(body_el)
            .when(!is_editing, |this| {
                this.child(
                    div()
                        .id(SharedString::from(format!("cnv-resize-{}", node.id)))
                        .absolute()
                        .right_0()
                        .bottom_0()
                        .w(px(14.))
                        .h(px(14.))
                        .cursor(CursorStyle::ResizeUpLeftDownRight)
                        .flex()
                        .items_end()
                        .justify_end()
                        .p(px(3.))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.selected_node = Some(nid_resize.clone());
                                this.drag = Some(CanvasDrag::ResizeNode {
                                    node_id: nid_resize.clone(),
                                    start_mouse: ev.position,
                                    orig_w: node_w,
                                    orig_h: node_h,
                                });
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .w(px(6.))
                                .h(px(6.))
                                .rounded(px(2.))
                                .bg(accent_alpha(0x66)),
                        ),
                )
            })
            .into_any_element();

        Some(el)
    }
}

impl Render for CanvasView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.source_mode {
            return div()
                .size_full()
                .bg(hex_color(kd::BG_MAIN))
                .child(wrap_editor_with_find(
                    Editor::new(&self.source_editor)
                        .size_full()
                        .appearance(false)
                        .bordered(false)
                        .into_any_element(),
                    &self.source_find_bar,
                    cx,
                ))
                .into_any_element();
        }

        if self.initial_fit_pending {
            let (_, _, vp_w, vp_h) = self.board_geo.get();
            if vp_w > 50.0 && vp_h > 50.0 {
                self.fit_view(cx);
            }
        }

        let (_, _, vp_w, vp_h) = self.board_geo.get();
        let mut node_elements: Vec<AnyElement> = Vec::with_capacity(self.doc.nodes.len());
        for node in self.doc.nodes.iter().filter(|n| n.kind == "group") {
            if let Some(el) = self.render_node_card(node, vp_w, vp_h, cx) {
                node_elements.push(el);
            }
        }
        for node in self.doc.nodes.iter().filter(|n| n.kind != "group") {
            if let Some(el) = self.render_node_card(node, vp_w, vp_h, cx) {
                node_elements.push(el);
            }
        }

        // Snapshot edge geometry cleanly for the background canvas painter.
        let zoom = self.zoom;
        let pan_x = self.pan_x;
        let pan_y = self.pan_y;
        let selected_edge = self.selected_edge.clone();
        let board_geo_ref = self.board_geo.clone();
        let edge_draw_list: Vec<(
            (f32, f32),
            (f32, f32),
            (f32, f32),
            (f32, f32),
            Option<u32>,
            bool,
            Option<String>,
        )> = self
            .doc
            .edges
            .iter()
            .filter_map(|e| {
                let from = self.doc.nodes.iter().find(|n| n.id == e.from_node)?;
                let to = self.doc.nodes.iter().find(|n| n.id == e.to_node)?;
                let (p0, ca, cb, p3) = edge_endpoints(from, to, e);
                Some((
                    p0,
                    ca,
                    cb,
                    p3,
                    canvas_color_u32(e.color.as_deref()),
                    selected_edge.as_deref() == Some(e.id.as_str()),
                    e.label.clone(),
                ))
            })
            .collect();
        let mut label_font = window.text_style().font();
        label_font.family = cx.theme().font_family.clone();

        let bg_canvas = canvas(
            move |bounds, _, _| {
                board_geo_ref.set((
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height),
                ));
                bounds
            },
            move |bounds, _, window, cx| {
                let ox = f32::from(bounds.origin.x);
                let oy = f32::from(bounds.origin.y);
                let w = f32::from(bounds.size.width);
                let h = f32::from(bounds.size.height);

                // Subtle infinite dot grid
                let step = (36.0 * zoom).clamp(14.0, 72.0);
                let start_gx = pan_x.rem_euclid(step);
                let start_gy = pan_y.rem_euclid(step);
                let dot_color = accent_alpha(0x26);
                let mut gx = start_gx;
                while gx < w {
                    let mut gy = start_gy;
                    while gy < h {
                        window.paint_quad(fill(
                            gpui_kit::Bounds {
                                origin: point(px(ox + gx), px(oy + gy)),
                                size: size(px(2.), px(2.)),
                            },
                            dot_color,
                        ));
                        gy += step;
                    }
                    gx += step;
                }

                let to_screen = |pt_w: (f32, f32)| -> Point<Pixels> {
                    point(px(ox + pan_x + pt_w.0 * zoom), px(oy + pan_y + pt_w.1 * zoom))
                };

                for (p0_w, ca_w, cb_w, p3_w, col_opt, is_sel, label_opt) in edge_draw_list {
                    let p0 = to_screen(p0_w);
                    let ca = to_screen(ca_w);
                    let cb = to_screen(cb_w);
                    let p3 = to_screen(p3_w);

                    let color = if is_sel {
                        hex_color(kd::ACCENT)
                    } else if let Some(c) = col_opt {
                        hex_color(c)
                    } else {
                        hex_color(kd::FG_MUTED)
                    };

                    let mut pb = PathBuilder::stroke(px((if is_sel { 3.0 } else { 1.8 }) * zoom));
                    pb.move_to(p0);
                    pb.cubic_bezier_to(p3, ca, cb);
                    if let Ok(path) = pb.build() {
                        window.paint_path(path, color);
                    }

                    // Directional arrowhead at p3
                    let dx = f32::from(p3.x) - f32::from(cb.x);
                    let dy = f32::from(p3.y) - f32::from(cb.y);
                    let len = dx.hypot(dy);
                    let (ux, uy) = if len > 0.5 {
                        (dx / len, dy / len)
                    } else {
                        let d2x = f32::from(p3.x) - f32::from(p0.x);
                        let d2y = f32::from(p3.y) - f32::from(p0.y);
                        let l2 = d2x.hypot(d2y).max(1.0);
                        (d2x / l2, d2y / l2)
                    };
                    let alen = (10.0 * zoom).clamp(6.0, 14.0);
                    let awid = (4.5 * zoom).clamp(3.0, 6.5);
                    let base_x = f32::from(p3.x) - ux * alen;
                    let base_y = f32::from(p3.y) - uy * alen;
                    let nx = -uy;
                    let ny = ux;
                    let mut arrow_pb = PathBuilder::fill();
                    arrow_pb.add_polygon(
                        &[
                            p3,
                            point(px(base_x + nx * awid), px(base_y + ny * awid)),
                            point(px(base_x - nx * awid), px(base_y - ny * awid)),
                        ],
                        true,
                    );
                    if let Ok(arrow_path) = arrow_pb.build() {
                        window.paint_path(arrow_path, color);
                    }

                    if let Some(lbl) = label_opt
                        && !lbl.trim().is_empty()
                    {
                        let mid_x = 0.125 * f32::from(p0.x)
                            + 0.375 * f32::from(ca.x)
                            + 0.375 * f32::from(cb.x)
                            + 0.125 * f32::from(p3.x);
                        let mid_y = 0.125 * f32::from(p0.y)
                            + 0.375 * f32::from(ca.y)
                            + 0.375 * f32::from(cb.y)
                            + 0.125 * f32::from(p3.y);
                        let text: SharedString = lbl.into();
                        let run = TextRun {
                            len: text.len(),
                            font: label_font.clone(),
                            color: hex_color(kd::FG),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shaped = window.text_system().shape_line(
                            text,
                            px((11.0 * zoom).clamp(9.0, 14.0)),
                            &[run],
                            None,
                        );
                        let pill_w = f32::from(shaped.width) + 12.0;
                        let pill_h = 20.0;
                        window.paint_quad(gpui_kit::quad(
                            gpui_kit::Bounds {
                                origin: point(px(mid_x - pill_w * 0.5), px(mid_y - pill_h * 0.5)),
                                size: size(px(pill_w), px(pill_h)),
                            },
                            px(4.),
                            hex_color(kd::BG_DEEP),
                            gpui_kit::Edges::all(px(1.)),
                            color,
                            gpui_kit::BorderStyle::Solid,
                        ));
                        let _ = shaped.paint(
                            point(px(mid_x - f32::from(shaped.width) * 0.5), px(mid_y - 7.5)),
                            px(16.),
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                }
            },
        )
        .size_full();

        div()
            .id(SharedString::from(format!("canvas-board-{:?}", cx.entity_id())))
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(hex_color(kd::BG_DEEP))
            .track_focus(&self.focus_handle)
            .when(self.connect_mode, |s| s.cursor(CursorStyle::Crosshair))
            .when(!self.connect_mode, |s| {
                s.cursor(if matches!(self.drag, Some(CanvasDrag::Pan { .. })) {
                    CursorStyle::ClosedHand
                } else {
                    CursorStyle::OpenHand
                })
            })
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if this.editing_node.is_some() {
                    if ev.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.editing_node = None;
                        cx.notify();
                    }
                    return;
                }
                match ev.keystroke.key.as_str() {
                    "delete" | "backspace" => {
                        cx.stop_propagation();
                        this.delete_selected(cx);
                    }
                    "f3" if this.find_open => {
                        cx.stop_propagation();
                        this.step_find(!ev.keystroke.modifiers.shift);
                        cx.notify();
                    }
                    "escape" => {
                        cx.stop_propagation();
                        if this.find_open {
                            this.close_find(Some(window), cx);
                        } else {
                            this.selected_node = None;
                            this.selected_edge = None;
                            this.connect_mode = false;
                            this.connect_from = None;
                            cx.notify();
                        }
                    }
                    _ => {}
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    this.focus_handle.focus(window, cx);
                    this.commit_node_edit(cx);
                    let (ox, oy, _, _) = this.board_geo.get();
                    let rx = f32::from(ev.position.x) - ox;
                    let ry = f32::from(ev.position.y) - oy;
                    this.selected_node = None;
                    this.selected_edge = this.hit_test_edge(rx, ry);
                    if this.connect_mode && this.selected_edge.is_none() {
                        this.connect_from = None;
                    }
                    if this.selected_edge.is_none() {
                        this.drag = Some(CanvasDrag::Pan {
                            start_mouse: ev.position,
                            orig_pan_x: this.pan_x,
                            orig_pan_y: this.pan_y,
                        });
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                let Some(drag) = this.drag.clone() else {
                    return;
                };
                if !ev.dragging() {
                    this.drag = None;
                    return;
                }
                match drag {
                    CanvasDrag::Pan {
                        start_mouse,
                        orig_pan_x,
                        orig_pan_y,
                    } => {
                        this.pan_x = orig_pan_x + f32::from(ev.position.x - start_mouse.x);
                        this.pan_y = orig_pan_y + f32::from(ev.position.y - start_mouse.y);
                        cx.notify();
                    }
                    CanvasDrag::MoveNode {
                        node_id,
                        start_mouse,
                        orig_x,
                        orig_y,
                        moved,
                    } => {
                        let dx = (f32::from(ev.position.x - start_mouse.x) / this.zoom) as f64;
                        let dy = (f32::from(ev.position.y - start_mouse.y) / this.zoom) as f64;
                        if let Some(node) = this.doc.nodes.iter_mut().find(|n| n.id == node_id) {
                            node.x = (orig_x + dx).round();
                            node.y = (orig_y + dy).round();
                            if !moved && (dx.abs() > 1.5 || dy.abs() > 1.5) {
                                this.drag = Some(CanvasDrag::MoveNode {
                                    node_id,
                                    start_mouse,
                                    orig_x,
                                    orig_y,
                                    moved: true,
                                });
                            }
                            cx.notify();
                        }
                    }
                    CanvasDrag::ResizeNode {
                        node_id,
                        start_mouse,
                        orig_w,
                        orig_h,
                    } => {
                        let dw = (f32::from(ev.position.x - start_mouse.x) / this.zoom) as f64;
                        let dh = (f32::from(ev.position.y - start_mouse.y) / this.zoom) as f64;
                        if let Some(node) = this.doc.nodes.iter_mut().find(|n| n.id == node_id) {
                            node.width = (orig_w + dw).max(120.0).round();
                            node.height = (orig_h + dh).max(60.0).round();
                            cx.notify();
                        }
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    let changed = match &this.drag {
                        Some(CanvasDrag::MoveNode { moved, .. }) => *moved,
                        Some(CanvasDrag::ResizeNode { .. }) => true,
                        _ => false,
                    };
                    this.drag = None;
                    if changed {
                        cx.emit(CanvasViewEvent::Changed);
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    let changed = match &this.drag {
                        Some(CanvasDrag::MoveNode { moved, .. }) => *moved,
                        Some(CanvasDrag::ResizeNode { .. }) => true,
                        _ => false,
                    };
                    this.drag = None;
                    if changed {
                        cx.emit(CanvasViewEvent::Changed);
                    }
                }),
            )
            .on_scroll(cx.listener(|this, ev: &ScrollEvent, _, cx| {
                let d = ev.delta.pixel_delta(px(28.));
                let (ox, oy, _, _) = this.board_geo.get();
                if ev.modifiers.control {
                    let factor = if f32::from(d.y) > 0.0 { 1.1 } else { 0.9 };
                    let sx = f32::from(ev.position.x) - ox;
                    let sy = f32::from(ev.position.y) - oy;
                    this.zoom_around(sx, sy, this.zoom * factor);
                } else {
                    this.pan_x += f32::from(d.x);
                    this.pan_y += f32::from(d.y);
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .child(bg_canvas)
            .children(node_elements)
            .when(self.find_open, |board| {
                board.child(self.render_find_overlay(window, cx))
            })
            .into_any_element()
    }
}

pub fn render_canvas_tab(
    app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Canvas { view, .. } = &tab.kind else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let dirty = tab.dirty;
    let zen_mode = app.zen_mode;
    let cv_read = view.read(cx);
    let summary = cv_read.summary();
    let has_selection = cv_read.selected_node.is_some() || cv_read.selected_edge.is_some();
    let connect_mode = cv_read.connect_mode;
    let connect_from = cv_read.connect_from.is_some();
    let source_mode = cv_read.source_mode;
    let find_open = cv_read.is_find_open(cx);

    let color_presets: [(&str, u32); 6] = [
        ("1", 0xc4746e),
        ("2", 0xb6927b),
        ("3", 0xc4b28a),
        ("4", 0x87a987),
        ("5", 0x8ba4b0),
        ("6", 0xa292a3),
    ];

    let v_find = view.clone();
    let v_card = view.clone();
    let v_group = view.clone();
    let v_conn = view.clone();
    let v_del = view.clone();
    let v_zout = view.clone();
    let v_z100 = view.clone();
    let v_zin = view.clone();
    let v_fit = view.clone();
    let v_src = view.clone();

    let header = h_flex()
        .h(px(36.))
        .w_full()
        .px_3()
        .gap_2()
        .items_center()
        .justify_between()
        .bg(hex_color(kd::BG_SIDEBAR))
        .border_b_1()
        .border_color(hex_color(kd::BORDER_SUBTLE))
        .child(
            h_flex()
                .min_w_0()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(hex_color(kd::FG_MUTED))
                        .truncate()
                        .child(rel_path),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(hex_color(kd::VIOLET))
                        .child(summary),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new(SharedString::from(format!("cnv-find-{tab_id}")))
                        .with_variant(if find_open {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Search)
                        .tooltip("Find Nodes (Ctrl+F)")
                        .on_click(cx.listener(move |_, _, window, cx| {
                            v_find.update(cx, |v, cx| v.open_find(false, window, cx));
                        })),
                )
                .when(!source_mode, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("cnv-add-card-{tab_id}")))
                            .secondary()
                            .xsmall()
                            .icon(IconName::Plus)
                            .label("Card")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_card.update(cx, |v, cx| v.add_card_at_center(cx));
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("cnv-add-grp-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::FolderPlus)
                            .label("Group")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_group.update(cx, |v, cx| v.add_group_at_center(cx));
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("cnv-connect-{tab_id}")))
                            .with_variant(if connect_mode {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .xsmall()
                            .label(if connect_from {
                                "Pick Target…"
                            } else {
                                "Connect"
                            })
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_conn.update(cx, |v, cx| {
                                    v.connect_mode = !v.connect_mode;
                                    v.connect_from = None;
                                    cx.notify();
                                });
                            })),
                    )
                    .when(has_selection, |row| {
                        row.children(color_presets.iter().enumerate().map(|(ix, (code, hex))| {
                            let vc = view.clone();
                            let code_str = *code;
                            div()
                                .id(SharedString::from(format!("cnv-col-{tab_id}-{ix}")))
                                .w(px(14.))
                                .h(px(14.))
                                .rounded_full()
                                .bg(hex_color(*hex))
                                .border_1()
                                .border_color(hex_color(kd::BORDER_SUBTLE))
                                .cursor_pointer()
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    vc.update(cx, |v, cx| v.set_selected_color(Some(code_str), cx));
                                }))
                        }))
                        .child(
                            Button::new(SharedString::from(format!("cnv-del-{tab_id}")))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Trash)
                                .tooltip("Delete selected")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    v_del.update(cx, |v, cx| v.delete_selected(cx));
                                })),
                        )
                    })
                    .child(
                        Button::new(SharedString::from(format!("cnv-zout-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::ZoomOut)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_zout.update(cx, |v, cx| v.adjust_zoom(0.85, cx));
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("cnv-z100-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .label("100%")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_z100.update(cx, |v, cx| v.reset_zoom(cx));
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("cnv-zin-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::ZoomIn)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_zin.update(cx, |v, cx| v.adjust_zoom(1.18, cx));
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("cnv-fit-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .label("Fit")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_fit.update(cx, |v, cx| v.fit_view(cx));
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("cnv-src-{tab_id}")))
                        .with_variant(if source_mode {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Code)
                        .label(if source_mode { "Board" } else { "JSON" })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let res = v_src.update(cx, |v, cx| v.toggle_source_mode(window, cx));
                            if let Err(err) = res {
                                this.toast_err(err, window, cx);
                            }
                        })),
                )
                .when(dirty && !app.config.autosave, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("cnv-save-{tab_id}")))
                            .primary()
                            .xsmall()
                            .icon(IconName::Save)
                            .label("Save")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.save_tab(side, tab_id, window, cx);
                            })),
                    )
                }),
        );

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_DEEP))
        .when(!zen_mode, |this| this.child(header))
        .child(div().flex_1().min_h_0().w_full().child(view.clone()))
        .into_any_element()
}
