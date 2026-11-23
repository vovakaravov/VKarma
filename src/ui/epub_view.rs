use super::{TabKind, VKarmaApp};
use crate::formats::epub::{EpubBlock, EpubBook};
use crate::theme::{hex_color, kd};
use crate::vault::OutlineHeading;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, FocusHandle, Focusable, FontWeight, InteractiveElement as _,
    IntoElement, KeyDownEvent, ObjectFit, ParentElement, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled, StyledImage as _, Window, div, img, point,
    prelude::FluentBuilder as _, px,
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpubTheme {
    Dark,
    Sepia,
    Light,
}

impl EpubTheme {
    fn next(self) -> Self {
        match self {
            Self::Dark => Self::Sepia,
            Self::Sepia => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Sepia => "Sepia",
            Self::Light => "Light",
        }
    }
}

pub struct EpubView {
    pub book: EpubBook,
    pub chapter_idx: usize,
    pub font_size: f32,
    pub theme: EpubTheme,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
}

impl Focusable for EpubView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EpubView {
    pub fn new(book: EpubBook, cx: &mut Context<Self>) -> Self {
        Self {
            book,
            chapter_idx: 0,
            font_size: 16.0,
            theme: EpubTheme::Dark,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
        }
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

    pub fn outline_headings(&self) -> Vec<OutlineHeading> {
        self.book.outline_headings()
    }

    pub fn summary(&self) -> String {
        format!(
            "Ch {}/{} · {} words",
            self.chapter_idx + 1,
            self.book.chapters.len().max(1),
            self.book.total_words
        )
    }
}

impl Render for EpubView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (outer_bg, page_bg, text_fg, muted_fg, heading_fg, border_c) = match self.theme {
            EpubTheme::Dark => (
                hex_color(kd::BG_DEEP),
                hex_color(kd::BG_MAIN),
                hex_color(kd::FG),
                hex_color(kd::FG_MUTED),
                hex_color(kd::ACCENT),
                hex_color(kd::BORDER_SUBTLE),
            ),
            EpubTheme::Sepia => (
                hex_color(0x1f1c18),
                hex_color(0xf4ecd8),
                hex_color(0x3b2f1e),
                hex_color(0x73634e),
                hex_color(0x8a4b25),
                hex_color(0xd8cbb2),
            ),
            EpubTheme::Light => (
                hex_color(0xe5e3dc),
                hex_color(0xfaf9f5),
                hex_color(0x1f2328),
                hex_color(0x656d76),
                hex_color(0x2d5a7b),
                hex_color(0xd8d5cc),
            ),
        };

        let ch_count = self.book.chapters.len();
        let cur_ix = self.chapter_idx.min(ch_count.saturating_sub(1));
        let fs = self.font_size;

        let chapter_blocks: Vec<AnyElement> = if let Some(ch) = self.book.chapters.get(cur_ix) {
            ch.blocks
                .iter()
                .map(|blk| match blk {
                    EpubBlock::Heading { level, text } => {
                        let scale = match level {
                            1 => 1.55,
                            2 => 1.32,
                            3 => 1.16,
                            _ => 1.05,
                        };
                        div()
                            .pt_3()
                            .text_size(px(fs * scale))
                            .font_weight(FontWeight::BOLD)
                            .text_color(heading_fg)
                            .child(text.clone())
                            .into_any_element()
                    }
                    EpubBlock::Paragraph { text, is_quote } => {
                        let p = div()
                            .text_size(px(fs))
                            .line_height(px(fs * 1.65))
                            .text_color(if *is_quote { muted_fg } else { text_fg })
                            .child(text.clone());
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
                    EpubBlock::ListItem(text) => h_flex()
                        .pl_4()
                        .gap_2()
                        .items_start()
                        .child(
                            div()
                                .text_size(px(fs))
                                .text_color(heading_fg)
                                .child("•"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(fs))
                                .line_height(px(fs * 1.6))
                                .text_color(text_fg)
                                .child(text.clone()),
                        )
                        .into_any_element(),
                    EpubBlock::Image { alt, image } => v_flex()
                        .w_full()
                        .items_center()
                        .gap_1()
                        .py_2()
                        .child(
                            img(Arc::clone(image))
                                .max_w(px(616.))
                                .max_h(px(460.))
                                .object_fit(ObjectFit::Contain)
                                .rounded(px(6.)),
                        )
                        .when(!alt.is_empty(), |col| {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(muted_fg)
                                    .child(alt.clone()),
                            )
                        })
                        .into_any_element(),
                    EpubBlock::Rule => div()
                        .w_full()
                        .h(px(1.))
                        .my_3()
                        .bg(border_c)
                        .into_any_element(),
                })
                .collect()
        } else {
            vec![
                div()
                    .text_sm()
                    .text_color(muted_fg)
                    .child("Empty EPUB book.")
                    .into_any_element(),
            ]
        };

        let ch_title = self
            .book
            .chapters
            .get(cur_ix)
            .map(|c| c.title.clone())
            .unwrap_or_default();
        let author_line = self
            .book
            .author
            .as_ref()
            .map(|a| format!("{} — {a}", self.book.title))
            .unwrap_or_else(|| self.book.title.clone());

        div()
            .id(SharedString::from(format!("epub-view-{:?}", cx.entity_id())))
            .size_full()
            .bg(outer_bg)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                match ev.keystroke.key.as_str() {
                    "left" => {
                        cx.stop_propagation();
                        this.prev_chapter(cx);
                    }
                    "right" => {
                        cx.stop_propagation();
                        this.next_chapter(cx);
                    }
                    _ => {}
                }
            }))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "epub-ch-scroll-{:?}",
                        cx.entity_id()
                    )))
                    .track_scroll(&self.scroll_handle)
                    .size_full()
                    .overflow_y_scrollbar()
                    .py_6()
                    .px_6()
                    .flex()
                    .justify_center()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(680.))
                            .p_8()
                            .gap_3p5()
                            .rounded(px(10.))
                            .bg(page_bg)
                            .border_1()
                            .border_color(border_c)
                            .child(
                                h_flex()
                                    .pb_3()
                                    .border_b_1()
                                    .border_color(border_c)
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted_fg)
                                            .truncate()
                                            .child(author_line),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .italic()
                                            .text_color(muted_fg)
                                            .child(ch_title),
                                    ),
                            )
                            .children(chapter_blocks)
                            .child(
                                h_flex()
                                    .pt_5()
                                    .mt_4()
                                    .border_t_1()
                                    .border_color(border_c)
                                    .justify_between()
                                    .child(
                                        Button::new("epub-foot-prev")
                                            .ghost()
                                            .small()
                                            .icon(IconName::ChevronLeft)
                                            .label("Previous Chapter")
                                            .disabled(cur_ix == 0)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.prev_chapter(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("epub-foot-next")
                                            .ghost()
                                            .small()
                                            .icon(IconName::ChevronRight)
                                            .label("Next Chapter")
                                            .disabled(cur_ix + 1 >= ch_count)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.next_chapter(cx);
                                            })),
                                    ),
                            ),
                    ),
            )
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
    let zen_mode = app.zen_mode;
    let kb = *file_size_bytes as f64 / 1024.0;
    let ev = view.read(cx);
    let summary = ev.summary();
    let theme_label = ev.theme.label();
    let cur_ch = ev.chapter_idx;
    let total_ch = ev.book.chapters.len();

    let v_prev = view.clone();
    let v_next = view.clone();
    let v_sm = view.clone();
    let v_lg = view.clone();
    let v_thm = view.clone();

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
                        .child(format!("{summary} · {kb:.0} KB")),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
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
                            v_sm.update(cx, |v, cx| {
                                v.font_size = (v.font_size - 1.0).clamp(11.0, 26.0);
                                cx.notify();
                            });
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("epub-lg-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .label("A+")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_lg.update(cx, |v, cx| {
                                v.font_size = (v.font_size + 1.0).clamp(11.0, 26.0);
                                cx.notify();
                            });
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("epub-theme-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .icon(IconName::BookOpen)
                        .label(theme_label)
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_thm.update(cx, |v, cx| {
                                v.theme = v.theme.next();
                                cx.notify();
                            });
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
