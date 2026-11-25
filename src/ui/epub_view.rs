use super::find_bar::{FIND_INPUT_W, find_count_badge, floating_find_card};
use super::{TabKind, VKarmaApp};
use crate::formats::epub::{EpubBlock, EpubBook, EpubRun};
use crate::theme::{hex_alpha, hex_color, kd};
use crate::vault::{OutlineHeading, find_all_byte_ranges};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable, FontStyle,
    FontWeight, HighlightStyle, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent,
    MouseButton, ObjectFit, ParentElement, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled, StyledImage as _, StyledText, Subscription,
    UnderlineStyle, Window, div, img, point, prelude::FluentBuilder as _, px,
};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

fn clamp_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn build_block_highlights(
    text: &str,
    runs: &[EpubRun],
    matches: &[Range<usize>],
    active_match_rel: Option<usize>,
    code_bg: Hsla,
    code_fg: Hsla,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut pts = Vec::with_capacity((runs.len() + matches.len() + 1) * 2);
    let mut push_range = |r: &Range<usize>| {
        let a = clamp_char_boundary(text, r.start);
        let b = clamp_char_boundary(text, r.end);
        if a < b {
            pts.push(a);
            pts.push(b);
        }
    };
    for r in runs {
        push_range(&r.range);
    }
    for m in matches {
        push_range(m);
    }
    if pts.len() < 2 {
        return Vec::new();
    }
    pts.sort_unstable();
    pts.dedup();

    let mut out: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b {
            continue;
        }
        let base_run = runs.iter().find(|r| {
            let rs = clamp_char_boundary(text, r.range.start);
            let re = clamp_char_boundary(text, r.range.end);
            rs <= a && re >= b
        });
        let match_ix = matches.iter().position(|r| {
            let rs = clamp_char_boundary(text, r.start);
            let re = clamp_char_boundary(text, r.end);
            rs <= a && re >= b
        });
        if base_run.is_none() && match_ix.is_none() {
            continue;
        }
        let mut style = HighlightStyle::default();
        if let Some(run) = base_run {
            if run.code {
                style.color = Some(code_fg);
                style.background_color = Some(code_bg);
            }
            if run.bold {
                style.font_weight = Some(FontWeight::BOLD);
            }
            if run.italic {
                style.font_style = Some(FontStyle::Italic);
            }
            if run.underline {
                style.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: None,
                    wavy: false,
                });
            }
        }
        if let Some(ix) = match_ix {
            style.background_color = Some(if active_match_rel == Some(ix) {
                hex_alpha(0xe8a84999)
            } else {
                hex_alpha(0xc4b28a52)
            });
        }
        if let Some((last_r, last_s)) = out.last_mut()
            && last_r.end == a
            && *last_s == style
        {
            last_r.end = b;
        } else {
            out.push((a..b, style));
        }
    }
    out
}

pub struct EpubView {
    pub path: PathBuf,
    pub book: EpubBook,
    pub chapter_idx: usize,
    pub font_size: f32,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    find_open: bool,
    find_case_sensitive: bool,
    find_input: Entity<InputState>,
    _find_sub: Subscription,
    find_query: String,
    find_matches: Vec<(usize, usize, Range<usize>)>,
    find_active: usize,
    find_needs_focus: bool,
}

impl Focusable for EpubView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn block_plain_text(blk: &EpubBlock) -> Option<&str> {
    match blk {
        EpubBlock::Heading { text, .. }
        | EpubBlock::Paragraph { text, .. }
        | EpubBlock::ListItem { text, .. } => Some(text.as_str()),
        EpubBlock::Image { .. } | EpubBlock::Rule => None,
    }
}

impl EpubView {
    pub fn new(
        path: PathBuf,
        book: EpubBook,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in EPUB…"));
        let _find_sub = cx.subscribe(&find_input, |this, state, event, cx| match event {
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
        });

        Self {
            path,
            book,
            chapter_idx: 0,
            font_size: 16.5,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            find_open: false,
            find_case_sensitive: false,
            find_input,
            _find_sub,
            find_query: String::new(),
            find_matches: Vec::new(),
            find_active: 0,
            find_needs_focus: false,
        }
    }

    pub fn relocate_book(&mut self, new_path: PathBuf) {
        self.path = new_path;
    }

    pub fn outline_headings(&self) -> Vec<OutlineHeading> {
        self.book.outline_headings()
    }

    pub fn go_to_chapter(&mut self, idx: usize, cx: &mut Context<Self>) {
        if self.book.chapters.is_empty() {
            return;
        }
        self.chapter_idx = idx.min(self.book.chapters.len() - 1);
        self.scroll_handle.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    pub fn next_chapter(&mut self, cx: &mut Context<Self>) {
        if self.chapter_idx + 1 < self.book.chapters.len() {
            self.go_to_chapter(self.chapter_idx + 1, cx);
        }
    }

    pub fn prev_chapter(&mut self, cx: &mut Context<Self>) {
        if self.chapter_idx > 0 {
            self.go_to_chapter(self.chapter_idx - 1, cx);
        }
    }

    pub fn set_font_size(&mut self, next: f32, cx: &mut Context<Self>) {
        self.font_size = next.clamp(12.0, 30.0);
        cx.notify();
    }

    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = true;
        self.find_needs_focus = true;
        let query_len = self.find_input.read(cx).value().len();
        self.find_input.update(cx, |s, cx| {
            s.focus(window, cx);
            if query_len > 0 {
                s.set_selected_range(0..query_len, cx);
            }
        });
        cx.notify();
    }

    pub fn close_find(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.find_open = false;
        self.find_needs_focus = false;
        if let Some(win) = window {
            self.focus_handle.focus(win, cx);
        }
        cx.notify();
    }

    pub fn is_find_open(&self) -> bool {
        self.find_open
    }

    fn clear_find_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_query.clear();
        self.find_matches.clear();
        self.find_active = 0;
        self.find_input.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    fn toggle_find_case(&mut self, cx: &mut Context<Self>) {
        self.find_case_sensitive = !self.find_case_sensitive;
        self.recompute_find(true);
        cx.notify();
    }

    fn recompute_find(&mut self, jump_to_current_chapter: bool) {
        self.find_matches.clear();
        self.find_active = 0;
        let query = self.find_query.trim();
        if query.is_empty() {
            return;
        }
        for (ch_ix, ch) in self.book.chapters.iter().enumerate() {
            for (blk_ix, blk) in ch.blocks.iter().enumerate() {
                let Some(text) = block_plain_text(blk) else {
                    continue;
                };
                for range in find_all_byte_ranges(text, query, self.find_case_sensitive) {
                    self.find_matches.push((ch_ix, blk_ix, range));
                }
            }
        }
        if !self.find_matches.is_empty() && jump_to_current_chapter {
            if let Some(pos) = self
                .find_matches
                .iter()
                .position(|(ch, _, _)| *ch >= self.chapter_idx)
            {
                self.find_active = pos;
            }
            let target_ch = self.find_matches[self.find_active].0;
            if target_ch != self.chapter_idx {
                self.chapter_idx = target_ch;
                self.scroll_handle.set_offset(point(px(0.), px(0.)));
            }
        }
    }

    fn step_find(&mut self, forward: bool) {
        if self.find_matches.is_empty() {
            return;
        }
        let n = self.find_matches.len();
        self.find_active = if forward {
            (self.find_active + 1) % n
        } else if self.find_active == 0 {
            n - 1
        } else {
            self.find_active - 1
        };
        let target_ch = self.find_matches[self.find_active].0;
        if target_ch != self.chapter_idx {
            self.chapter_idx = target_ch;
            self.scroll_handle.set_offset(point(px(0.), px(0.)));
        }
    }

    pub fn summary(&self) -> String {
        let ch_count = self.book.chapters.len().max(1);
        let cur = (self.chapter_idx + 1).min(ch_count);
        let words = self.book.total_words;
        if self.book.title.trim().is_empty() {
            format!("EPUB · Ch {cur}/{ch_count} · {words} words")
        } else {
            format!("{} · Ch {cur}/{ch_count} · {words} words", self.book.title)
        }
    }

    fn render_block_text(
        &self,
        text: &str,
        runs: &[EpubRun],
        ch_ix: usize,
        blk_ix: usize,
    ) -> AnyElement {
        let mut matches = Vec::new();
        let mut active_rel = None;
        for (ix, (mc, mb, mr)) in self.find_matches.iter().enumerate() {
            if *mc == ch_ix && *mb == blk_ix {
                if ix == self.find_active {
                    active_rel = Some(matches.len());
                }
                matches.push(mr.clone());
            }
        }
        let flat = build_block_highlights(
            text,
            runs,
            &matches,
            active_rel,
            hex_color(kd::BG_PANEL),
            hex_color(kd::AQUA),
        );
        let own = text.to_string();
        if flat.is_empty() {
            StyledText::new(own).into_any_element()
        } else {
            StyledText::new(own).with_highlights(flat).into_any_element()
        }
    }

    fn render_find_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let total = self.find_matches.len();
        let query_non_empty = !self.find_query.is_empty();
        let trimmed_non_empty = !self.find_query.trim().is_empty();
        let count_text = if !trimmed_non_empty {
            String::new()
        } else if total == 0 {
            "0 / 0".to_string()
        } else {
            let ch = self.find_matches[self.find_active].0 + 1;
            format!("{} / {} · Ch {}", self.find_active + 1, total, ch)
        };
        let clear_btn = Button::new(SharedString::from(format!(
            "epub-find-x-{:?}",
            cx.entity_id()
        )))
        .text()
        .xsmall()
        .tab_stop(false)
        .icon(IconName::Close)
        .tooltip("Clear")
        .on_click(cx.listener(|this, _, window, cx| {
            this.clear_find_query(window, cx);
        }));

        floating_find_card(SharedString::from(format!(
            "epub-find-card-{:?}",
            cx.entity_id()
        )))
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
                            .when(query_non_empty, |i| i.suffix(clear_btn)),
                    ),
                )
                .child(find_count_badge(count_text, trimmed_non_empty && total == 0))
                .child(
                    Button::new(SharedString::from(format!(
                        "epub-find-case-{:?}",
                        cx.entity_id()
                    )))
                    .with_variant(if self.find_case_sensitive {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Ghost
                    })
                    .xsmall()
                    .tab_stop(false)
                    .label("Aa")
                    .tooltip("Match Case")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_find_case(cx);
                    })),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "epub-find-prev-{:?}",
                        cx.entity_id()
                    )))
                    .ghost()
                    .xsmall()
                    .tab_stop(false)
                    .icon(IconName::ChevronUp)
                    .disabled(total == 0)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.step_find(false);
                        cx.notify();
                    })),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "epub-find-next-{:?}",
                        cx.entity_id()
                    )))
                    .ghost()
                    .xsmall()
                    .tab_stop(false)
                    .icon(IconName::ChevronDown)
                    .disabled(total == 0)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.step_find(true);
                        cx.notify();
                    })),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "epub-find-close-{:?}",
                        cx.entity_id()
                    )))
                    .ghost()
                    .xsmall()
                    .tab_stop(false)
                    .icon(IconName::Close)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_find(Some(window), cx);
                    })),
                ),
        )
        .into_any_element()
    }
}

impl Render for EpubView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.find_needs_focus {
            self.find_needs_focus = false;
            self.find_input.update(cx, |s, cx| s.focus(window, cx));
        }

        let ch_count = self.book.chapters.len();
        let cur_ix = self.chapter_idx.min(ch_count.saturating_sub(1));
        let fs = self.font_size;

        let fg = hex_color(kd::FG);
        let muted = hex_color(kd::FG_MUTED);
        let heading_fg = hex_color(kd::VIOLET);
        let border = hex_color(kd::BORDER_SUBTLE);
        let find_bar = self.find_open.then(|| self.render_find_bar(cx));

        let chapter_col = if let Some(ch) = self.book.chapters.get(cur_ix) {
            v_flex()
                .w_full()
                .max_w(px(740.))
                .mx_auto()
                .px_8()
                .py_8()
                .gap_4()
                .child(
                    v_flex()
                        .pb_3()
                        .mb_2()
                        .border_b_1()
                        .border_color(border)
                        .gap_1()
                        .child(
                            div()
                                .text_size(px((fs + 7.0).min(30.0)))
                                .font_weight(FontWeight::BOLD)
                                .font_family("serif")
                                .text_color(fg)
                                .child(ch.title.clone()),
                        )
                        .when_some(self.book.author.clone(), |col, author| {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{} · {}", self.book.title, author)),
                            )
                        }),
                )
                .children(ch.blocks.iter().enumerate().map(|(blk_ix, blk)| match blk {
                    EpubBlock::Heading { level, text } => {
                        let size = match level {
                            1 => fs + 6.0,
                            2 => fs + 4.0,
                            _ => fs + 2.0,
                        };
                        div()
                            .pt_2()
                            .text_size(px(size))
                            .font_weight(FontWeight::BOLD)
                            .text_color(heading_fg)
                            .child(self.render_block_text(text, &[], cur_ix, blk_ix))
                            .into_any_element()
                    }
                    EpubBlock::Paragraph {
                        text,
                        runs,
                        is_quote,
                    } => {
                        let p = div()
                            .text_size(px(fs))
                            .line_height(px(fs * 1.65))
                            .text_color(if *is_quote { muted } else { fg })
                            .child(self.render_block_text(text, runs, cur_ix, blk_ix));
                        if *is_quote {
                            div()
                                .pl_4()
                                .py_1()
                                .border_l_2()
                                .border_color(heading_fg)
                                .italic()
                                .child(p)
                                .into_any_element()
                        } else {
                            p.into_any_element()
                        }
                    }
                    EpubBlock::ListItem { text, runs } => h_flex()
                        .pl_4()
                        .gap_2()
                        .items_start()
                        .child(div().text_size(px(fs)).text_color(heading_fg).child("•"))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(fs))
                                .line_height(px(fs * 1.6))
                                .text_color(fg)
                                .child(self.render_block_text(text, runs, cur_ix, blk_ix)),
                        )
                        .into_any_element(),
                    EpubBlock::Image { alt, image } => v_flex()
                        .w_full()
                        .items_center()
                        .gap_1()
                        .py_2()
                        .child(
                            img(Arc::clone(image))
                                .max_w(px(620.))
                                .max_h(px(460.))
                                .object_fit(ObjectFit::Contain)
                                .rounded(px(6.)),
                        )
                        .when(!alt.is_empty(), |col| {
                            col.child(div().text_xs().text_color(muted).child(alt.clone()))
                        })
                        .into_any_element(),
                    EpubBlock::Rule => div()
                        .w_full()
                        .h(px(1.))
                        .my_3()
                        .bg(border)
                        .into_any_element(),
                }))
                .child(
                    h_flex()
                        .pt_6()
                        .mt_4()
                        .border_t_1()
                        .border_color(border)
                        .justify_between()
                        .items_center()
                        .child(
                            Button::new("epub-foot-prev")
                                .secondary()
                                .small()
                                .icon(IconName::ChevronLeft)
                                .label("Previous Chapter")
                                .disabled(cur_ix == 0)
                                .on_click(cx.listener(|this, _, _, cx| this.prev_chapter(cx))),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("Chapter {} / {}", cur_ix + 1, ch_count.max(1))),
                        )
                        .child(
                            Button::new("epub-foot-next")
                                .secondary()
                                .small()
                                .icon(IconName::ChevronRight)
                                .label("Next Chapter")
                                .disabled(cur_ix + 1 >= ch_count)
                                .on_click(cx.listener(|this, _, _, cx| this.next_chapter(cx))),
                        ),
                )
        } else {
            v_flex()
                .w_full()
                .p_8()
                .items_center()
                .child(div().text_sm().text_color(muted).child("Empty EPUB file."))
        };

        div()
            .id(SharedString::from(format!("epub-view-{:?}", cx.entity_id())))
            .relative()
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if !this.find_open {
                    match ev.keystroke.key.as_str() {
                        "left" => this.prev_chapter(cx),
                        "right" => this.next_chapter(cx),
                        "escape" => this.close_find(Some(window), cx),
                        _ => {}
                    }
                }
            }))
            .child(
                div()
                    .id("epub-preview-scroll")
                    .track_scroll(&self.scroll_handle)
                    .size_full()
                    .overflow_y_scrollbar()
                    .child(chapter_col),
            )
            .when_some(find_bar, |root, bar| root.child(bar))
            .into_any_element()
    }
}

pub fn render_epub_tab(
    app: &VKarmaApp,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Epub {
        view,
        file_size_bytes,
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let kb = *file_size_bytes as f64 / 1024.0;
    let zen_mode = app.zen_mode;
    let ev = view.read(cx);
    let summary = ev.summary();
    let cur_ch = ev.chapter_idx;
    let total_ch = ev.book.chapters.len();
    let find_open = ev.find_open;
    let fs = ev.font_size;

    let v_find = view.clone();
    let v_prev = view.clone();
    let v_next = view.clone();
    let v_sm = view.clone();
    let v_lg = view.clone();

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
                .flex_1()
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
                        .truncate()
                        .child(format!("{summary} · {kb:.0} KB · Read-only preview")),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new(SharedString::from(format!("epub-find-{tab_id}")))
                        .with_variant(if find_open {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Search)
                        .tooltip("Find in EPUB (Ctrl+F)")
                        .on_click(cx.listener(move |_, _, window, cx| {
                            v_find.update(cx, |v, cx| v.open_find(window, cx));
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("epub-prev-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::ChevronLeft)
                        .disabled(cur_ch == 0)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_prev.update(cx, |v, cx| v.prev_chapter(cx));
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("epub-next-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::ChevronRight)
                        .disabled(cur_ch + 1 >= total_ch)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_next.update(cx, |v, cx| v.next_chapter(cx));
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("epub-sm-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .label("A-")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_sm.update(cx, |v, cx| v.set_font_size(v.font_size - 1.0, cx));
                        })),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(hex_color(kd::FG_MUTED))
                        .child(format!("{fs:.0}px")),
                )
                .child(
                    Button::new(SharedString::from(format!("epub-lg-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .label("A+")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_lg.update(cx, |v, cx| v.set_font_size(v.font_size + 1.0, cx));
                        })),
                ),
        );

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_DEEP))
        .when(!zen_mode, |this| this.child(header))
        .child(div().flex_1().min_h_0().w_full().child(view.clone()))
        .into_any_element()
}
