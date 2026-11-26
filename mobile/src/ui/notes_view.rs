use super::{ActiveNote, DrawerCreateKind, ItemContextTarget, VKarmaMobileApp};
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use crate::vault::{MarkdownFormatAction, find_line_matches};
use gpui::{prelude::*, *};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{Icon, IconName, Sizable as _, h_flex, v_flex};
use std::path::PathBuf;

fn svg_icon(name: IconName, sz: f32, color: u32) -> impl IntoElement {
    Icon::new(name).size(px(sz)).text_color(hex_color(color))
}

fn long_press_overlay(
    on_trigger: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_trigger = std::rc::Rc::new(on_trigger);
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
        move |_bounds, hitbox, window, _cx| {
            let on_trigger = on_trigger.clone();
            window.on_mouse_event(move |ev: &gpui::LongPressEvent, phase, window, cx| {
                if phase == gpui::DispatchPhase::Bubble
                    && ev.phase == gpui::TouchPhase::Started
                    && !window.default_prevented()
                    && hitbox.is_hovered(window)
                {
                    window.prevent_default();
                    cx.stop_propagation();
                    on_trigger(ev.position, window, cx);
                }
            });
        },
    )
    .absolute()
    .inset_0()
}

pub fn render_notes_workspace(
    app: &VKarmaMobileApp,
    bottom_inset: f32,
    window: &Window,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let vault_label = app
        .vault_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Vault")
        .to_string();

    let (title_text, subtitle_text, dirty, saved_badge, reading_only, find_open) =
        if let Some(note) = &app.active_note {
            (
                note.title.clone(),
                format!("{} · {} words", note.rel_folder, note.stats.0),
                note.dirty,
                note.saved_badge.clone(),
                note.reading_only,
                note.find_open,
            )
        } else {
            (
                vault_label.clone(),
                format!("{} notes", app.vault_index.note_count()),
                false,
                None,
                false,
                false,
            )
        };

    let top_bar = h_flex()
        .w_full()
        .h(px(48.))
        .px_2p5()
        .bg(hex_color(kd::BG_DEEP))
        .border_b_1()
        .border_color(hex_color(kd::BORDER_SUBTLE))
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .id("obs-drawer-btn")
                .w(px(38.))
                .h(px(38.))
                .rounded(px(9.))
                .flex_shrink_0()
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .bg(if app.drawer_open {
                    accent_alpha(0x2e)
                } else {
                    gpui::transparent_black()
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_drawer(!this.drawer_open, window, cx);
                }))
                .child(svg_icon(IconName::Menu, 18., kd::FG)),
        )
        .child(
            div()
                .id("obs-header-title")
                .flex_1()
                .min_w_0()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.active_note.is_some() {
                        this.more_sheet_open = true;
                        this.rename_mode = false;
                        window.blur(cx);
                        gpui_mobile::hide_keyboard();
                        cx.notify();
                    } else {
                        this.open_quick_switcher(window, cx);
                    }
                }))
                .child(
                    v_flex()
                        .w_full()
                        .min_w_0()
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(hex_color(kd::FG))
                                        .truncate()
                                        .child(title_text),
                                )
                                .when(dirty, |row| {
                                    row.child(
                                        div()
                                            .flex_shrink_0()
                                            .w(px(7.))
                                            .h(px(7.))
                                            .rounded(px(4.))
                                            .bg(hex_color(kd::YELLOW)),
                                    )
                                })
                                .when(!dirty && saved_badge.is_some(), |row| {
                                    row.child(svg_icon(IconName::Check, 13., kd::GREEN))
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(hex_color(kd::FG_MUTED))
                                .truncate()
                                .child(subtitle_text),
                        ),
                ),
        )
        .child(if app.active_note.is_some() {
            h_flex()
                .gap_1()
                .flex_shrink_0()
                .items_center()
                .child(
                    div()
                        .id("obs-mode-toggle")
                        .h(px(34.))
                        .px_2p5()
                        .rounded(px(8.))
                        .cursor_pointer()
                        .bg(if reading_only {
                            accent_alpha(0x34)
                        } else {
                            hex_color(kd::BG_SIDEBAR)
                        })
                        .border_1()
                        .border_color(hex_color(if reading_only {
                            kd::ACCENT
                        } else {
                            kd::BORDER
                        }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap_1()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hex_color(if reading_only {
                            kd::ACCENT_HOVER
                        } else {
                            kd::FG
                        }))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_note_reading_mode(window, cx);
                        }))
                        .child(svg_icon(
                            if reading_only {
                                IconName::FileText
                            } else {
                                IconName::Eye
                            },
                            13.,
                            if reading_only {
                                kd::ACCENT_HOVER
                            } else {
                                kd::FG_SUBTLE
                            },
                        ))
                        .child(if reading_only { "Live" } else { "Read" }),
                )
                .child(
                    div()
                        .id("obs-more-btn")
                        .w(px(36.))
                        .h(px(34.))
                        .rounded(px(8.))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, window, cx| {
                            window.blur(cx);
                            gpui_mobile::hide_keyboard();
                            this.more_sheet_open = !this.more_sheet_open;
                            this.rename_mode = false;
                            if let Some(n) = &mut this.active_note {
                                n.delete_armed = false;
                            }
                            cx.notify();
                        }))
                        .child(svg_icon(IconName::Ellipsis, 18., kd::FG_SUBTLE)),
                )
                .into_any_element()
        } else {
            h_flex()
                .gap_1p5()
                .flex_shrink_0()
                .items_center()
                .child(
                    Button::new("obs-home-search")
                        .secondary()
                        .small()
                        .label("Find")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_quick_switcher(window, cx);
                        })),
                )
                .child(
                    Button::new("obs-home-new")
                        .primary()
                        .small()
                        .label("+ Note")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_new_note_modal(window, cx);
                        })),
                )
                .into_any_element()
        });

    let find_strip = if let Some(note) = &app.active_note
        && find_open
    {
        let q = app.note_find_input.read(cx).value().to_string();
        let hits = if q.trim().is_empty() {
            Vec::new()
        } else {
            find_line_matches(&note.content, &q, 16)
        };
        Some(
            v_flex()
                .w_full()
                .px_3()
                .py_2()
                .gap_1p5()
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_b_1()
                .border_color(hex_color(kd::BORDER_SUBTLE))
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&app.note_find_input).small().cleanable(true)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(hex_color(kd::FG_MUTED))
                                .child(format!("{} hits", hits.len())),
                        )
                        .child(
                            div()
                                .id("note-find-close")
                                .w(px(28.))
                                .h(px(28.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    if let Some(n) = &mut this.active_note {
                                        n.find_open = false;
                                    }
                                    window.blur(cx);
                                    gpui_mobile::hide_keyboard();
                                    cx.notify();
                                }))
                                .child(svg_icon(IconName::Close, 14., kd::FG_MUTED)),
                        ),
                )
                .when(!hits.is_empty(), |col| {
                    col.child(
                        v_flex().w_full().gap_1().children(
                            hits.into_iter().take(4).enumerate().map(|(ix, (ln, snip))| {
                                div()
                                    .id(SharedString::from(format!("find-hit-{ix}")))
                                    .w_full()
                                    .px_2p5()
                                    .py_1()
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .bg(hex_color(kd::BG_PANEL))
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_SUBTLE))
                                    .truncate()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.scroll_note_to_line(ln.saturating_sub(1), window, cx);
                                    }))
                                    .child(format!("L{ln}: {snip}"))
                            }),
                        ),
                    )
                }),
        )
    } else {
        None
    };

    let blocked = app.has_modal_overlay();
    if app.active_note.is_some() {
        let ed = app.live_editor.clone();
        ed.update(cx, |editor, _| {
            editor.set_read_only(reading_only);
            editor.set_interaction_blocked(blocked);
        });
    }

    let canvas = match &app.active_note {
        Some(note) if note.reading_only => {
            render_reading_canvas(app, note, bottom_inset, cx).into_any_element()
        }
        Some(note) => render_live_edit_canvas(app, note, bottom_inset, cx).into_any_element(),
        None => render_empty_tab_home(app, bottom_inset, window, cx).into_any_element(),
    };

    v_flex()
        .size_full()
        .child(top_bar)
        .when(!app.has_storage_access && app.active_note.is_some(), |col| {
            col.child(
                h_flex()
                    .w_full()
                    .px_3p5()
                    .py_2()
                    .gap_2()
                    .justify_between()
                    .items_center()
                    .bg(hex_alpha(0xc4b28a20))
                    .border_b_1()
                    .border_color(hex_color(kd::YELLOW))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(hex_color(kd::YELLOW))
                            .child("All files access needed for /sdcard notes"),
                    )
                    .child(
                        Button::new("top-grant-storage")
                            .primary()
                            .small()
                            .label("Grant Access")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.request_storage_permission(cx);
                            })),
                    ),
            )
        })
        .when_some(find_strip, |col, strip| col.child(strip))
        .when_some(app.status_banner.clone(), |col, msg| {
            col.child(
                h_flex()
                    .w_full()
                    .px_3p5()
                    .py_1p5()
                    .justify_between()
                    .items_center()
                    .bg(hex_color(kd::BG_PANEL))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(hex_color(kd::AQUA))
                            .truncate()
                            .child(msg),
                    )
                    .child(
                        div()
                            .id("dismiss-banner")
                            .px_1p5()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.status_banner = None;
                                cx.notify();
                            }))
                            .child(svg_icon(IconName::Close, 13., kd::FG_MUTED)),
                    ),
            )
        })
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .child(canvas),
        )
}

fn render_formatting_bar(
    app: &VKarmaMobileApp,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let tools: &[(&str, &str, Option<MarkdownFormatAction>)] = &[
        ("h", "#", Some(MarkdownFormatAction::Heading)),
        ("b", "B", Some(MarkdownFormatAction::Bold)),
        ("i", "I", Some(MarkdownFormatAction::Italic)),
        ("chk", "[ ]", Some(MarkdownFormatAction::Checkbox)),
        ("lst", "•", Some(MarkdownFormatAction::Bullet)),
        ("qt", ">", Some(MarkdownFormatAction::Quote)),
        ("cd", "<>", Some(MarkdownFormatAction::Code)),
        ("wl", "[[]]", Some(MarkdownFormatAction::Wikilink)),
        ("hr", "—", Some(MarkdownFormatAction::HorizontalRule)),
    ];

    h_flex()
        .w_full()
        .h(px(44.))
        .flex_shrink_0()
        .bg(hex_color(kd::BG_SIDEBAR))
        .border_t_1()
        .border_color(hex_color(kd::BORDER_SUBTLE))
        .items_center()
        .child(
            div()
                .id("obs-fmt-strip")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_x_scroll()
                .track_scroll(&app.toolbar_scroll)
                .flex()
                .items_center()
                .px_2()
                .gap_1p5()
                .children(tools.iter().map(|&(id, glyph, action)| {
                    let act = action.expect("format action");
                    div()
                        .id(SharedString::from(format!("fmt-{id}")))
                        .min_w(px(36.))
                        .h(px(32.))
                        .px_2()
                        .flex_shrink_0()
                        .rounded(px(7.))
                        .cursor_pointer()
                        .bg(hex_color(kd::BG_PANEL))
                        .border_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(hex_color(kd::FG))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.apply_editor_format(act, window, cx);
                        }))
                        .child(glyph)
                }))
                .child(
                    div()
                        .id("fmt-undo")
                        .min_w(px(34.))
                        .h(px(32.))
                        .px_2()
                        .flex_shrink_0()
                        .rounded(px(7.))
                        .cursor_pointer()
                        .bg(hex_color(kd::BG_PANEL))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.editor_undo(window, cx);
                        }))
                        .child(svg_icon(IconName::Undo2, 15., kd::FG_SUBTLE)),
                )
                .child(
                    div()
                        .id("fmt-redo")
                        .min_w(px(34.))
                        .h(px(32.))
                        .px_2()
                        .flex_shrink_0()
                        .rounded(px(7.))
                        .cursor_pointer()
                        .bg(hex_color(kd::BG_PANEL))
                        .flex()
                        .items_center()
                        .justify_center()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.editor_redo(window, cx);
                        }))
                        .child(svg_icon(IconName::Redo2, 15., kd::FG_SUBTLE)),
                ),
        )
        .child(
            div()
                .id("fmt-done")
                .h(px(32.))
                .px_2p5()
                .mx_1p5()
                .flex_shrink_0()
                .rounded(px(7.))
                .cursor_pointer()
                .bg(accent_alpha(0x30))
                .flex()
                .items_center()
                .justify_center()
                .on_click(cx.listener(|this, _, window, cx| {
                    this.flush_active_note_sync(cx);
                    window.blur(cx);
                    gpui_mobile::hide_keyboard();
                    cx.notify();
                }))
                .child(svg_icon(IconName::ChevronDown, 16., kd::ACCENT_HOVER)),
        )
}

fn render_tag_strip(
    note: &ActiveNote,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .px_3p5()
        .py_1p5()
        .gap_1p5()
        .border_b_1()
        .border_color(hex_color(kd::BORDER_SUBTLE))
        .bg(hex_color(kd::BG_MAIN))
        .overflow_x_hidden()
        .children(note.tags.iter().take(6).enumerate().map(|(ix, tag)| {
            let tag_q = format!("#{tag}");
            div()
                .id(SharedString::from(format!("note-tag-{ix}")))
                .px_2()
                .py_0p5()
                .rounded(px(99.))
                .cursor_pointer()
                .bg(accent_alpha(0x22))
                .text_size(px(10.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(hex_color(kd::ACCENT_HOVER))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_quick_switcher(window, cx);
                    this.quick_switcher_content_mode = true;
                    this.quick_switcher_input.update(cx, |inp, cx| {
                        inp.set_value(tag_q.clone(), window, cx);
                    });
                    this.on_quick_switcher_query_changed(tag_q.clone(), cx);
                }))
                .child(format!("#{tag}"))
        }))
}

fn render_links_strip(
    note: &ActiveNote,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .px_3()
        .py_1p5()
        .gap_1p5()
        .items_center()
        .bg(hex_color(kd::BG_SIDEBAR))
        .border_t_1()
        .border_color(hex_color(kd::BORDER_SUBTLE))
        .overflow_x_hidden()
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(10.5))
                .font_weight(FontWeight::BOLD)
                .text_color(hex_color(kd::FG_MUTED))
                .child(format!("Links ({})", note.wikilinks.len())),
        )
        .children(note.wikilinks.iter().take(6).enumerate().map(|(ix, wl)| {
            let target = wl.target.clone();
            div()
                .id(SharedString::from(format!("wl-chip-{ix}")))
                .px_2()
                .py_0p5()
                .rounded(px(6.))
                .flex_shrink_0()
                .cursor_pointer()
                .bg(hex_color(kd::BG_PANEL))
                .border_1()
                .border_color(hex_color(kd::BORDER_SUBTLE))
                .text_size(px(11.))
                .text_color(hex_color(kd::LINK))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_wikilink(&target, window, cx);
                }))
                .child(wl.display.clone())
        }))
}

fn render_live_edit_canvas(
    app: &VKarmaMobileApp,
    note: &ActiveNote,
    bottom_inset: f32,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let keyboard_visible = bottom_inset > app.safe_area.bottom + 40.0;
    let show_toolbar = app.live_editor_focused || keyboard_visible;

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .when(!note.tags.is_empty(), |col| {
            col.child(render_tag_strip(note, cx))
        })
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .child(app.live_editor.clone()),
        )
        .when(!show_toolbar && !note.wikilinks.is_empty(), |col| {
            col.child(render_links_strip(note, cx))
        })
        .child(if show_toolbar {
            v_flex()
                .w_full()
                .flex_shrink_0()
                .child(render_formatting_bar(app, cx))
                .when(bottom_inset > 0.0, |col| {
                    col.child(
                        div()
                            .w_full()
                            .h(px(bottom_inset))
                            .flex_shrink_0()
                            .bg(hex_color(kd::BG_SIDEBAR)),
                    )
                })
                .into_any_element()
        } else {
            render_obsidian_bottom_bar(app, bottom_inset, cx).into_any_element()
        })
}

fn render_reading_canvas(
    app: &VKarmaMobileApp,
    note: &ActiveNote,
    bottom_inset: f32,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .when(!note.tags.is_empty(), |col| {
            col.child(render_tag_strip(note, cx))
        })
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .child(app.live_editor.clone()),
        )
        .when(!note.wikilinks.is_empty(), |col| {
            col.child(render_links_strip(note, cx))
        })
        .child(render_obsidian_bottom_bar(app, bottom_inset, cx))
}

fn render_empty_tab_home(
    app: &VKarmaMobileApp,
    bottom_inset: f32,
    _window: &Window,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let vault_name = app
        .vault_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Vault")
        .to_string();

    let mut recents: Vec<(PathBuf, String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in &app.recent_notes {
        if let Some(node) = app.vault_index.notes.iter().find(|n| &n.path == p)
            && seen.insert(node.path.clone())
        {
            recents.push((
                node.path.clone(),
                node.display_name.clone(),
                node.rel_path.clone(),
            ));
        }
    }
    for node in &app.vault_index.notes {
        if recents.len() >= 10 {
            break;
        }
        if seen.insert(node.path.clone()) {
            recents.push((
                node.path.clone(),
                node.display_name.clone(),
                node.rel_path.clone(),
            ));
        }
    }

    let body = v_flex()
        .w_full()
        .p_4()
        .gap_4()
        .when(!app.has_storage_access, |col| {
            col.child(
                v_flex()
                    .w_full()
                    .p_3p5()
                    .gap_2()
                    .rounded(px(12.))
                    .bg(hex_alpha(0xc4b28a20))
                    .border_1()
                    .border_color(hex_color(kd::YELLOW))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(hex_color(kd::YELLOW))
                            .child("All files access required for /sdcard vaults"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(hex_color(kd::FG_SUBTLE))
                            .child("Android needs All files access to read and sync your Markdown vault in /sdcard/Documents."),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("home-grant-storage")
                                    .primary()
                                    .small()
                                    .label("Grant All Files Access")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.request_storage_permission(cx);
                                    })),
                            )
                            .child(
                                Button::new("home-recheck-storage")
                                    .secondary()
                                    .small()
                                    .label("Recheck")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.recheck_storage_and_refresh(cx);
                                    })),
                            ),
                    ),
            )
        })
        .child(
            v_flex()
                .w_full()
                .p_4()
                .gap_3()
                .rounded(px(14.))
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_1()
                .border_color(hex_color(kd::BORDER_SUBTLE))
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .text_color(hex_color(kd::FG))
                                .child(vault_name),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(hex_color(kd::FG_SUBTLE))
                                .child(if app.vault_indexing && app.vault_index.note_count() == 0 {
                                    "Indexing vault…".to_string()
                                } else if app.vault_indexing {
                                    format!("{} notes · refreshing…", app.vault_index.note_count())
                                } else {
                                    format!("{} notes ready", app.vault_index.note_count())
                                }),
                        ),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .id("home-create-note")
                                .w_full()
                                .h(px(44.))
                                .px_3p5()
                                .rounded(px(10.))
                                .cursor_pointer()
                                .bg(hex_color(kd::ACCENT))
                                .flex()
                                .items_center()
                                .justify_between()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(hex_color(kd::BG_MAIN))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_new_note_modal(window, cx);
                                }))
                                .child("+ Create new note")
                                .child(svg_icon(IconName::ArrowRight, 16., kd::BG_MAIN)),
                        )
                        .child(
                            div()
                                .id("home-daily-note")
                                .w_full()
                                .h(px(44.))
                                .px_3p5()
                                .rounded(px(10.))
                                .cursor_pointer()
                                .bg(hex_color(kd::BG_PANEL))
                                .border_1()
                                .border_color(hex_color(kd::BORDER_SUBTLE))
                                .flex()
                                .items_center()
                                .justify_between()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(hex_color(kd::FG))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_today_daily_note(window, cx);
                                }))
                                .child("Today's daily note")
                                .child(svg_icon(IconName::Calendar, 16., kd::ACCENT_HOVER)),
                        )
                        .child(
                            div()
                                .id("home-quick-switcher")
                                .w_full()
                                .h(px(44.))
                                .px_3p5()
                                .rounded(px(10.))
                                .cursor_pointer()
                                .bg(hex_color(kd::BG_PANEL))
                                .border_1()
                                .border_color(hex_color(kd::BORDER_SUBTLE))
                                .flex()
                                .items_center()
                                .justify_between()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(hex_color(kd::FG))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_quick_switcher(window, cx);
                                }))
                                .child("Open quick switcher")
                                .child(svg_icon(IconName::Search, 16., kd::FG_SUBTLE)),
                        ),
                ),
        )
        .when(!recents.is_empty(), |col| {
            col.child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .px_1()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(hex_color(kd::FG_MUTED))
                            .child("RECENT NOTES"),
                    )
                    .children(recents.into_iter().enumerate().map(|(ix, (path, name, rel))| {
                        let lp_path = path.clone();
                        let lp_rel = rel.clone();
                        let lp_name = name.clone();
                        div()
                            .id(SharedString::from(format!("recent-note-{ix}")))
                            .relative()
                            .w_full()
                            .min_h(px(48.))
                            .px_3p5()
                            .py_2()
                            .rounded(px(10.))
                            .cursor_pointer()
                            .bg(hex_color(kd::BG_SIDEBAR))
                            .border_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_note(path.clone(), false, true, window, cx);
                            }))
                            .child(long_press_overlay({
                                let weak = cx.entity().downgrade();
                                move |_, window, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.open_item_context_menu(
                                            ItemContextTarget::Note {
                                                path: lp_path.clone(),
                                                rel_path: lp_rel.clone(),
                                                display_name: lp_name.clone(),
                                            },
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(hex_color(kd::FG))
                                            .truncate()
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(hex_color(kd::FG_MUTED))
                                            .truncate()
                                            .child(rel),
                                    ),
                            )
                            .child(svg_icon(IconName::ChevronRight, 15., kd::FG_MUTED))
                    })),
            )
        });

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .child(
            div()
                .id("obs-home-scroll")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .track_scroll(&app.home_scroll)
                .child(body),
        )
        .child(render_obsidian_bottom_bar(app, bottom_inset, cx))
}

fn render_obsidian_bottom_bar(
    app: &VKarmaMobileApp,
    bottom_inset: f32,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let can_back = !app.history_back.is_empty();
    let can_fwd = !app.history_forward.is_empty();

    v_flex()
        .w_full()
        .flex_shrink_0()
        .bg(hex_color(kd::BG_DEEP))
        .border_t_1()
        .border_color(hex_color(kd::BORDER_SUBTLE))
        .child(
            h_flex()
                .w_full()
                .h(px(50.))
                .px_2()
                .items_center()
                .justify_around()
                .child(nav_icon_btn(
                    "bot-back",
                    IconName::ArrowLeft,
                    can_back,
                    false,
                    cx.listener(|this, _, window, cx| {
                        this.navigate_history_back(window, cx);
                    }),
                ))
                .child(nav_icon_btn(
                    "bot-fwd",
                    IconName::ArrowRight,
                    can_fwd,
                    false,
                    cx.listener(|this, _, window, cx| {
                        this.navigate_history_forward(window, cx);
                    }),
                ))
                .child(nav_icon_btn(
                    "bot-search",
                    IconName::Search,
                    true,
                    false,
                    cx.listener(|this, _, window, cx| {
                        this.open_quick_switcher(window, cx);
                    }),
                ))
                .child(nav_icon_btn(
                    "bot-new",
                    IconName::Plus,
                    true,
                    true,
                    cx.listener(|this, _, window, cx| {
                        this.open_new_note_modal(window, cx);
                    }),
                )),
        )
        .when(bottom_inset > 0.0, |col| {
            col.child(div().w_full().h(px(bottom_inset)))
        })
}

fn nav_icon_btn(
    id: &'static str,
    icon: IconName,
    enabled: bool,
    accented: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let color = if accented {
        kd::ACCENT_HOVER
    } else if enabled {
        kd::FG
    } else {
        kd::FG_MUTED
    };

    div()
        .id(id)
        .flex_1()
        .max_w(px(58.))
        .h(px(40.))
        .rounded(px(10.))
        .cursor_pointer()
        .bg(if accented {
            accent_alpha(0x34)
        } else {
            gpui::transparent_black()
        })
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |d| d.on_click(on_click))
        .child(svg_icon(icon, 19., color))
}

pub fn render_sidebar_drawer(
    app: &VKarmaMobileApp,
    window: &Window,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let vp_w = f32::from(window.viewport_size().width) - app.safe_area.left - app.safe_area.right;
    let drawer_w = (vp_w * 0.84).clamp(240.0, 316.0);
    let vault_name = app
        .vault_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Vault")
        .to_string();

    let rows = app.visible_tree.clone();
    let row_count = rows.len();
    let active_path = app.active_note.as_ref().map(|n| n.path.clone());
    let app_weak: AnyWeakEntity = cx.entity().downgrade().into();

    h_flex()
        .id("obs-drawer-overlay")
        .size_full()
        .absolute()
        .inset_0()
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .child(
            v_flex()
                .id("obs-drawer-panel")
                .w(px(drawer_w))
                .h_full()
                .flex_shrink_0()
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_r_1()
                .border_color(hex_color(kd::BORDER))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    h_flex()
                        .w_full()
                        .h(px(48.))
                        .px_3()
                        .bg(hex_color(kd::BG_DEEP))
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .items_center()
                        .justify_between()
                        .gap_1()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(hex_color(kd::FG))
                                        .truncate()
                                        .child(vault_name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .child(if app.vault_indexing && app.vault_index.note_count() == 0 {
                                            "Scanning…".to_string()
                                        } else {
                                            format!("{} notes", app.vault_index.note_count())
                                        }),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .flex_shrink_0()
                                .child(drawer_tool_btn(
                                    "drw-new-note",
                                    IconName::Plus,
                                    cx.listener(|this, _, window, cx| {
                                        this.start_drawer_create(
                                            DrawerCreateKind::Note,
                                            window,
                                            cx,
                                        );
                                    }),
                                ))
                                .child(drawer_tool_btn(
                                    "drw-new-dir",
                                    IconName::Folder,
                                    cx.listener(|this, _, window, cx| {
                                        this.start_drawer_create(
                                            DrawerCreateKind::Folder,
                                            window,
                                            cx,
                                        );
                                    }),
                                ))
                                .child(drawer_tool_btn(
                                    "drw-sort",
                                    IconName::ChevronsUpDown,
                                    cx.listener(|this, _, _, cx| {
                                        this.cycle_sort_mode(cx);
                                    }),
                                ))
                                .child(drawer_tool_btn(
                                    "drw-rescan",
                                    IconName::RefreshCw,
                                    cx.listener(|this, _, _, cx| {
                                        this.refresh_vault_async(cx);
                                    }),
                                )),
                        ),
                )
                .child(
                    v_flex()
                        .w_full()
                        .px_2p5()
                        .py_2()
                        .gap_2()
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .child(
                            h_flex()
                                .w_full()
                                .gap_1p5()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(Input::new(&app.drawer_filter_input).small().cleanable(true)),
                                )
                                .child(
                                    div()
                                        .px_1p5()
                                        .py_1()
                                        .rounded(px(6.))
                                        .bg(hex_color(kd::BG_PANEL))
                                        .text_size(px(10.))
                                        .text_color(hex_color(kd::FG_SUBTLE))
                                        .child(app.sort_mode.label()),
                                ),
                        )
                        .when_some(app.drawer_creating, |col, kind| {
                            let lbl = match kind {
                                DrawerCreateKind::Note => "Note",
                                DrawerCreateKind::Folder => "Dir",
                            };
                            col.child(
                                h_flex()
                                    .w_full()
                                    .gap_1p5()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .child(Input::new(&app.drawer_create_input).small()),
                                    )
                                    .child(
                                        Button::new("drw-create-go")
                                            .primary()
                                            .small()
                                            .label(lbl)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.submit_drawer_create(window, cx);
                                            })),
                                    )
                                    .child(drawer_tool_btn(
                                        "drw-create-cancel",
                                        IconName::Close,
                                        cx.listener(|this, _, window, cx| {
                                            this.drawer_creating = None;
                                            window.blur(cx);
                                            gpui_mobile::hide_keyboard();
                                            cx.notify();
                                        }),
                                    )),
                            )
                        }),
                )
                .child(if row_count == 0 {
                    div()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .p_4()
                        .text_xs()
                        .text_color(hex_color(kd::FG_MUTED))
                        .child(if app.vault_indexing {
                            "Indexing vault…"
                        } else {
                            "No matching notes in vault."
                        })
                        .into_any_element()
                } else {
                    uniform_list("obsidian-file-tree", row_count, move |range, _, _| {
                        range
                            .map(|ix| {
                                let row = &rows[ix];
                                let is_active =
                                    !row.is_dir && active_path.as_ref() == Some(&row.path);
                                let indent = px(10.0 + (row.depth.min(5) as f32) * 14.0);
                                let is_dir = row.is_dir;
                                let rel_dir = row.rel_path.clone();
                                let lp_rel = row.rel_path.clone();
                                let lp_name = row.display_name.clone();
                                let row_path = row.path.clone();
                                let lp_path = row.path.clone();
                                let weak = app_weak.clone();
                                let lp_weak = app_weak.clone();

                                let right_meta = if is_dir {
                                    format!("{}", row.child_count)
                                } else if !matches!(row.ext.as_str(), "" | "md" | "markdown") {
                                    row.ext.to_uppercase()
                                } else {
                                    String::new()
                                };

                                let row_icon = if is_dir {
                                    if row.is_expanded {
                                        svg_icon(IconName::ChevronDown, 14., kd::ACCENT_HOVER)
                                    } else {
                                        svg_icon(IconName::ChevronRight, 14., kd::ACCENT_HOVER)
                                    }
                                } else {
                                    svg_icon(
                                        IconName::FileText,
                                        13.,
                                        if is_active { kd::ACCENT_HOVER } else { kd::FG_MUTED },
                                    )
                                };

                                div()
                                    .id(SharedString::from(format!("tree-row-{ix}")))
                                    .relative()
                                    .w_full()
                                    .h(px(42.))
                                    .pl(indent)
                                    .pr_3()
                                    .cursor_pointer()
                                    .bg(if is_active {
                                        accent_alpha(0x2e)
                                    } else {
                                        gpui::transparent_black()
                                    })
                                    .border_l_2()
                                    .border_color(if is_active {
                                        hex_color(kd::ACCENT)
                                    } else {
                                        gpui::transparent_black()
                                    })
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .on_click(move |_, window, cx| {
                                        if let Some(any_ent) = weak.upgrade()
                                            && let Ok(ent) = any_ent.downcast::<VKarmaMobileApp>()
                                        {
                                            ent.update(cx, |this, cx| {
                                                if is_dir {
                                                    this.drawer_target_dir = Some(row_path.clone());
                                                    this.toggle_tree_dir(rel_dir.clone(), cx);
                                                } else {
                                                    this.open_note(
                                                        row_path.clone(),
                                                        false,
                                                        true,
                                                        window,
                                                        cx,
                                                    );
                                                }
                                            });
                                        }
                                    })
                                    .child(long_press_overlay(move |_, window, cx| {
                                        if let Some(any_ent) = lp_weak.upgrade()
                                            && let Ok(ent) = any_ent.downcast::<VKarmaMobileApp>()
                                        {
                                            let target = if is_dir {
                                                ItemContextTarget::Folder {
                                                    path: lp_path.clone(),
                                                    rel_path: lp_rel.clone(),
                                                    display_name: lp_name.clone(),
                                                }
                                            } else {
                                                ItemContextTarget::Note {
                                                    path: lp_path.clone(),
                                                    rel_path: lp_rel.clone(),
                                                    display_name: lp_name.clone(),
                                                }
                                            };
                                            ent.update(cx, |this, cx| {
                                                this.open_item_context_menu(target, window, cx);
                                            });
                                        }
                                    }))
                                    .child(
                                        h_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .w(px(16.))
                                                    .flex_shrink_0()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(row_icon),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_sm()
                                                    .font_weight(if is_dir || is_active {
                                                        FontWeight::SEMIBOLD
                                                    } else {
                                                        FontWeight::NORMAL
                                                    })
                                                    .text_color(hex_color(if is_active {
                                                        kd::ACCENT_HOVER
                                                    } else {
                                                        kd::FG
                                                    }))
                                                    .truncate()
                                                    .child(row.display_name.clone()),
                                            ),
                                    )
                                    .when(!right_meta.is_empty(), |r| {
                                        r.child(
                                            div()
                                                .flex_shrink_0()
                                                .text_size(px(10.5))
                                                .text_color(hex_color(kd::FG_MUTED))
                                                .child(right_meta),
                                        )
                                    })
                            })
                            .collect()
                    })
                    .track_scroll(&app.tree_scroll)
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .into_any_element()
                })
                .child(
                    v_flex()
                        .w_full()
                        .flex_shrink_0()
                        .bg(hex_color(kd::BG_DEEP))
                        .border_t_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .child(
                            h_flex()
                                .w_full()
                                .px_2p5()
                                .py_2()
                                .gap_1p5()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .id("drw-vault-switch")
                                        .flex_1()
                                        .min_w_0()
                                        .h(px(36.))
                                        .px_2p5()
                                        .rounded(px(8.))
                                        .cursor_pointer()
                                        .bg(hex_color(kd::BG_PANEL))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap_1p5()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_surface_settings(window, cx);
                                        }))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(hex_color(kd::FG))
                                                .truncate()
                                                .child(vault_name),
                                        )
                                        .child(svg_icon(
                                            IconName::ChevronsUpDown,
                                            13.,
                                            kd::ACCENT_HOVER,
                                        )),
                                )
                                .child(drawer_tool_btn(
                                    "drw-daily",
                                    IconName::Calendar,
                                    cx.listener(|this, _, window, cx| {
                                        this.open_today_daily_note(window, cx);
                                    }),
                                ))
                                .child(drawer_tool_btn(
                                    "drw-settings",
                                    IconName::Settings,
                                    cx.listener(|this, _, window, cx| {
                                        this.open_surface_settings(window, cx);
                                    }),
                                )),
                        )
                        .when(app.safe_area.bottom > 0.0, |col| {
                            col.child(div().w_full().h(px(app.safe_area.bottom)))
                        }),
                ),
        )
        .child(
            div()
                .id("obs-drawer-scrim")
                .flex_1()
                .h_full()
                .bg(hex_alpha(0x00000092))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.toggle_drawer(false, window, cx);
                })),
        )
}

fn drawer_tool_btn(
    id: &'static str,
    icon: IconName,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .w(px(32.))
        .h(px(32.))
        .rounded(px(7.))
        .flex_shrink_0()
        .cursor_pointer()
        .bg(hex_color(kd::BG_PANEL))
        .flex()
        .items_center()
        .justify_center()
        .on_click(on_click)
        .child(svg_icon(icon, 15., kd::FG))
}

pub fn render_quick_switcher(
    app: &VKarmaMobileApp,
    bottom_inset: f32,
    _window: &Window,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let query = app.quick_switcher_input.read(cx).value().trim().to_string();
    let show_create_row = !query.is_empty() && !app.vault_index.has_exact_note_name(&query);
    let app_weak: AnyWeakEntity = cx.entity().downgrade().into();

    let results_list = if app.quick_switcher_content_mode {
        if app.content_searching {
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .p_4()
                .text_xs()
                .text_color(hex_color(kd::FG_SUBTLE))
                .child("Searching note contents…")
                .into_any_element()
        } else if app.content_hits.is_empty() {
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .p_4()
                .text_xs()
                .text_color(hex_color(kd::FG_MUTED))
                .child(if query.is_empty() {
                    "Type keywords to search inside every note."
                } else {
                    "No content matches."
                })
                .into_any_element()
        } else {
            let hits = app.content_hits.clone();
            let count = hits.len();
            uniform_list("quick-switcher-content", count, move |range, _, _| {
                range
                    .map(|ix| {
                        let hit = &hits[ix];
                        let p = hit.path.clone();
                        let weak = app_weak.clone();
                        div()
                            .id(SharedString::from(format!("qs-c-{ix}")))
                            .w_full()
                            .h(px(58.))
                            .px_3p5()
                            .border_b_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .cursor_pointer()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap_0p5()
                            .on_click(move |_, window, cx| {
                                if let Some(any_ent) = weak.upgrade()
                                    && let Ok(ent) = any_ent.downcast::<VKarmaMobileApp>()
                                {
                                    ent.update(cx, |this, cx| {
                                        this.open_note(p.clone(), false, true, window, cx);
                                    });
                                }
                            })
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(hex_color(kd::FG))
                                            .truncate()
                                            .child(hit.name.clone()),
                                    )
                                    .when_some(hit.line_num, |r, ln| {
                                        r.child(
                                            div()
                                                .text_size(px(10.5))
                                                .text_color(hex_color(kd::ACCENT_HOVER))
                                                .child(format!("L{ln}")),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .truncate()
                                    .child(hit.snippet.clone()),
                            )
                    })
                    .collect()
            })
            .track_scroll(&app.switcher_scroll)
            .flex_1()
            .min_h_0()
            .w_full()
            .into_any_element()
        }
    } else if app.quick_hits.is_empty() {
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .p_4()
            .text_xs()
            .text_color(hex_color(kd::FG_MUTED))
            .child(if app.vault_indexing && app.vault_index.note_count() == 0 {
                "Indexing vault…"
            } else {
                "No matching note titles."
            })
            .into_any_element()
    } else {
        let hits = app.quick_hits.clone();
        let count = hits.len();
        uniform_list("quick-switcher-files", count, move |range, _, _| {
            range
                .map(|ix| {
                    let hit = &hits[ix];
                    let p = hit.path.clone();
                    let weak = app_weak.clone();
                    div()
                        .id(SharedString::from(format!("qs-f-{ix}")))
                        .w_full()
                        .h(px(54.))
                        .px_3p5()
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .on_click(move |_, window, cx| {
                            if let Some(any_ent) = weak.upgrade()
                                && let Ok(ent) = any_ent.downcast::<VKarmaMobileApp>()
                            {
                                ent.update(cx, |this, cx| {
                                    this.open_note(p.clone(), false, true, window, cx);
                                });
                            }
                        })
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(hex_color(kd::FG))
                                        .truncate()
                                        .child(hit.display_name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .truncate()
                                        .child(hit.folder_hint.clone()),
                                ),
                        )
                        .child(svg_icon(IconName::ChevronRight, 15., kd::FG_MUTED))
                })
                .collect()
        })
        .track_scroll(&app.switcher_scroll)
        .flex_1()
        .min_h_0()
        .w_full()
        .into_any_element()
    };

    div()
        .id("obs-qs-scrim")
        .size_full()
        .absolute()
        .inset_0()
        .occlude()
        .p_3()
        .pb(px(bottom_inset + 12.0))
        .bg(hex_alpha(0x0000009a))
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, window, cx| {
            cx.stop_propagation();
            this.close_quick_switcher(window, cx);
        }))
        .child(
            v_flex()
                .id("obs-qs-card")
                .w_full()
                .flex_1()
                .min_h_0()
                .max_h(px(520.))
                .rounded(px(14.))
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_1()
                .border_color(hex_color(kd::BORDER))
                .overflow_hidden()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    v_flex()
                        .w_full()
                        .flex_shrink_0()
                        .p_3()
                        .gap_2()
                        .bg(hex_color(kd::BG_DEEP))
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(Input::new(&app.quick_switcher_input).small().cleanable(true)),
                                )
                                .child(
                                    Button::new("qs-close")
                                        .ghost()
                                        .small()
                                        .label("Cancel")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.close_quick_switcher(window, cx);
                                        })),
                                ),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .gap_1p5()
                                .child(mode_pill(
                                    "qs-tab-files",
                                    "Files (Instant)",
                                    !app.quick_switcher_content_mode,
                                    cx.listener(|this, _, _, cx| {
                                        this.set_quick_switcher_mode(false, cx);
                                    }),
                                ))
                                .child(mode_pill(
                                    "qs-tab-content",
                                    "Content Search",
                                    app.quick_switcher_content_mode,
                                    cx.listener(|this, _, _, cx| {
                                        this.set_quick_switcher_mode(true, cx);
                                    }),
                                )),
                        ),
                )
                .when(query.is_empty() && !app.quick_switcher_content_mode, |col| {
                    col.child(
                        div()
                            .id("qs-daily")
                            .w_full()
                            .h(px(46.))
                            .flex_shrink_0()
                            .px_3p5()
                            .cursor_pointer()
                            .bg(accent_alpha(0x22))
                            .border_b_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .flex()
                            .items_center()
                            .justify_between()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_today_daily_note(window, cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(hex_color(kd::ACCENT_HOVER))
                                    .child("Today's Daily Note"),
                            )
                            .child(svg_icon(IconName::Calendar, 16., kd::ACCENT_HOVER)),
                    )
                })
                .when(show_create_row, |col| {
                    let create_q = query.clone();
                    col.child(
                        div()
                            .id("qs-create")
                            .w_full()
                            .h(px(48.))
                            .flex_shrink_0()
                            .px_3p5()
                            .cursor_pointer()
                            .bg(accent_alpha(0x28))
                            .border_b_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .flex()
                            .items_center()
                            .justify_between()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.create_note_by_name(&create_q, None, window, cx);
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(hex_color(kd::ACCENT_HOVER))
                                    .truncate()
                                    .child(format!("+ Create \"{query}.md\"")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::ACCENT_HOVER))
                                    .child("Enter"),
                            ),
                    )
                })
                .child(results_list),
        )
}

fn mode_pill(
    id: &'static str,
    label: &'static str,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_2p5()
        .py_1()
        .rounded(px(7.))
        .cursor_pointer()
        .bg(hex_color(if active { kd::ACCENT } else { kd::BG_PANEL }))
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(hex_color(if active { kd::BG_MAIN } else { kd::FG_SUBTLE }))
        .on_click(on_click)
        .child(label)
}

pub fn render_new_note_modal(
    app: &VKarmaMobileApp,
    bottom_inset: f32,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let target_name = app
        .drawer_target_dir
        .as_ref()
        .and_then(|d| d.strip_prefix(&app.vault_root).ok())
        .and_then(|p| p.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Vault Root")
        .to_string();

    div()
        .id("obs-new-scrim")
        .size_full()
        .absolute()
        .inset_0()
        .occlude()
        .p_4()
        .pb(px(bottom_inset + 16.0))
        .bg(hex_alpha(0x0000009a))
        .flex()
        .flex_col()
        .justify_center()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, window, cx| {
            cx.stop_propagation();
            this.new_note_modal_open = false;
            window.blur(cx);
            gpui_mobile::hide_keyboard();
            cx.notify();
        }))
        .child(
            v_flex()
                .id("obs-new-card")
                .w_full()
                .p_4()
                .gap_3()
                .rounded(px(14.))
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_1()
                .border_color(hex_color(kd::BORDER))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::BOLD)
                                .text_color(hex_color(kd::FG))
                                .child("New Note"),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(hex_color(kd::FG_MUTED))
                                .child(format!("Folder: {target_name}")),
                        ),
                )
                .child(Input::new(&app.new_note_input).small())
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("nn-daily")
                                .secondary()
                                .small()
                                .label("Daily Note")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.new_note_modal_open = false;
                                    this.open_today_daily_note(window, cx);
                                })),
                        )
                        .child(
                            h_flex()
                                .gap_1p5()
                                .child(
                                    Button::new("nn-cancel")
                                        .ghost()
                                        .small()
                                        .label("Cancel")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.new_note_modal_open = false;
                                            window.blur(cx);
                                            gpui_mobile::hide_keyboard();
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("nn-create")
                                        .primary()
                                        .small()
                                        .label("Create")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.submit_new_note_modal(window, cx);
                                        })),
                                ),
                        ),
                ),
        )
}

pub fn render_more_sheet(
    app: &VKarmaMobileApp,
    bottom_inset: f32,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let Some(note) = &app.active_note else {
        return div().into_any_element();
    };
    let stats_line = format!(
        "{} words · {} lines · {} chars",
        note.stats.0, note.stats.2, note.stats.1
    );
    let edit_label = if note.reading_only {
        "Switch to Live Edit"
    } else {
        "Lock Reading View"
    };
    let delete_armed = note.delete_armed;

    div()
        .id("obs-more-scrim")
        .size_full()
        .absolute()
        .inset_0()
        .occlude()
        .bg(hex_alpha(0x00000090))
        .flex()
        .flex_col()
        .justify_end()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, window, cx| {
            cx.stop_propagation();
            this.more_sheet_open = false;
            this.rename_mode = false;
            window.blur(cx);
            gpui_mobile::hide_keyboard();
            cx.notify();
        }))
        .child(
            v_flex()
                .id("obs-more-panel")
                .w_full()
                .p_4()
                .pb(px(bottom_inset + 16.0))
                .gap_2p5()
                .rounded_t(px(16.))
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_t_1()
                .border_color(hex_color(kd::BORDER))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    v_flex()
                        .pb_2()
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .gap_0p5()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(hex_color(kd::FG))
                                .truncate()
                                .child(note.filename.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(hex_color(kd::FG_MUTED))
                                .child(stats_line),
                        ),
                )
                .when(app.rename_mode, |col| {
                    col.child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(div().flex_1().min_w_0().child(Input::new(&app.rename_note_input).small()))
                            .child(
                                Button::new("more-rename-save")
                                    .primary()
                                    .small()
                                    .label("Rename")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.submit_rename_note(window, cx);
                                    })),
                            ),
                    )
                })
                .when(!app.rename_mode, |col| {
                    col.child(sheet_action_row(
                        "more-rename",
                        "Rename note",
                        false,
                        cx.listener(|this, _, window, cx| {
                            this.begin_rename_active_note(window, cx);
                        }),
                    ))
                })
                .child(sheet_action_row(
                    "more-find",
                    "Find in note",
                    false,
                    cx.listener(|this, _, _, cx| {
                        if let Some(n) = &mut this.active_note {
                            n.find_open = !n.find_open;
                        }
                        this.more_sheet_open = false;
                        cx.notify();
                    }),
                ))
                .child(sheet_action_row(
                    "more-mode",
                    edit_label,
                    false,
                    cx.listener(|this, _, window, cx| {
                        this.more_sheet_open = false;
                        this.toggle_note_reading_mode(window, cx);
                    }),
                ))
                .child(sheet_action_row(
                    "more-daily",
                    "Open today's daily note",
                    false,
                    cx.listener(|this, _, window, cx| {
                        this.more_sheet_open = false;
                        this.open_today_daily_note(window, cx);
                    }),
                ))
                .child(sheet_action_row(
                    "more-home",
                    "Close note tab",
                    false,
                    cx.listener(|this, _, window, cx| {
                        this.close_active_note_to_home(window, cx);
                    }),
                ))
                .child(sheet_action_row(
                    "more-delete",
                    if delete_armed {
                        "Tap again to confirm delete"
                    } else {
                        "Delete note"
                    },
                    true,
                    cx.listener(|this, _, window, cx| {
                        this.delete_active_note(window, cx);
                    }),
                )),
        )
        .into_any_element()
}

fn sheet_action_row(
    id: &'static str,
    label: &'static str,
    danger: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .w_full()
        .h(px(44.))
        .px_3p5()
        .rounded(px(10.))
        .cursor_pointer()
        .bg(if danger {
            hex_alpha(0xc4746e20)
        } else {
            hex_color(kd::BG_PANEL)
        })
        .flex()
        .items_center()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(hex_color(if danger { kd::RED } else { kd::FG }))
        .on_click(on_click)
        .child(label)
}

pub fn render_item_context_menu(
    app: &VKarmaMobileApp,
    bottom_inset: f32,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let Some(target) = app.item_context_menu.clone() else {
        return div().into_any_element();
    };
    let (is_dir, name, rel_path, item_path) = match &target {
        ItemContextTarget::Note {
            path,
            rel_path,
            display_name,
        } => (false, display_name.clone(), rel_path.clone(), path.clone()),
        ItemContextTarget::Folder {
            path,
            rel_path,
            display_name,
        } => (true, display_name.clone(), rel_path.clone(), path.clone()),
    };
    let delete_armed = app.context_delete_armed;

    div()
        .id("obs-item-ctx-scrim")
        .size_full()
        .absolute()
        .inset_0()
        .occlude()
        .bg(hex_alpha(0x00000090))
        .flex()
        .flex_col()
        .justify_end()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, window, cx| {
            cx.stop_propagation();
            this.close_item_context_menu(window, cx);
        }))
        .child(
            v_flex()
                .id("obs-item-ctx-panel")
                .w_full()
                .p_4()
                .pb(px(bottom_inset + 16.0))
                .gap_2()
                .rounded_t(px(16.))
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_t_1()
                .border_color(hex_color(kd::BORDER))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    h_flex()
                        .pb_2()
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .gap_2p5()
                        .items_center()
                        .child(svg_icon(
                            if is_dir {
                                IconName::Folder
                            } else {
                                IconName::FileText
                            },
                            18.,
                            kd::ACCENT_HOVER,
                        ))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(hex_color(kd::FG))
                                        .truncate()
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .truncate()
                                        .child(rel_path),
                                ),
                        ),
                )
                .when(app.context_rename_mode, |col| {
                    col.child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&app.rename_note_input).small()),
                            )
                            .child(
                                Button::new("ctx-rename-save")
                                    .primary()
                                    .small()
                                    .label("Save")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.submit_context_rename(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("ctx-rename-cancel")
                                    .ghost()
                                    .small()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.context_rename_mode = false;
                                        window.blur(cx);
                                        gpui_mobile::hide_keyboard();
                                        cx.notify();
                                    })),
                            ),
                    )
                })
                .when(!app.context_rename_mode && !is_dir, |col| {
                    let p_open = item_path.clone();
                    col.child(sheet_action_row(
                        "ictx-open",
                        "Open Note",
                        false,
                        cx.listener(move |this, _, window, cx| {
                            this.item_context_menu = None;
                            this.context_delete_armed = false;
                            this.open_note(p_open.clone(), false, true, window, cx);
                        }),
                    ))
                    .child(sheet_action_row(
                        "ictx-rename",
                        "Rename",
                        false,
                        cx.listener(|this, _, window, cx| {
                            this.begin_context_rename(window, cx);
                        }),
                    ))
                    .child(sheet_action_row(
                        "ictx-wikilink",
                        "Copy [[Wikilink]]",
                        false,
                        cx.listener(|this, _, window, cx| {
                            this.copy_context_wikilink(window, cx);
                        }),
                    ))
                    .child(sheet_action_row(
                        "ictx-duplicate",
                        "Duplicate",
                        false,
                        cx.listener(|this, _, window, cx| {
                            this.duplicate_context_note(window, cx);
                        }),
                    ))
                    .child(sheet_action_row(
                        "ictx-delete-note",
                        if delete_armed {
                            "Tap again to confirm delete"
                        } else {
                            "Delete Note"
                        },
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.delete_context_item(window, cx);
                        }),
                    ))
                })
                .when(!app.context_rename_mode && is_dir, |col| {
                    col.child(sheet_action_row(
                        "ictx-new-note",
                        "New Note Here",
                        false,
                        cx.listener(|this, _, window, cx| {
                            this.context_create_in_folder(DrawerCreateKind::Note, window, cx);
                        }),
                    ))
                    .child(sheet_action_row(
                        "ictx-new-dir",
                        "New Subfolder",
                        false,
                        cx.listener(|this, _, window, cx| {
                            this.context_create_in_folder(DrawerCreateKind::Folder, window, cx);
                        }),
                    ))
                    .child(sheet_action_row(
                        "ictx-rename-dir",
                        "Rename Folder",
                        false,
                        cx.listener(|this, _, window, cx| {
                            this.begin_context_rename(window, cx);
                        }),
                    ))
                    .child(sheet_action_row(
                        "ictx-delete-dir",
                        if delete_armed {
                            "Tap again to delete folder & contents"
                        } else {
                            "Delete Folder"
                        },
                        true,
                        cx.listener(|this, _, window, cx| {
                            this.delete_context_item(window, cx);
                        }),
                    ))
                })
                .child(
                    Button::new("ictx-cancel")
                        .ghost()
                        .small()
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_item_context_menu(window, cx);
                        })),
                ),
        )
        .into_any_element()
}
