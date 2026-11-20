use super::{PaneSide, TabKind, VKarmaApp};
use crate::config::MdViewMode;
use crate::markdown::{
    preprocess_markdown_with_options, resolve_inline_image_source, split_markdown_chapters,
};
use crate::theme::{hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::Editor;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder as _, px, relative,
};

pub fn render_markdown_tab(
    app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Markdown {
        editor,
        view_mode,
        source_mode,
        preprocessed,
        word_count,
        char_count,
        is_mdx,
        chapter_mode,
        active_chapter,
        ..
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let dirty = tab.dirty;
    let view_mode = *view_mode;
    let source_mode = *source_mode;
    let wc = *word_count;
    let cc = *char_count;
    let is_mdx = *is_mdx;
    let chapter_mode = *chapter_mode;
    let read_mins = if wc == 0 { 0 } else { (wc + 219) / 220 };
    let preprocessed_str = preprocessed.clone();

    let raw_text = editor.read(cx).value();
    let chapters = split_markdown_chapters(raw_text.as_ref());
    let has_chapters = chapters.len() >= 2;
    let (effective_preview, chapter_nav) = if chapter_mode && has_chapters && view_mode != MdViewMode::Edit
    {
        let idx = (*active_chapter).min(chapters.len().saturating_sub(1));
        let ch = &chapters[idx];
        let ch_pre: SharedString =
            preprocess_markdown_with_options(&ch.content, &tab.path, &app.vault, 0, is_mdx)
                .into();
        (ch_pre, Some((idx, chapters.len(), ch.title.clone())))
    } else {
        (preprocessed_str, None)
    };

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
                .when(is_mdx, |this| {
                    this.child(
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded(px(4.))
                            .bg(hex_color(kd::BG_DEEP))
                            .border_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .text_xs()
                            .text_color(hex_color(kd::ORANGE))
                            .child("MDX"),
                    )
                })
                .child(
                    div()
                        .text_xs()
                        .text_color(hex_color(kd::FG_MUTED))
                        .child(format!("{wc} words · ~{read_mins} min read · {cc} chars")),
                ),
        )
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .flex_shrink_0()
                .when(has_chapters && view_mode != MdViewMode::Edit, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("md-chapters-toggle-{tab_id}")))
                            .with_variant(if chapter_mode {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .xsmall()
                            .icon(IconName::BookOpen)
                            .label(if chapter_mode { "Chapters" } else { "Scroll" })
                            .tooltip("Toggle Chapter Pagination vs Continuous Scroll")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(tab) = this.find_tab_mut(side, tab_id)
                                    && let TabKind::Markdown {
                                        chapter_mode,
                                        active_chapter,
                                        ..
                                    } = &mut tab.kind
                                {
                                    *chapter_mode = !*chapter_mode;
                                    *active_chapter = 0;
                                    cx.notify();
                                }
                            })),
                    )
                })
                .child(
                    h_flex()
                        .p_0p5()
                        .rounded(px(6.))
                        .bg(hex_color(kd::BG_DEEP))
                        .border_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .gap_0p5()
                        .child(mode_pill_btn(
                            format!("md-edit-{tab_id}"),
                            "Edit",
                            IconName::Pencil,
                            view_mode == MdViewMode::Edit,
                            side,
                            tab_id,
                            MdViewMode::Edit,
                            cx,
                        ))
                        .child(mode_pill_btn(
                            format!("md-split-{tab_id}"),
                            "Split",
                            IconName::Split,
                            view_mode == MdViewMode::Split,
                            side,
                            tab_id,
                            MdViewMode::Split,
                            cx,
                        ))
                        .child(mode_pill_btn(
                            format!("md-read-{tab_id}"),
                            "Read",
                            IconName::BookOpen,
                            view_mode == MdViewMode::Reading,
                            side,
                            tab_id,
                            MdViewMode::Reading,
                            cx,
                        )),
                )
                .when(view_mode != MdViewMode::Reading, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("md-source-toggle-{tab_id}")))
                            .with_variant(if source_mode {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .xsmall()
                            .icon(IconName::Code)
                            .label("Source")
                            .tooltip("Toggle Raw Source Mode vs Live Preview (Ctrl+M)")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.active_pane = side;
                                if let Some(p) = this.pane_mut(side)
                                    && let Some(pos) = p.tabs.iter().position(|t| t.id == tab_id)
                                {
                                    p.active_tab = pos;
                                }
                                this.toggle_md_source_mode(window, cx);
                            })),
                    )
                })
                .when(dirty, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("md-save-{tab_id}")))
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

    let app_entity = cx.entity();
    let extensions = app.markdown_extensions.clone();
    let body = match view_mode {
        MdViewMode::Edit => {
            if source_mode {
                render_md_editor_pane(app_entity, side, tab_id, editor, source_mode, cx)
                    .into_any_element()
            } else {
                super::live_preview_view::render_md_live_preview_pane(app, side, tab, true, cx)
            }
        }
        MdViewMode::Reading => render_md_preview_pane(
            app_entity,
            side,
            tab_id,
            effective_preview,
            extensions,
            true,
            chapter_nav,
            cx,
        )
        .into_any_element(),
        MdViewMode::Split => {
            let edit_el = if source_mode {
                render_md_editor_pane(
                    app_entity.clone(),
                    side,
                    tab_id,
                    editor,
                    source_mode,
                    cx,
                )
                .into_any_element()
            } else {
                super::live_preview_view::render_md_live_preview_pane(app, side, tab, false, cx)
            };
            let preview_el = render_md_preview_pane(
                app_entity,
                side,
                tab_id,
                effective_preview,
                extensions,
                false,
                chapter_nav,
                cx,
            );
            h_resizable(SharedString::from(format!("md-split-{side:?}-{tab_id}")))
                .child(
                    resizable_panel()
                        .size_range(px(220.)..px(2000.))
                        .child(edit_el),
                )
                .child(
                    resizable_panel()
                        .size_range(px(220.)..px(2000.))
                        .child(preview_el),
                )
                .into_any_element()
        }
    };

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .child(header)
        .child(div().flex_1().min_h_0().w_full().child(body))
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn mode_pill_btn(
    id: String,
    label: &'static str,
    icon: IconName,
    active: bool,
    side: PaneSide,
    tab_id: u64,
    target_mode: MdViewMode,
    cx: &mut Context<VKarmaApp>,
) -> Button {
    Button::new(SharedString::from(id))
        .with_variant(if active {
            ButtonVariant::Secondary
        } else {
            ButtonVariant::Ghost
        })
        .xsmall()
        .icon(icon)
        .label(label)
        .on_click(cx.listener(move |this, _, window, cx| {
            this.active_pane = side;
            this.commit_active_live_block(side, tab_id, window, cx);
            if let Some(tab) = this.find_tab_mut(side, tab_id)
                && let TabKind::Markdown { view_mode, .. } = &mut tab.kind
            {
                *view_mode = target_mode;
                cx.notify();
            }
        }))
}

fn render_md_editor_pane(
    app_entity: gpui_kit::Entity<VKarmaApp>,
    side: PaneSide,
    tab_id: u64,
    editor: &gpui_kit::Entity<gpui_kit::component::input::EditorState>,
    source_mode: bool,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement + use<> {
    let paste_app = app_entity;
    let ed = Editor::new(editor)
        .size_full()
        .appearance(false)
        .bordered(false)
        .when(!source_mode, |this| {
            this.font_family(cx.theme().font_family.clone())
                .text_size(px(15.))
                .line_height(relative(1.65))
        })
        .when(source_mode, |this| {
            this.text_size(px(13.5)).line_height(relative(1.5))
        })
        .on_paste(move |item, window, cx| {
            paste_app.update(cx, |app, cx| {
                app.handle_markdown_image_paste(side, tab_id, item, window, cx)
            })
        });

    div()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .child(ed)
}

#[allow(clippy::too_many_arguments)]
fn render_md_preview_pane(
    app_entity: gpui_kit::Entity<VKarmaApp>,
    side: PaneSide,
    tab_id: u64,
    preprocessed: SharedString,
    extensions: gpui_kit::component::text::MarkdownExtensions,
    centered_read: bool,
    chapter_nav: Option<(usize, usize, String)>,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let app_for_links = app_entity.clone();
    let app_for_tasks = app_entity;

    let ch_suffix = chapter_nav
        .as_ref()
        .map(|(idx, _, _)| format!("-ch{idx}"))
        .unwrap_or_default();
    let tv_id: SharedString = format!("md-preview-{side:?}-{tab_id}{ch_suffix}").into();

    let text_view = TextView::markdown(tv_id, preprocessed)
        .markdown_extensions(extensions)
        .image_source(resolve_inline_image_source)
        .selectable(true)
        .on_link_click(move |url, _, window, cx| {
            app_for_links.update(cx, |app, cx| {
                app.handle_markdown_link(tab_id, side, url, window, cx);
            });
        })
        .on_task_toggle(move |ev, window, cx| {
            app_for_tasks.update(cx, |app, cx| {
                app.handle_task_toggle(tab_id, side, ev, window, cx);
            });
        })
        .w_full()
        .text_size(px(15.))
        .line_height(relative(1.65))
        .text_color(hex_color(kd::FG));

    let scroll = div()
        .id(SharedString::from(format!("md-preview-scroll-{side:?}-{tab_id}{ch_suffix}")))
        .flex_1()
        .min_h_0()
        .w_full()
        .bg(hex_color(kd::BG_MAIN))
        .overflow_y_scrollbar()
        .px(px(if centered_read { 32. } else { 24. }))
        .py(px(20.))
        .child(
            div()
                .w_full()
                .when(centered_read, |d| d.max_w(px(820.)).mx_auto())
                .child(text_view),
        );

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .child(scroll)
        .when_some(chapter_nav, |this, (idx, total, title)| {
            let can_prev = idx > 0;
            let can_next = idx + 1 < total;
            this.child(
                h_flex()
                    .h(px(40.))
                    .w_full()
                    .px_4()
                    .items_center()
                    .justify_between()
                    .bg(hex_color(kd::BG_SIDEBAR))
                    .border_t_1()
                    .border_color(hex_color(kd::BORDER_SUBTLE))
                    .child(
                        Button::new(SharedString::from(format!("md-ch-prev-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .label("← Prev")
                            .disabled(!can_prev)
                            .on_click(cx.listener(move |app, _, _, cx| {
                                if let Some(tab) = app.find_tab_mut(side, tab_id)
                                    && let TabKind::Markdown { active_chapter, .. } = &mut tab.kind
                                    && *active_chapter > 0
                                {
                                    *active_chapter -= 1;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .px_2()
                            .text_xs()
                            .text_color(hex_color(kd::FG_SUBTLE))
                            .truncate()
                            .child(format!("Chapter {} / {} · {title}", idx + 1, total)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("md-ch-next-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .label("Next →")
                            .disabled(!can_next)
                            .on_click(cx.listener(move |app, _, _, cx| {
                                if let Some(tab) = app.find_tab_mut(side, tab_id)
                                    && let TabKind::Markdown { active_chapter, .. } = &mut tab.kind
                                {
                                    *active_chapter = (*active_chapter + 1).min(total.saturating_sub(1));
                                    cx.notify();
                                }
                            })),
                    ),
            )
        })
}
