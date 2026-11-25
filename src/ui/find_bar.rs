use crate::theme::{hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{EditorState, Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, ElementId, Entity, Focusable as _, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement, Render,
    Stateful, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};

pub const FIND_INPUT_W: f32 = 176.0;

pub fn floating_find_card(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .top(px(8.))
        .right(px(18.))
        .p_1p5()
        .rounded(px(8.))
        .bg(hex_color(kd::BG_SIDEBAR))
        .border_1()
        .border_color(hex_color(kd::BORDER))
        .shadow_md()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
}

pub fn find_count_badge(label: impl Into<gpui_kit::SharedString>, is_zero: bool) -> Div {
    div()
        .min_w(px(42.))
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(if is_zero {
            hex_color(kd::RED)
        } else {
            hex_color(kd::FG_MUTED)
        })
        .child(label.into())
}

pub fn wrap_editor_with_find(
    editor_el: AnyElement,
    find_bar: &Entity<EditorFindBar>,
    cx: &App,
) -> AnyElement {
    let is_open = find_bar.read(cx).is_open();
    div()
        .relative()
        .size_full()
        .child(editor_el)
        .when(is_open, |d| d.child(find_bar.clone()))
        .into_any_element()
}

pub struct EditorFindBar {
    editor: Entity<EditorState>,
    open: bool,
    replace_mode: bool,
    case_sensitive: bool,
    query_input: Entity<InputState>,
    replace_input: Entity<InputState>,
    _subs: Vec<Subscription>,
}

impl EditorFindBar {
    pub fn new(editor: Entity<EditorState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find…"));
        let replace_input = cx.new(|cx| InputState::new(window, cx).placeholder("Replace…"));

        let q_sub = cx.subscribe(&query_input, |this, _, event, cx| match event {
            InputEvent::Change => {
                this.sync_query(true, cx);
            }
            InputEvent::PressEnter {
                secondary, shift, ..
            } => {
                if *shift || *secondary {
                    this.prev_match(cx);
                } else {
                    this.next_match(cx);
                }
            }
            _ => {}
        });

        let r_sub = cx.subscribe_in(&replace_input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.replace_current(window, cx);
            }
        });

        let ed_sub = cx.subscribe(&editor, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) && this.open {
                cx.notify();
            }
        });

        Self {
            editor,
            open: false,
            replace_mode: false,
            case_sensitive: false,
            query_input,
            replace_input,
            _subs: vec![q_sub, r_sub, ed_sub],
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self, replace_mode: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        if replace_mode {
            self.replace_mode = self.editor.read(cx).is_replaceable();
        }
        let sel = self.editor.read(cx).selected_text().to_string();
        let had_new_selection = !sel.is_empty() && !sel.contains('\n') && sel.len() <= 120;
        if had_new_selection {
            self.query_input
                .update(cx, |s, cx| s.set_value(sel, window, cx));
        }
        self.sync_query(true, cx);

        let cur_q_empty = self.query_input.read(cx).value().is_empty();
        let focus_replace = self.replace_mode && !had_new_selection && !cur_q_empty;
        if focus_replace {
            self.replace_input.update(cx, |s, cx| {
                let len = s.value().len();
                s.focus(window, cx);
                if len > 0 {
                    s.set_selected_range(0..len, cx);
                }
            });
        } else {
            self.query_input.update(cx, |s, cx| {
                let len = s.value().len();
                s.focus(window, cx);
                if len > 0 {
                    s.set_selected_range(0..len, cx);
                }
            });
        }
        cx.notify();
    }

    pub fn close(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.editor.update(cx, |s, cx| s.close_search(cx));
        if let Some(window) = window {
            self.editor.focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn sync_query(&mut self, scroll_to_match: bool, cx: &mut Context<Self>) {
        let q = self.query_input.read(cx).value().to_string();
        let case_insensitive = !self.case_sensitive;
        self.editor.update(cx, |s, cx| {
            s.set_search_query(q.clone(), case_insensitive, cx);
            if scroll_to_match && !q.is_empty() && !s.search_session().matcher.is_empty() {
                s.previous_search_match(cx);
                s.next_search_match(cx);
            }
        });
        cx.notify();
    }

    pub fn next_match(&mut self, cx: &mut Context<Self>) {
        self.editor.update(cx, |s, cx| {
            let _ = s.next_search_match(cx);
        });
        cx.notify();
    }

    pub fn prev_match(&mut self, cx: &mut Context<Self>) {
        self.editor.update(cx, |s, cx| {
            let _ = s.previous_search_match(cx);
        });
        cx.notify();
    }

    pub fn toggle_case(&mut self, cx: &mut Context<Self>) {
        self.case_sensitive = !self.case_sensitive;
        self.sync_query(true, cx);
    }

    pub fn replace_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rep = self.replace_input.read(cx).value().to_string();
        self.editor.update(cx, |s, cx| {
            let _ = s.replace_current_search_match(&rep, window, cx);
        });
        cx.notify();
    }

    pub fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rep = self.replace_input.read(cx).value().to_string();
        self.editor.update(cx, |s, cx| {
            let _ = s.replace_all_search_matches(&rep, window, cx);
        });
        cx.notify();
    }
}

impl Render for EditorFindBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }

        let raw_q = self.query_input.read(cx).value().to_string();
        let has_query = !raw_q.is_empty();
        let replaceable = self.editor.read(cx).is_replaceable();
        let replace_mode = self.replace_mode && replaceable;
        let (match_count, cur_opt) = {
            let ed = self.editor.read(cx);
            let m = &ed.search_session().matcher;
            (m.len(), m.current())
        };
        let count_label = if !has_query {
            String::new()
        } else if match_count == 0 {
            "0 / 0".to_string()
        } else {
            format!("{} / {}", cur_opt.map_or(1, |i| i + 1), match_count)
        };

        let clear_q_btn = Button::new("ed-find-clear")
            .text()
            .xsmall()
            .tab_stop(false)
            .icon(IconName::Close)
            .tooltip("Clear")
            .on_click(cx.listener(|this, _, window, cx| {
                this.query_input
                    .update(cx, |s, cx| s.set_value("", window, cx));
                this.sync_query(false, cx);
                this.query_input.focus_handle(cx).focus(window, cx);
            }));

        let row1 = h_flex()
            .gap_1()
            .items_center()
            .when(replaceable, |r| {
                r.child(
                    Button::new("ed-find-toggle-rep")
                        .ghost()
                        .xsmall()
                        .icon(if replace_mode {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .tooltip("Toggle Replace (Ctrl+H)")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.replace_mode = !this.replace_mode;
                            cx.notify();
                        })),
                )
            })
            .child(
                div().w(px(FIND_INPUT_W)).child(
                    Input::new(&self.query_input)
                        .xsmall()
                        .prefix(
                            Icon::new(IconName::Search)
                                .xsmall()
                                .text_color(hex_color(kd::FG_MUTED)),
                        )
                        .when(has_query, |i| i.suffix(clear_q_btn)),
                ),
            )
            .when(!count_label.is_empty(), |row| {
                row.child(find_count_badge(count_label, has_query && match_count == 0))
            })
            .child(
                Button::new("ed-find-case")
                    .with_variant(if self.case_sensitive {
                        ButtonVariant::Secondary
                    } else {
                        ButtonVariant::Ghost
                    })
                    .xsmall()
                    .label("Aa")
                    .tooltip("Match Case")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_case(cx);
                    })),
            )
            .child(
                Button::new("ed-find-prev")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronUp)
                    .tooltip("Previous Match (Shift+Enter)")
                    .disabled(match_count == 0)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.prev_match(cx);
                    })),
            )
            .child(
                Button::new("ed-find-next")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronDown)
                    .tooltip("Next Match (Enter)")
                    .disabled(match_count == 0)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.next_match(cx);
                    })),
            )
            .child(
                Button::new("ed-find-close")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("Close (Esc)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close(Some(window), cx);
                    })),
            );

        let row2 = if replace_mode {
            Some(
                h_flex()
                    .gap_1()
                    .items_center()
                    .pl(px(26.))
                    .child(
                        div()
                            .w(px(FIND_INPUT_W))
                            .child(Input::new(&self.replace_input).xsmall()),
                    )
                    .child(
                        Button::new("ed-rep-one")
                            .secondary()
                            .xsmall()
                            .label("Replace")
                            .disabled(match_count == 0)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.replace_current(window, cx);
                            })),
                    )
                    .child(
                        Button::new("ed-rep-all")
                            .ghost()
                            .xsmall()
                            .label("All")
                            .disabled(match_count == 0)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.replace_all(window, cx);
                            })),
                    ),
            )
        } else {
            None
        };

        floating_find_card(format!("ed-find-{:?}", cx.entity_id()))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                match ev.keystroke.key.as_str() {
                    "escape" => {
                        cx.stop_propagation();
                        this.close(Some(window), cx);
                    }
                    "f3" => {
                        cx.stop_propagation();
                        if ev.keystroke.modifiers.shift {
                            this.prev_match(cx);
                        } else {
                            this.next_match(cx);
                        }
                    }
                    "tab" if this.replace_mode => {
                        cx.stop_propagation();
                        let q_focused = this.query_input.focus_handle(cx).is_focused(window);
                        if q_focused {
                            this.replace_input.focus_handle(cx).focus(window, cx);
                        } else {
                            this.query_input.focus_handle(cx).focus(window, cx);
                        }
                    }
                    _ => {}
                }
            }))
            .child(v_flex().gap_1().child(row1).children(row2))
            .into_any_element()
    }
}
