use super::{PaneSide, TabKind, VKarmaApp};
use crate::formats::subtitle::{
    SubtitleDoc, SubtitleFormat, format_duration_short, format_timestamp_ms, parse_timestamp_ms,
};
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

pub enum SubtitleViewEvent {
    Changed,
}

pub struct SubtitleView {
    pub doc: SubtitleDoc,
    focus_handle: FocusHandle,
    filter_input: Entity<InputState>,
    _filter_sub: Subscription,
    selected_cue_id: Option<u64>,
    edit_start: Entity<InputState>,
    edit_end: Entity<InputState>,
    edit_text: Entity<EditorState>,
    _edit_subs: [Subscription; 3],
    syncing_editor: bool,
    source_mode: bool,
    source_editor: Entity<EditorState>,
    _source_sub: Subscription,
}

impl EventEmitter<SubtitleViewEvent> for SubtitleView {}

impl Focusable for SubtitleView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SubtitleView {
    pub fn new(
        raw: String,
        default_format: SubtitleFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let doc = SubtitleDoc::parse(&raw, default_format);
        let first_id = doc.cues.first().map(|c| c.id);

        let filter_input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter cues…"));
        let filter_sub = cx.subscribe(&filter_input, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });

        let edit_start = cx.new(|cx| InputState::new(window, cx));
        let edit_end = cx.new(|cx| InputState::new(window, cx));
        let edit_text = cx.new(|cx| EditorState::new(window, cx).soft_wrap(true));

        let s1 = cx.subscribe(&edit_start, |this, inp, event, cx| {
            if !this.syncing_editor && matches!(event, InputEvent::Change) {
                let raw = inp.read(cx).value().to_string();
                if let Some(ms) = parse_timestamp_ms(&raw)
                    && let Some(cid) = this.selected_cue_id
                    && let Some(cue) = this.doc.cues.iter_mut().find(|c| c.id == cid)
                {
                    cue.start_ms = ms;
                    cx.emit(SubtitleViewEvent::Changed);
                    cx.notify();
                }
            }
        });
        let s2 = cx.subscribe(&edit_end, |this, inp, event, cx| {
            if !this.syncing_editor && matches!(event, InputEvent::Change) {
                let raw = inp.read(cx).value().to_string();
                if let Some(ms) = parse_timestamp_ms(&raw)
                    && let Some(cid) = this.selected_cue_id
                    && let Some(cue) = this.doc.cues.iter_mut().find(|c| c.id == cid)
                {
                    cue.end_ms = ms;
                    cx.emit(SubtitleViewEvent::Changed);
                    cx.notify();
                }
            }
        });
        let s3 = cx.subscribe(&edit_text, |this, ed, event, cx| {
            if !this.syncing_editor && matches!(event, InputEvent::Change) {
                let txt = ed.read(cx).value().to_string();
                if let Some(cid) = this.selected_cue_id
                    && let Some(cue) = this.doc.cues.iter_mut().find(|c| c.id == cid)
                {
                    cue.text = txt;
                    cx.emit(SubtitleViewEvent::Changed);
                    cx.notify();
                }
            }
        });

        let source_editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("text")
                .soft_wrap(true)
                .line_number(true)
                .default_value(raw)
        });
        let source_sub = cx.subscribe(&source_editor, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) && this.source_mode {
                cx.emit(SubtitleViewEvent::Changed);
            }
        });

        let mut view = Self {
            doc,
            focus_handle: cx.focus_handle(),
            filter_input,
            _filter_sub: filter_sub,
            selected_cue_id: first_id,
            edit_start,
            edit_end,
            edit_text,
            _edit_subs: [s1, s2, s3],
            syncing_editor: false,
            source_mode: false,
            source_editor,
            _source_sub: source_sub,
        };
        if let Some(fid) = first_id {
            view.select_cue(fid, window, cx);
        }
        view
    }

    pub fn select_cue(&mut self, cue_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_cue_id = Some(cue_id);
        let fmt = self.doc.format;
        if let Some(cue) = self.doc.cues.iter().find(|c| c.id == cue_id) {
            let start_s = format_timestamp_ms(cue.start_ms, fmt);
            let end_s = format_timestamp_ms(cue.end_ms, fmt);
            let txt = cue.text.clone();
            self.syncing_editor = true;
            self.edit_start
                .update(cx, |s, cx| s.set_value(start_s, window, cx));
            self.edit_end
                .update(cx, |s, cx| s.set_value(end_s, window, cx));
            self.edit_text
                .update(cx, |s, cx| s.set_value(txt, window, cx));
            self.syncing_editor = false;
        }
        cx.notify();
    }

    pub fn shift_all(&mut self, delta_ms: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.doc.shift_all(delta_ms);
        if let Some(cid) = self.selected_cue_id {
            self.select_cue(cid, window, cx);
        }
        cx.emit(SubtitleViewEvent::Changed);
        cx.notify();
    }

    pub fn add_cue(&mut self, after_id: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        let new_id = self.doc.insert_cue_after(after_id);
        self.select_cue(new_id, window, cx);
        cx.emit(SubtitleViewEvent::Changed);
    }

    pub fn remove_cue(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.doc.remove_cue(id);
        if self.selected_cue_id == Some(id) {
            self.selected_cue_id = self.doc.cues.first().map(|c| c.id);
            if let Some(next_id) = self.selected_cue_id {
                self.select_cue(next_id, window, cx);
            }
        }
        cx.emit(SubtitleViewEvent::Changed);
        cx.notify();
    }

    pub fn toggle_source_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let default_fmt = self.doc.format;
        if self.source_mode {
            let raw = self.source_editor.read(cx).value().to_string();
            self.doc = SubtitleDoc::parse(&raw, default_fmt);
            self.selected_cue_id = self.doc.cues.first().map(|c| c.id);
            if let Some(cid) = self.selected_cue_id {
                self.select_cue(cid, window, cx);
            }
            self.source_mode = false;
            self.focus_handle.focus(window, cx);
        } else {
            let txt = self.doc.to_text();
            self.source_editor
                .update(cx, |s, cx| s.set_value(txt, window, cx));
            self.source_mode = true;
            self.source_editor.focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    pub fn to_text(&self, cx: &App) -> String {
        if self.source_mode {
            self.source_editor.read(cx).value().to_string()
        } else {
            self.doc.to_text()
        }
    }

    pub fn summary(&self) -> String {
        let label = match self.doc.format {
            SubtitleFormat::Srt => "SRT",
            SubtitleFormat::Vtt => "VTT",
        };
        let dur = format_duration_short(self.doc.total_duration_ms());
        format!("{label} · {} cues · {dur}", self.doc.cues.len())
    }
}

impl Render for SubtitleView {
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

        let fmt = self.doc.format;
        let query = self.filter_input.read(cx).value().trim().to_ascii_lowercase();
        let selected_id = self.selected_cue_id;

        let cue_rows: Vec<_> = self
            .doc
            .cues
            .iter()
            .enumerate()
            .filter(|(_, cue)| {
                query.is_empty()
                    || cue.text.to_ascii_lowercase().contains(&query)
                    || format_timestamp_ms(cue.start_ms, fmt).contains(&query)
            })
            .map(|(ix, cue)| {
                let cid = cue.id;
                let is_sel = selected_id == Some(cid);
                let overlap = self.doc.cue_overlaps_prev(ix);
                let cps = cue.cps();
                let dur_s = cue.duration_ms() as f32 / 1000.0;
                let start_str = format_timestamp_ms(cue.start_ms, fmt);
                let end_str = format_timestamp_ms(cue.end_ms, fmt);
                let cps_color = if cps > 21.0 {
                    kd::RED
                } else if cps > 17.0 {
                    kd::YELLOW
                } else {
                    kd::GREEN
                };

                div()
                    .id(SharedString::from(format!("sub-cue-{cid}")))
                    .w_full()
                    .px_3()
                    .py_2()
                    .rounded(px(6.))
                    .bg(if is_sel {
                        accent_alpha(0x24)
                    } else {
                        hex_color(kd::BG_SIDEBAR)
                    })
                    .border_1()
                    .border_color(if is_sel {
                        hex_color(kd::ACCENT)
                    } else if overlap {
                        hex_color(kd::RED)
                    } else {
                        hex_color(kd::BORDER_SUBTLE)
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_cue(cid, window, cx);
                    }))
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_2()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded(px(4.))
                                            .bg(hex_color(kd::BG_DEEP))
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(hex_color(kd::FG_MUTED))
                                            .child(format!("#{}", ix + 1)),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(hex_color(kd::AQUA))
                                            .child(format!("{start_str} -> {end_str}")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(hex_color(kd::FG_MUTED))
                                            .child(format!("{dur_s:.2}s")),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .when(overlap, |r| {
                                        r.child(
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded(px(4.))
                                                .bg(hex_alpha(0xc4746e33))
                                                .text_size(px(10.))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(hex_color(kd::RED))
                                                .child("OVERLAP"),
                                        )
                                    })
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded(px(4.))
                                            .bg(hex_alpha((cps_color << 8) | 0x26))
                                            .text_size(px(10.))
                                            .text_color(hex_color(cps_color))
                                            .child(format!("{cps:.1} cps")),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("sub-add-{cid}")))
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Plus)
                                            .tooltip("Insert cue after")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                cx.stop_propagation();
                                                this.add_cue(Some(cid), window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("sub-del-{cid}")))
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Trash)
                                            .tooltip("Delete cue")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                cx.stop_propagation();
                                                this.remove_cue(cid, window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .mt_1p5()
                            .text_sm()
                            .text_color(hex_color(kd::FG))
                            .child(cue.text.clone()),
                    )
            })
            .collect();

        v_flex()
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .track_focus(&self.focus_handle)
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(hex_color(kd::BORDER_SUBTLE))
                    .child(
                        div()
                            .flex_1()
                            .max_w(px(320.))
                            .child(Input::new(&self.filter_input).xsmall()),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("sub-list-{:?}", cx.entity_id())))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_3()
                    .child(if cue_rows.is_empty() {
                        div()
                            .py_8()
                            .text_xs()
                            .text_color(hex_color(kd::FG_MUTED))
                            .child("No subtitle cues match the current filter.")
                            .into_any_element()
                    } else {
                        v_flex().gap_2().children(cue_rows).into_any_element()
                    }),
            )
            .when(self.selected_cue_id.is_some(), |this| {
                this.child(
                    v_flex()
                        .p_3()
                        .gap_2()
                        .bg(hex_color(kd::BG_SIDEBAR))
                        .border_t_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .child("Start"),
                                )
                                .child(div().w(px(150.)).child(Input::new(&self.edit_start).xsmall()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .child("End"),
                                )
                                .child(div().w(px(150.)).child(Input::new(&self.edit_end).xsmall())),
                        )
                        .child(
                            div()
                                .h(px(76.))
                                .w_full()
                                .rounded(px(6.))
                                .bg(hex_color(kd::BG_DEEP))
                                .border_1()
                                .border_color(hex_color(kd::BORDER_SUBTLE))
                                .child(
                                    Editor::new(&self.edit_text)
                                        .size_full()
                                        .appearance(false)
                                        .bordered(false),
                                ),
                        ),
                )
            })
            .into_any_element()
    }
}

pub fn render_subtitle_tab(
    app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Subtitle { view, .. } = &tab.kind else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let dirty = tab.dirty;
    let zen_mode = app.zen_mode;
    let sv = view.read(cx);
    let summary = sv.summary();
    let source_mode = sv.source_mode;

    let v_m500 = view.clone();
    let v_m100 = view.clone();
    let v_p100 = view.clone();
    let v_p500 = view.clone();
    let v_add = view.clone();
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
                        .text_color(hex_color(kd::ORANGE))
                        .child(summary),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .when(!source_mode, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("sub-m500-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .label("-500ms")
                            .on_click(cx.listener(move |_, _, window, cx| {
                                v_m500.update(cx, |v, cx| v.shift_all(-500, window, cx));
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("sub-m100-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .label("-100ms")
                            .on_click(cx.listener(move |_, _, window, cx| {
                                v_m100.update(cx, |v, cx| v.shift_all(-100, window, cx));
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("sub-p100-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .label("+100ms")
                            .on_click(cx.listener(move |_, _, window, cx| {
                                v_p100.update(cx, |v, cx| v.shift_all(100, window, cx));
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("sub-p500-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .label("+500ms")
                            .on_click(cx.listener(move |_, _, window, cx| {
                                v_p500.update(cx, |v, cx| v.shift_all(500, window, cx));
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("sub-new-{tab_id}")))
                            .secondary()
                            .xsmall()
                            .icon(IconName::Plus)
                            .label("Cue")
                            .on_click(cx.listener(move |_, _, window, cx| {
                                v_add.update(cx, |v, cx| v.add_cue(None, window, cx));
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("sub-src-{tab_id}")))
                        .with_variant(if source_mode {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Code)
                        .label(if source_mode { "Cues" } else { "Source" })
                        .on_click(cx.listener(move |_, _, window, cx| {
                            v_src.update(cx, |v, cx| v.toggle_source_mode(window, cx));
                        })),
                )
                .when(dirty, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("sub-save-{tab_id}")))
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
        .bg(hex_color(kd::BG_MAIN))
        .when(!zen_mode, |this| this.child(header))
        .child(div().flex_1().min_h_0().w_full().child(view.clone()))
        .into_any_element()
}
