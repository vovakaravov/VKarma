use super::{PaneSide, TabKind, VKarmaApp};
use crate::formats::kanban::KanbanBoard;
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use std::collections::HashMap;

pub enum KanbanViewEvent {
    Changed,
}

#[derive(Clone)]
struct DraggedKanbanCard {
    from_col_id: u64,
    card_id: u64,
    title: String,
}

struct DraggedKanbanPreview {
    title: String,
}

impl Render for DraggedKanbanPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2p5()
            .py_1p5()
            .rounded(px(6.))
            .bg(hex_color(kd::BG_PANEL))
            .border_1()
            .border_color(hex_color(kd::ACCENT))
            .text_xs()
            .text_color(hex_color(kd::FG))
            .max_w(px(240.))
            .truncate()
            .child(self.title.clone())
    }
}

enum InlineKanbanEdit {
    Card {
        col_id: u64,
        card_id: u64,
        input: Entity<InputState>,
        _sub: Subscription,
    },
    Column {
        col_id: u64,
        input: Entity<InputState>,
        _sub: Subscription,
    },
}

pub struct KanbanView {
    pub board: KanbanBoard,
    focus_handle: FocusHandle,
    add_inputs: HashMap<u64, Entity<InputState>>,
    _add_subs: HashMap<u64, Subscription>,
    inline_edit: Option<InlineKanbanEdit>,
    adding_column_input: Option<(Entity<InputState>, Subscription)>,
    source_mode: bool,
    source_editor: Entity<EditorState>,
    _source_sub: Subscription,
}

impl EventEmitter<KanbanViewEvent> for KanbanView {}

impl Focusable for KanbanView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl KanbanView {
    pub fn new(
        raw: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let board = KanbanBoard::parse(&raw);
        let source_editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("markdown")
                .soft_wrap(true)
                .line_number(true)
                .default_value(raw)
        });
        let source_sub = cx.subscribe(&source_editor, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) && this.source_mode {
                cx.emit(KanbanViewEvent::Changed);
            }
        });

        let mut view = Self {
            board,
            focus_handle: cx.focus_handle(),
            add_inputs: HashMap::new(),
            _add_subs: HashMap::new(),
            inline_edit: None,
            adding_column_input: None,
            source_mode: false,
            source_editor,
            _source_sub: source_sub,
        };
        view.ensure_column_inputs(window, cx);
        view
    }

    fn ensure_column_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let col_ids: Vec<u64> = self.board.columns.iter().map(|c| c.id).collect();
        self.add_inputs.retain(|id, _| col_ids.contains(id));
        self._add_subs.retain(|id, _| col_ids.contains(id));

        for cid in col_ids {
            if self.add_inputs.contains_key(&cid) {
                continue;
            }
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("+ Add card…"));
            let input_ref = input.clone();
            let sub = cx.subscribe_in(&input, window, move |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let val = input_ref.read(cx).value().to_string();
                    if this.board.add_card(cid, val).is_some() {
                        input_ref.update(cx, |s, cx| s.set_value("", window, cx));
                        cx.emit(KanbanViewEvent::Changed);
                        cx.notify();
                    }
                }
            });
            self.add_inputs.insert(cid, input);
            self._add_subs.insert(cid, sub);
        }
    }

    pub fn to_markdown(&self, cx: &App) -> String {
        if self.source_mode {
            self.source_editor.read(cx).value().to_string()
        } else {
            self.board.to_markdown()
        }
    }

    pub fn summary(&self) -> String {
        let (cols, done, total) = self.board.counts();
        format!("{cols} cols · {done}/{total} done")
    }

    pub fn toggle_source_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.source_mode {
            let raw = self.source_editor.read(cx).value().to_string();
            self.board = KanbanBoard::parse(&raw);
            self.ensure_column_inputs(window, cx);
            self.source_mode = false;
            self.focus_handle.focus(window, cx);
        } else {
            self.commit_inline_edit(cx);
            let md = self.board.to_markdown();
            self.source_editor
                .update(cx, |s, cx| s.set_value(md, window, cx));
            self.source_mode = true;
            self.source_editor.focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    pub fn begin_add_column(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("New column name…"));
        let input_ref = input.clone();
        let sub = cx.subscribe_in(&input, window, move |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let name = input_ref.read(cx).value().to_string();
                this.adding_column_input = None;
                if !name.trim().is_empty() {
                    this.board.add_column(name);
                    this.ensure_column_inputs(window, cx);
                    cx.emit(KanbanViewEvent::Changed);
                }
                cx.notify();
            }
        });
        input.focus_handle(cx).focus(window, cx);
        self.adding_column_input = Some((input, sub));
        cx.notify();
    }

    fn begin_card_edit(
        &mut self,
        col_id: u64,
        card_id: u64,
        orig: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(orig));
        let sub = cx.subscribe_in(&input, window, move |this, _, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.commit_inline_edit(cx);
            }
        });
        input.focus_handle(cx).focus(window, cx);
        self.inline_edit = Some(InlineKanbanEdit::Card {
            col_id,
            card_id,
            input,
            _sub: sub,
        });
        cx.notify();
    }

    fn begin_column_edit(
        &mut self,
        col_id: u64,
        orig: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(orig));
        let sub = cx.subscribe_in(&input, window, move |this, _, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.commit_inline_edit(cx);
            }
        });
        input.focus_handle(cx).focus(window, cx);
        self.inline_edit = Some(InlineKanbanEdit::Column {
            col_id,
            input,
            _sub: sub,
        });
        cx.notify();
    }

    fn commit_inline_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.inline_edit.take() else {
            return;
        };
        match edit {
            InlineKanbanEdit::Card {
                col_id,
                card_id,
                input,
                ..
            } => {
                let next = input.read(cx).value().to_string();
                self.board.update_card(col_id, card_id, next);
            }
            InlineKanbanEdit::Column { col_id, input, .. } => {
                let next = input.read(cx).value().to_string();
                self.board.rename_column(col_id, next);
            }
        }
        cx.emit(KanbanViewEvent::Changed);
        cx.notify();
    }
}

fn tag_badge_color(tag: &str) -> u32 {
    let lower = tag.to_ascii_lowercase();
    if lower.contains("bug") || lower.contains("p0") || lower.contains("urgent") {
        kd::RED
    } else if lower.contains("ui") || lower.contains("design") {
        kd::VIOLET
    } else if lower.contains("done") || lower.contains("perf") {
        kd::GREEN
    } else if lower.contains("wip") || lower.contains("p1") {
        kd::ORANGE
    } else {
        kd::BLUE
    }
}

impl Render for KanbanView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.source_mode {
            return div()
                .size_full()
                .bg(hex_color(kd::BG_MAIN))
                .child(
                    Editor::new(&self.source_editor)
                        .size_full()
                        .appearance(false)
                        .bordered(false),
                )
                .into_any_element();
        }

        let col_count = self.board.columns.len();
        let columns_ui = self.board.columns.iter().enumerate().map(|(col_ix, col)| {
            let col_id = col.id;
            let col_title = col.title.clone();
            let done_cnt = col.cards.iter().filter(|c| c.done).count();
            let tot_cnt = col.cards.len();

            let editing_this_col = match &self.inline_edit {
                Some(InlineKanbanEdit::Column { col_id: cid, input, .. }) if *cid == col_id => {
                    Some(input.clone())
                }
                _ => None,
            };

            let cards_ui = col.cards.iter().enumerate().map(|(card_ix, card)| {
                let card_id = card.id;
                let full_title = card.title.clone();
                let disp_title = card.display_title();
                let tags = card.extract_tags();
                let done = card.done;
                let notes = card.notes.clone();

                let editing_this_card = match &self.inline_edit {
                    Some(InlineKanbanEdit::Card {
                        col_id: cid,
                        card_id: kid,
                        input,
                        ..
                    }) if *cid == col_id && *kid == card_id => Some(input.clone()),
                    _ => None,
                };

                div()
                    .id(SharedString::from(format!("kbn-card-{col_id}-{card_id}")))
                    .w_full()
                    .p_2p5()
                    .rounded(px(7.))
                    .bg(hex_color(kd::BG_MAIN))
                    .border_1()
                    .border_color(hex_color(kd::BORDER_SUBTLE))
                    .cursor_pointer()
                    .on_drag(
                        DraggedKanbanCard {
                            from_col_id: col_id,
                            card_id,
                            title: disp_title.clone(),
                        },
                        |drag, _, _, cx| {
                            cx.new(|_| DraggedKanbanPreview {
                                title: drag.title.clone(),
                            })
                        },
                    )
                    .drag_over::<DraggedKanbanCard>(|style, _, _, _| {
                        style.border_t_2().border_color(hex_color(kd::ACCENT))
                    })
                    .on_drop(cx.listener(move |this, drag: &DraggedKanbanCard, _, cx| {
                        cx.stop_propagation();
                        this.board.move_card(
                            drag.from_col_id,
                            drag.card_id,
                            col_id,
                            Some(card_ix),
                        );
                        cx.emit(KanbanViewEvent::Changed);
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                        if ev.click_count() >= 2 {
                            this.begin_card_edit(
                                col_id,
                                card_id,
                                full_title.clone(),
                                window,
                                cx,
                            );
                        }
                    }))
                    .child(if let Some(ref ren_input) = editing_this_card {
                        div()
                            .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                                if ev.keystroke.key == "escape" {
                                    cx.stop_propagation();
                                    this.inline_edit = None;
                                    cx.notify();
                                }
                            }))
                            .child(Input::new(ren_input).xsmall())
                            .into_any_element()
                    } else {
                        v_flex()
                            .gap_1p5()
                            .child(
                                h_flex()
                                    .items_start()
                                    .justify_between()
                                    .gap_1p5()
                                    .child(
                                        h_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .items_start()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "kbn-chk-{card_id}"
                                                    )))
                                                    .mt(px(2.))
                                                    .w(px(14.))
                                                    .h(px(14.))
                                                    .rounded(px(3.))
                                                    .border_1()
                                                    .border_color(if done {
                                                        hex_color(kd::GREEN)
                                                    } else {
                                                        hex_color(kd::FG_MUTED)
                                                    })
                                                    .bg(if done {
                                                        hex_color(kd::GREEN)
                                                    } else {
                                                        hex_color(kd::BG_DEEP)
                                                    })
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .cursor_pointer()
                                                    .on_mouse_down(
                                                        gpui_kit::MouseButton::Left,
                                                        |_, _, cx| cx.stop_propagation(),
                                                    )
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        cx.stop_propagation();
                                                        this.board.toggle_card(col_id, card_id);
                                                        cx.emit(KanbanViewEvent::Changed);
                                                        cx.notify();
                                                    }))
                                                    .when(done, |box_el| {
                                                        box_el.child(
                                                            Icon::new(IconName::Check)
                                                                .xsmall()
                                                                .text_color(hex_color(kd::BG_MAIN)),
                                                        )
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_xs()
                                                    .text_color(if done {
                                                        hex_color(kd::FG_MUTED)
                                                    } else {
                                                        hex_color(kd::FG)
                                                    })
                                                    .child(disp_title),
                                            ),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "kbn-del-card-{card_id}"
                                        )))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::X)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.board.remove_card(col_id, card_id);
                                            cx.emit(KanbanViewEvent::Changed);
                                            cx.notify();
                                        })),
                                    ),
                            )
                            .when(!tags.is_empty(), |card_el| {
                                card_el.child(
                                    h_flex().gap_1().flex_wrap().children(tags.into_iter().map(
                                        |tag| {
                                            let c = tag_badge_color(&tag);
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded(px(4.))
                                                .bg(hex_alpha((c << 8) | 0x26))
                                                .text_size(px(10.))
                                                .text_color(hex_color(c))
                                                .child(format!("#{tag}"))
                                        },
                                    )),
                                )
                            })
                            .when(!notes.is_empty(), |card_el| {
                                card_el.child(
                                    div()
                                        .pl_5()
                                        .text_size(px(11.))
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .child(notes.join(" · ")),
                                )
                            })
                            .into_any_element()
                    })
            });

            let add_input = self.add_inputs.get(&col_id).cloned();

            v_flex()
                .id(SharedString::from(format!("kbn-col-{col_id}")))
                .w(px(286.))
                .flex_shrink_0()
                .max_h_full()
                .rounded(px(9.))
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_1()
                .border_color(hex_color(kd::BORDER_SUBTLE))
                .drag_over::<DraggedKanbanCard>(|style, _, _, _| {
                    style.border_color(hex_color(kd::ACCENT)).bg(accent_alpha(0x18))
                })
                .on_drop(cx.listener(move |this, drag: &DraggedKanbanCard, _, cx| {
                    cx.stop_propagation();
                    this.board
                        .move_card(drag.from_col_id, drag.card_id, col_id, None);
                    cx.emit(KanbanViewEvent::Changed);
                    cx.notify();
                }))
                .child(
                    h_flex()
                        .h(px(38.))
                        .px_3()
                        .items_center()
                        .justify_between()
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .child(if let Some(ref ren_col) = editing_this_col {
                            div()
                                .flex_1()
                                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                                    if ev.keystroke.key == "escape" {
                                        cx.stop_propagation();
                                        this.inline_edit = None;
                                        cx.notify();
                                    }
                                }))
                                .child(Input::new(ren_col).xsmall())
                                .into_any_element()
                        } else {
                            let col_title_click = col_title.clone();
                            h_flex()
                                .id(SharedString::from(format!("kbn-col-hdr-{col_id}")))
                                .flex_1()
                                .min_w_0()
                                .gap_2()
                                .items_center()
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                                    if ev.click_count() >= 2 {
                                        this.begin_column_edit(
                                            col_id,
                                            col_title_click.clone(),
                                            window,
                                            cx,
                                        );
                                    }
                                }))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(hex_color(kd::ACCENT))
                                        .truncate()
                                        .child(col_title),
                                )
                                .child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(4.))
                                        .bg(hex_color(kd::BG_DEEP))
                                        .text_size(px(10.))
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .child(format!("{done_cnt}/{tot_cnt}")),
                                )
                                .into_any_element()
                        })
                        .child(
                            h_flex()
                                .gap_0p5()
                                .when(col_ix > 0, |row| {
                                    row.child(
                                        Button::new(SharedString::from(format!(
                                            "kbn-col-l-{col_id}"
                                        )))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::ChevronLeft)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.board.move_column(col_id, -1);
                                            cx.emit(KanbanViewEvent::Changed);
                                            cx.notify();
                                        })),
                                    )
                                })
                                .when(col_ix + 1 < col_count, |row| {
                                    row.child(
                                        Button::new(SharedString::from(format!(
                                            "kbn-col-r-{col_id}"
                                        )))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::ChevronRight)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.board.move_column(col_id, 1);
                                            cx.emit(KanbanViewEvent::Changed);
                                            cx.notify();
                                        })),
                                    )
                                })
                                .when(col_count > 1, |row| {
                                    row.child(
                                        Button::new(SharedString::from(format!(
                                            "kbn-col-del-{col_id}"
                                        )))
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Trash)
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.board.remove_column(col_id);
                                            this.ensure_column_inputs(window, cx);
                                            cx.emit(KanbanViewEvent::Changed);
                                            cx.notify();
                                        })),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("kbn-col-scroll-{col_id}")))
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .p_2()
                        .child(v_flex().gap_2().children(cards_ui)),
                )
                .when_some(add_input, |col_el, input| {
                    col_el.child(
                        div()
                            .p_2()
                            .border_t_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .child(Input::new(&input).xsmall()),
                    )
                })
        });

        div()
            .id(SharedString::from(format!("kanban-board-{:?}", cx.entity_id())))
            .size_full()
            .bg(hex_color(kd::BG_DEEP))
            .track_focus(&self.focus_handle)
            .overflow_x_scrollbar()
            .p_4()
            .child(
                h_flex()
                    .h_full()
                    .items_start()
                    .gap_3()
                    .children(columns_ui)
                    .when_some(self.adding_column_input.as_ref(), |row, (input, _)| {
                        row.child(
                            div()
                                .w(px(260.))
                                .p_2p5()
                                .rounded(px(9.))
                                .bg(hex_color(kd::BG_SIDEBAR))
                                .border_1()
                                .border_color(hex_color(kd::ACCENT))
                                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                                    if ev.keystroke.key == "escape" {
                                        cx.stop_propagation();
                                        this.adding_column_input = None;
                                        cx.notify();
                                    }
                                }))
                                .child(Input::new(input).xsmall()),
                        )
                    }),
            )
            .into_any_element()
    }
}

pub fn render_kanban_tab(
    app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Kanban { view, .. } = &tab.kind else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let dirty = tab.dirty;
    let zen_mode = app.zen_mode;
    let kv = view.read(cx);
    let summary = kv.summary();
    let source_mode = kv.source_mode;

    let v_add_col = view.clone();
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
                        .text_color(hex_color(kd::AQUA))
                        .child(summary),
                ),
        )
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .when(!source_mode, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("kbn-new-col-{tab_id}")))
                            .secondary()
                            .xsmall()
                            .icon(IconName::Plus)
                            .label("Column")
                            .on_click(cx.listener(move |_, _, window, cx| {
                                v_add_col.update(cx, |v, cx| v.begin_add_column(window, cx));
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("kbn-src-{tab_id}")))
                        .with_variant(if source_mode {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Code)
                        .label(if source_mode { "Board" } else { "Markdown" })
                        .on_click(cx.listener(move |_, _, window, cx| {
                            v_src.update(cx, |v, cx| v.toggle_source_mode(window, cx));
                        })),
                )
                .when(dirty, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("kbn-save-{tab_id}")))
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
