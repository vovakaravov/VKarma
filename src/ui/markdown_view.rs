use super::find_bar::{
    EditorFindBar, FIND_INPUT_W, find_count_badge, floating_find_card, wrap_editor_with_find,
};
use super::live_editor::LiveMarkdownEditor;
use super::{PaneSide, TabKind, VKarmaApp};
use crate::config::MdViewMode;
use crate::markdown::split_markdown_chapters;
use crate::theme::{hex_color, kd};
use crate::vault::VaultIndex;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Editor, Input};
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, Entity, Focusable as _, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement, SharedString, Styled, div, prelude::FluentBuilder as _, px, relative,
};
use std::path::Path;
use std::sync::Arc;

pub fn render_markdown_tab(
    app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Markdown {
        editor,
        editor_find,
        live_doc,
        preview_live_doc,
        view_mode,
        source_mode,
        word_count,
        char_count,
        is_mdx,
        chapter_mode,
        active_chapter,
        read_find,
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
    let zen_mode = app.zen_mode;
    let effective_mode = if zen_mode && view_mode == MdViewMode::Split {
        MdViewMode::Edit
    } else {
        view_mode
    };
    let read_find_open = read_find.open;
    let read_find_query = read_find.query.clone();
    let read_find_case = read_find.case_sensitive;
    let read_find_input = read_find.input.clone();
    let read_find_total = read_find.matches.len();
    let read_find_active = read_find.active;
    let read_find_hit = read_find.matches.get(read_find.active).cloned();

    let raw_text: String = if source_mode
        && let Some(ed) = editor.as_ref()
    {
        ed.read(cx).value().to_string()
    } else {
        live_doc.read(cx).text().to_string()
    };
    let chapters = split_markdown_chapters(&raw_text);
    let has_chapters = chapters.len() >= 2;
    let chapter_nav = if chapter_mode && view_mode != MdViewMode::Edit && has_chapters {
        let idx = (*active_chapter).min(chapters.len().saturating_sub(1));
        let ch = &chapters[idx];
        Some((idx, chapters.len(), ch.title.clone(), ch.content.clone()))
    } else {
        None
    };

    let find_btn_active = if effective_mode == MdViewMode::Reading {
        if chapter_nav.is_some() || source_mode {
            preview_live_doc.read(cx).is_search_open()
        } else {
            live_doc.read(cx).is_search_open()
        }
    } else {
        (!source_mode && live_doc.read(cx).is_search_open())
            || (source_mode
                && editor_find
                    .as_ref()
                    .is_some_and(|fb| fb.read(cx).is_open()))
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
                .child(
                    Button::new(SharedString::from(format!("md-find-{tab_id}")))
                        .with_variant(if find_btn_active {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Search)
                        .tooltip("Find in File (Ctrl+F)")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.active_pane = side;
                            if let Some(p) = this.pane_mut(side)
                                && let Some(pos) = p.tabs.iter().position(|t| t.id == tab_id)
                            {
                                p.active_tab = pos;
                            }
                            this.trigger_find_in_active_tab(false, window, cx);
                        })),
                )
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
    let live_doc = live_doc.clone();
    let preview_live_doc = preview_live_doc.clone();
    let body = match effective_mode {
        MdViewMode::Edit => {
            if source_mode
                && let Some(ed) = editor.as_ref()
            {
                render_md_editor_pane(
                    app_entity,
                    side,
                    tab_id,
                    ed,
                    editor_find.as_ref(),
                    source_mode,
                    zen_mode,
                    cx,
                )
                .into_any_element()
            } else {
                let vault = Arc::clone(&app.vault);
                live_doc.update(cx, |ed, _| ed.sync_render_context(true, zen_mode, vault, false));
                live_doc.into_any_element()
            }
        }
        MdViewMode::Reading => {
            if let Some((idx, total, title, ch_content)) = chapter_nav {
                let vault = Arc::clone(&app.vault);
                sync_read_preview_doc(&preview_live_doc, &ch_content, true, zen_mode, vault, cx);
                wrap_with_chapter_footer(
                    preview_live_doc.into_any_element(),
                    side,
                    tab_id,
                    idx,
                    total,
                    title,
                    cx,
                )
            } else if source_mode && editor.is_some() {
                let vault = Arc::clone(&app.vault);
                sync_read_preview_doc(&preview_live_doc, &raw_text, true, zen_mode, vault, cx);
                preview_live_doc.into_any_element()
            } else {
                let vault = Arc::clone(&app.vault);
                live_doc.update(cx, |ed, _| ed.sync_render_context(true, zen_mode, vault, true));
                live_doc.into_any_element()
            }
        }
        MdViewMode::Split => {
            let edit_el = if source_mode
                && let Some(ed) = editor.as_ref()
            {
                render_md_editor_pane(
                    app_entity.clone(),
                    side,
                    tab_id,
                    ed,
                    editor_find.as_ref(),
                    source_mode,
                    zen_mode,
                    cx,
                )
                .into_any_element()
            } else {
                let vault = Arc::clone(&app.vault);
                live_doc.update(cx, |ed, _| ed.sync_render_context(false, zen_mode, vault, false));
                live_doc.into_any_element()
            };
            let preview_el = if let Some((idx, total, title, ch_content)) = chapter_nav {
                let vault = Arc::clone(&app.vault);
                sync_read_preview_doc(&preview_live_doc, &ch_content, false, zen_mode, vault, cx);
                wrap_with_chapter_footer(
                    preview_live_doc.into_any_element(),
                    side,
                    tab_id,
                    idx,
                    total,
                    title,
                    cx,
                )
            } else {
                let vault = Arc::clone(&app.vault);
                sync_read_preview_doc(&preview_live_doc, &raw_text, false, zen_mode, vault, cx);
                preview_live_doc.into_any_element()
            };
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

    let read_find_card = if effective_mode == MdViewMode::Reading && read_find_open {
        let trimmed_non_empty = !read_find_query.trim().is_empty();
        let count_text = if !trimmed_non_empty {
            String::new()
        } else if read_find_total == 0 {
            "0 / 0".to_string()
        } else {
            format!("{} / {}", read_find_active + 1, read_find_total)
        };
        let hit_row = read_find_hit.as_ref().map(|h| {
            h_flex()
                .gap_1p5()
                .items_center()
                .pl(px(4.))
                .child(
                    div()
                        .text_xs()
                        .text_color(hex_color(kd::ACCENT))
                        .child(format!("L{}", h.line)),
                )
                .child(
                    div()
                        .max_w(px(210.))
                        .text_xs()
                        .text_color(hex_color(kd::FG_MUTED))
                        .truncate()
                        .child(h.snippet.clone()),
                )
                .child(
                    Button::new(SharedString::from(format!("md-rfind-edit-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .icon(IconName::Pencil)
                        .label("Edit")
                        .tooltip("Jump to match in Edit mode")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.jump_md_read_match_to_edit(side, tab_id, window, cx);
                        })),
                )
        });
        let rfi_clr = read_find_input.clone();
        let clear_btn = Button::new(SharedString::from(format!("md-rfind-x-{tab_id}")))
            .text()
            .xsmall()
            .tab_stop(false)
            .icon(IconName::Close)
            .tooltip("Clear")
            .on_click(cx.listener(move |this, _, window, cx| {
                rfi_clr.update(cx, |s, cx| s.set_value("", window, cx));
                rfi_clr.focus_handle(cx).focus(window, cx);
                this.clear_md_read_find_query(side, tab_id, cx);
            }));

        let row1 = h_flex()
            .gap_1()
            .items_center()
            .child(
                div().w(px(FIND_INPUT_W)).child(
                    Input::new(&read_find_input)
                        .xsmall()
                        .prefix(
                            Icon::new(IconName::Search)
                                .xsmall()
                                .text_color(hex_color(kd::FG_MUTED)),
                        )
                        .when(!read_find_query.is_empty(), |i| i.suffix(clear_btn)),
                ),
            )
            .when(!count_text.is_empty(), |row| {
                row.child(find_count_badge(
                    count_text,
                    trimmed_non_empty && read_find_total == 0,
                ))
            })
            .child(
                Button::new(SharedString::from(format!("md-rfind-case-{tab_id}")))
                    .with_variant(if read_find_case {
                        ButtonVariant::Secondary
                    } else {
                        ButtonVariant::Ghost
                    })
                    .xsmall()
                    .label("Aa")
                    .tooltip("Match Case")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_md_read_find_case(side, tab_id, cx);
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("md-rfind-prev-{tab_id}")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronUp)
                    .tooltip("Previous Match (Shift+Enter)")
                    .disabled(read_find_total == 0)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.step_md_read_find(side, tab_id, false, cx);
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("md-rfind-next-{tab_id}")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronDown)
                    .tooltip("Next Match (Enter)")
                    .disabled(read_find_total == 0)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.step_md_read_find(side, tab_id, true, cx);
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("md-rfind-close-{tab_id}")))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("Close (Esc)")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_md_read_find(side, tab_id, Some(window), cx);
                    })),
            );

        Some(
            floating_find_card(SharedString::from(format!("md-rfind-card-{tab_id}")))
                .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.key.as_str() == "escape" {
                        cx.stop_propagation();
                        this.close_md_read_find(side, tab_id, Some(window), cx);
                    } else if ev.keystroke.key.as_str() == "f3" {
                        cx.stop_propagation();
                        this.step_md_read_find(side, tab_id, !ev.keystroke.modifiers.shift, cx);
                    }
                }))
                .child(v_flex().gap_1().child(row1).children(hit_row)),
        )
    } else {
        None
    };

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .when(!zen_mode, |this| this.child(header))
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(body)
                .when_some(read_find_card, |d, card| d.child(card)),
        )
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
        .on_click(cx.listener(move |this, _, _window, cx| {
            this.active_pane = side;
            if let Some(tab) = this.find_tab_mut(side, tab_id)
                && let TabKind::Markdown { view_mode, .. } = &mut tab.kind
            {
                *view_mode = target_mode;
                cx.notify();
            }
        }))
}

#[allow(clippy::too_many_arguments)]
fn render_md_editor_pane(
    app_entity: gpui_kit::Entity<VKarmaApp>,
    side: PaneSide,
    tab_id: u64,
    editor: &gpui_kit::Entity<gpui_kit::component::input::EditorState>,
    editor_find: Option<&gpui_kit::Entity<EditorFindBar>>,
    source_mode: bool,
    zen_mode: bool,
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

    let framed = super::zen_editor_frame(ed, zen_mode);
    if let Some(fb) = editor_find {
        wrap_editor_with_find(framed, fb, cx)
    } else {
        framed
    }
}

fn preview_link_tooltip_text(url: &str, current_file: &Path, vault: &VaultIndex) -> String {
    if let Some(target) =
        crate::markdown::decode_uri_payload(url, crate::markdown::VKARMA_NOTE_SCHEME)
    {
        if let Some(resolved) = vault.resolve_wikilink(&target, current_file) {
            let rel = resolved.strip_prefix(&vault.root).unwrap_or(&resolved);
            return format!("Open {}", rel.display());
        }
        let mut clean = target
            .split('|')
            .next()
            .unwrap_or(&target)
            .split('#')
            .next()
            .unwrap_or(&target)
            .trim();
        while let Some(rest) = clean.strip_prefix("./").or_else(|| clean.strip_prefix(".\\")) {
            clean = rest;
        }
        let normalized = crate::vault::normalize_new_file_name(clean);
        let fallback = if normalized.is_empty() {
            target
        } else {
            normalized
        };
        return format!("Create {fallback}");
    }
    url.to_string()
}

fn sync_read_preview_doc(
    preview_doc: &Entity<LiveMarkdownEditor>,
    target_raw: &str,
    centered: bool,
    zen_mode: bool,
    vault: Arc<VaultIndex>,
    cx: &mut Context<VKarmaApp>,
) {
    let needs_text_sync = preview_doc.read(cx).text() != target_raw;
    preview_doc.update(cx, |ed, cx| {
        if needs_text_sync {
            ed.set_text(target_raw.to_string(), 0, 0, cx);
        }
        ed.sync_render_context(centered, zen_mode, vault, true);
    });
}

fn wrap_with_chapter_footer(
    content: gpui_kit::AnyElement,
    side: PaneSide,
    tab_id: u64,
    idx: usize,
    total: usize,
    title: String,
    cx: &mut Context<VKarmaApp>,
) -> gpui_kit::AnyElement {
    let can_prev = idx > 0;
    let can_next = idx + 1 < total;
    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .child(div().flex_1().min_h_0().w_full().child(content))
        .child(
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
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{VKARMA_NOTE_SCHEME, encode_uri_payload};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn preview_link_tooltip_text_formats_resolved_missing_and_url() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();

        let welcome = root.join("docs/welcome.md");
        let screenplay = root.join("scripts/Screenplay.fountain");
        fs::write(&welcome, "# Welcome").unwrap();
        fs::write(&screenplay, "Title: Demo").unwrap();

        let vault = VaultIndex::build(root, false);

        let resolved_uri = format!(
            "{VKARMA_NOTE_SCHEME}{}",
            encode_uri_payload("Screenplay.fountain|Read Script")
        );
        assert_eq!(
            preview_link_tooltip_text(&resolved_uri, &welcome, &vault),
            "Open scripts/Screenplay.fountain"
        );

        let missing_md_uri = format!("{VKARMA_NOTE_SCHEME}{}", encode_uri_payload("Missing Note"));
        assert_eq!(
            preview_link_tooltip_text(&missing_md_uri, &welcome, &vault),
            "Create Missing Note.md"
        );

        let missing_cs_uri = format!("{VKARMA_NOTE_SCHEME}{}", encode_uri_payload("Player.cs"));
        assert_eq!(
            preview_link_tooltip_text(&missing_cs_uri, &welcome, &vault),
            "Create Player.cs"
        );

        assert_eq!(
            preview_link_tooltip_text("https://example.com", &welcome, &vault),
            "https://example.com"
        );
    }
}
