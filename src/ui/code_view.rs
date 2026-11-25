use super::{CodeViewMode, PaneSide, TabKind, VKarmaApp};
use crate::formats::data_tree::{
    DataValueKind, collect_container_paths, extract_table, flatten_tree, is_structured_lang,
    parse_structured_data,
};
use crate::theme::{accent_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Editor, Input};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, ClipboardItem, Context, Focusable as _, FontWeight, InteractiveElement as _,
    IntoElement, ObjectFit, ParentElement, SharedString, StatefulInteractiveElement as _, Styled,
    StyledImage as _, div, img, prelude::FluentBuilder as _, px, relative,
};

pub fn render_code_tab(
    side: PaneSide,
    tab: &super::DocumentTab,
    zen_mode: bool,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Code {
        editor,
        find_bar,
        language,
        badge,
        soft_wrap,
        data_view,
        collapsed_paths,
        tree_filter,
        ..
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let dirty = tab.dirty;
    let wrap = *soft_wrap;
    let view_mode = *data_view;
    let structured = is_structured_lang(language);
    let active_view = if structured {
        view_mode
    } else {
        CodeViewMode::Code
    };
    let find_is_open = find_bar.read(cx).is_open();
    let fb_btn = find_bar.clone();

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
                        .px_1p5()
                        .py_0p5()
                        .rounded(px(4.))
                        .bg(hex_color(kd::BG_PANEL))
                        .text_xs()
                        .text_color(hex_color(kd::YELLOW))
                        .child(format!("{badge} ({language})")),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .when(structured, |row| {
                    row.child(
                        Button::new(SharedString::from(format!("code-mv-code-{tab_id}")))
                            .with_variant(if active_view == CodeViewMode::Code {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .xsmall()
                            .icon(IconName::Code)
                            .label("Code")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(t) = this.find_tab_mut(side, tab_id)
                                    && let TabKind::Code {
                                        data_view, editor, ..
                                    } = &mut t.kind
                                {
                                    *data_view = CodeViewMode::Code;
                                    editor.focus_handle(cx).focus(window, cx);
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("code-mv-tree-{tab_id}")))
                            .with_variant(if active_view == CodeViewMode::Tree {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .xsmall()
                            .label("Tree")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(t) = this.find_tab_mut(side, tab_id)
                                    && let TabKind::Code { data_view, .. } = &mut t.kind
                                {
                                    *data_view = CodeViewMode::Tree;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("code-mv-tbl-{tab_id}")))
                            .with_variant(if active_view == CodeViewMode::Table {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .xsmall()
                            .label("Table")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(t) = this.find_tab_mut(side, tab_id)
                                    && let TabKind::Code { data_view, .. } = &mut t.kind
                                {
                                    *data_view = CodeViewMode::Table;
                                    cx.notify();
                                }
                            })),
                    )
                })
                .when(active_view == CodeViewMode::Code, |row| {
                    row.child(
                        Button::new(SharedString::from(format!("code-wrap-{tab_id}")))
                            .with_variant(if wrap {
                                ButtonVariant::Secondary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .xsmall()
                            .label(if wrap { "Wrap: ON" } else { "Wrap: OFF" })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(t) = this.find_tab_mut(side, tab_id)
                                    && let TabKind::Code {
                                        editor, soft_wrap, ..
                                    } = &mut t.kind
                                {
                                    *soft_wrap = !*soft_wrap;
                                    let next = *soft_wrap;
                                    editor
                                        .clone()
                                        .update(cx, |s, cx| s.set_soft_wrap(next, window, cx));
                                    cx.notify();
                                }
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("code-find-{tab_id}")))
                        .with_variant(if find_is_open {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Search)
                        .tooltip("Find & Replace (Ctrl+F)")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                && let TabKind::Code { data_view, .. } = &mut t.kind
                                && *data_view != CodeViewMode::Code
                            {
                                *data_view = CodeViewMode::Code;
                            }
                            fb_btn.update(cx, |b, cx| b.open(false, window, cx));
                            cx.notify();
                        })),
                )
                .when(dirty, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("code-save-{tab_id}")))
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

    let body: AnyElement = match active_view {
        CodeViewMode::Code => super::find_bar::wrap_editor_with_find(
            super::zen_editor_frame(
                Editor::new(editor)
                    .size_full()
                    .appearance(false)
                    .bordered(false),
                zen_mode,
            ),
            find_bar,
            cx,
        ),
        CodeViewMode::Tree => {
            let raw = editor.read(cx).value().to_string();
            let filter_q = tree_filter
                .as_ref()
                .map(|inp| inp.read(cx).value().to_string())
                .unwrap_or_default();

            match parse_structured_data(language, &raw) {
                Ok(root) => {
                    let rows = flatten_tree(&root, collapsed_paths, &filter_q, 2000);
                    let mut all_containers = std::collections::HashSet::new();
                    collect_container_paths(&root, "", &mut all_containers);

                    let filter_widget = tree_filter.as_ref().map(|inp| {
                        let has_q = !filter_q.is_empty();
                        let inp_clr = inp.clone();
                        let clear_btn = Button::new(SharedString::from(format!(
                            "code-tree-x-{tab_id}"
                        )))
                        .text()
                        .xsmall()
                        .tab_stop(false)
                        .icon(IconName::Close)
                        .tooltip("Clear")
                        .on_click(cx.listener(move |_, _, window, cx| {
                            inp_clr.update(cx, |s, cx| s.set_value("", window, cx));
                            inp_clr.focus_handle(cx).focus(window, cx);
                            cx.notify();
                        }));
                        div()
                            .flex_1()
                            .max_w(px(300.))
                            .child(
                                Input::new(inp)
                                    .xsmall()
                                    .prefix(
                                        Icon::new(IconName::Search)
                                            .xsmall()
                                            .text_color(hex_color(kd::FG_MUTED)),
                                    )
                                    .when(has_q, |i| i.suffix(clear_btn)),
                            )
                            .into_any_element()
                    });

                    let row_els = rows.into_iter().enumerate().map(|(ix, row)| {
                        let indent = (row.depth as f32) * 18.0 + 8.0;
                        let path_toggle = row.path.clone();
                        let copy_val = row.copy_value.clone();
                        let val_color = match row.value_kind {
                            DataValueKind::String => hex_color(kd::GREEN),
                            DataValueKind::Number => hex_color(kd::ORANGE),
                            DataValueKind::Bool => hex_color(kd::VIOLET),
                            DataValueKind::Null => hex_color(kd::FG_MUTED),
                            DataValueKind::Container => hex_color(kd::FG_SUBTLE),
                        };

                        h_flex()
                            .id(SharedString::from(format!("dt-row-{tab_id}-{ix}")))
                            .w_full()
                            .py_1()
                            .pl(px(indent))
                            .pr_3()
                            .gap_2()
                            .items_center()
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(|s| s.bg(hex_color(kd::BG_SIDEBAR)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if row.is_container {
                                    if let Some(t) = this.find_tab_mut(side, tab_id)
                                        && let TabKind::Code {
                                            collapsed_paths, ..
                                        } = &mut t.kind
                                    {
                                        if !collapsed_paths.remove(&path_toggle) {
                                            collapsed_paths.insert(path_toggle.clone());
                                        }
                                        cx.notify();
                                    }
                                } else {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_val.clone(),
                                    ));
                                    this.toast_ok(format!("Copied: {copy_val}"), window, cx);
                                }
                            }))
                            .child(if row.is_container {
                                Icon::new(if row.collapsed {
                                    IconName::ChevronRight
                                } else {
                                    IconName::ChevronDown
                                })
                                .xsmall()
                                .text_color(hex_color(kd::FG_MUTED))
                                .into_any_element()
                            } else {
                                div().w(px(12.)).into_any_element()
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(hex_color(kd::ACCENT))
                                    .child(row.key),
                            )
                            .child(
                                div()
                                    .px_1()
                                    .rounded(px(3.))
                                    .bg(hex_color(kd::BG_PANEL))
                                    .text_size(px(10.))
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child(row.kind_badge),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(val_color)
                                    .truncate()
                                    .child(row.value_preview),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child(row.path),
                            )
                    });

                    v_flex()
                        .size_full()
                        .child(
                            h_flex()
                                .px_3()
                                .py_1p5()
                                .gap_2()
                                .items_center()
                                .border_b_1()
                                .border_color(hex_color(kd::BORDER_SUBTLE))
                                .when_some(filter_widget, |r, w| r.child(w))
                                .child(
                                    Button::new(SharedString::from(format!("dt-exp-{tab_id}")))
                                        .ghost()
                                        .xsmall()
                                        .label("Expand All")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                                && let TabKind::Code {
                                                    collapsed_paths, ..
                                                } = &mut t.kind
                                            {
                                                collapsed_paths.clear();
                                                cx.notify();
                                            }
                                        })),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("dt-col-{tab_id}")))
                                        .ghost()
                                        .xsmall()
                                        .label("Collapse All")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                                && let TabKind::Code {
                                                    collapsed_paths, ..
                                                } = &mut t.kind
                                            {
                                                *collapsed_paths = all_containers.clone();
                                                cx.notify();
                                            }
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("dt-scroll-{tab_id}")))
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scrollbar()
                                .p_2()
                                .child(v_flex().gap_0p5().children(row_els)),
                        )
                        .into_any_element()
                }
                Err(err) => render_parse_error_banner(err, side, tab_id, cx),
            }
        }
        CodeViewMode::Table => {
            let raw = editor.read(cx).value().to_string();
            match parse_structured_data(language, &raw) {
                Ok(root) => match extract_table(&root) {
                    Some((cols, rows)) => {
                        let header_row = h_flex()
                            .bg(hex_color(kd::BG_SIDEBAR))
                            .border_b_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .child(
                                div()
                                    .w(px(52.))
                                    .flex_shrink_0()
                                    .px_2()
                                    .py_1p5()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child("#"),
                            )
                            .children(cols.iter().map(|col| {
                                div()
                                    .w(px(160.))
                                    .flex_shrink_0()
                                    .px_2()
                                    .py_1p5()
                                    .border_l_1()
                                    .border_color(hex_color(kd::BORDER_SUBTLE))
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(hex_color(kd::ACCENT))
                                    .truncate()
                                    .child(col.clone())
                            }));

                        let data_rows = rows.into_iter().enumerate().map(|(rix, cells)| {
                            let bg = if rix % 2 == 1 {
                                accent_alpha(0x0c)
                            } else {
                                gpui_kit::transparent_black()
                            };
                            h_flex()
                                .bg(bg)
                                .border_b_1()
                                .border_color(hex_color(kd::BORDER_SUBTLE))
                                .child(
                                    div()
                                        .w(px(52.))
                                        .flex_shrink_0()
                                        .px_2()
                                        .py_1()
                                        .text_xs()
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .child(format!("{}", rix + 1)),
                                )
                                .children(cells.into_iter().map(|cell| {
                                    div()
                                        .w(px(160.))
                                        .flex_shrink_0()
                                        .px_2()
                                        .py_1()
                                        .border_l_1()
                                        .border_color(hex_color(kd::BORDER_SUBTLE))
                                        .text_xs()
                                        .text_color(hex_color(kd::FG))
                                        .truncate()
                                        .child(cell)
                                }))
                        });

                        let total_table_w = 52.0 + (cols.len() as f32) * 160.0;
                        div()
                            .id(SharedString::from(format!("dt-tbl-x-{tab_id}")))
                            .size_full()
                            .overflow_x_scrollbar()
                            .child(
                                v_flex()
                                    .w(px(total_table_w))
                                    .min_w(relative(1.))
                                    .h_full()
                                    .min_h_0()
                                    .child(header_row)
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("dt-tbl-y-{tab_id}")))
                                            .flex_1()
                                            .min_h_0()
                                            .w_full()
                                            .overflow_y_scrollbar()
                                            .child(v_flex().children(data_rows)),
                                    ),
                            )
                            .into_any_element()
                    }
                    None => div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_6()
                        .text_xs()
                        .text_color(hex_color(kd::FG_MUTED))
                        .child("No array of objects found in this file for Table view — switch to Tree or Code.")
                        .into_any_element(),
                },
                Err(err) => render_parse_error_banner(err, side, tab_id, cx),
            }
        }
    };

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .when(!zen_mode, |this| this.child(header))
        .child(div().flex_1().min_h_0().w_full().child(body))
        .into_any_element()
}

fn render_parse_error_banner(
    err: String,
    side: PaneSide,
    tab_id: u64,
    cx: &mut Context<VKarmaApp>,
) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_6()
        .child(
            v_flex()
                .max_w(px(520.))
                .p_4()
                .gap_3()
                .rounded(px(8.))
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_1()
                .border_color(hex_color(kd::RED))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(hex_color(kd::RED))
                        .child("Parse Error"),
                )
                .child(div().text_xs().text_color(hex_color(kd::FG)).child(err))
                .child(
                    h_flex().justify_end().child(
                        Button::new(SharedString::from(format!("dt-fix-code-{tab_id}")))
                            .secondary()
                            .xsmall()
                            .icon(IconName::Code)
                            .label("Edit in Code View")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(t) = this.find_tab_mut(side, tab_id)
                                    && let TabKind::Code { data_view, .. } = &mut t.kind
                                {
                                    *data_view = CodeViewMode::Code;
                                    cx.notify();
                                }
                            })),
                    ),
                ),
        )
        .into_any_element()
}

pub fn render_image_tab(
    tab: &super::DocumentTab,
    zen_mode: bool,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Image {
        zoom,
        fit_to_view,
        file_size_bytes,
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let path = tab.path.clone();
    let rel_path = tab.rel_path.clone();
    let name_for_embed = tab.title.clone();
    let zoom = *zoom;
    let fit = *fit_to_view;
    let kb = *file_size_bytes as f64 / 1024.0;

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
                        .text_color(hex_color(kd::PINK))
                        .child(format!("{kb:.1} KB")),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new(SharedString::from(format!("img-fit-{tab_id}")))
                        .with_variant(if fit {
                            ButtonVariant::Secondary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .label("Fit")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(t) = this.find_tab_mut(this.active_pane, tab_id)
                                && let TabKind::Image { fit_to_view, .. } = &mut t.kind
                            {
                                *fit_to_view = true;
                                cx.notify();
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("img-zout-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::ZoomOut)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(t) = this.find_tab_mut(this.active_pane, tab_id)
                                && let TabKind::Image {
                                    zoom, fit_to_view, ..
                                } = &mut t.kind
                            {
                                *fit_to_view = false;
                                *zoom = (*zoom - 0.2).clamp(0.2, 4.0);
                                cx.notify();
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("img-100-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .label("100%")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(t) = this.find_tab_mut(this.active_pane, tab_id)
                                && let TabKind::Image {
                                    zoom, fit_to_view, ..
                                } = &mut t.kind
                            {
                                *fit_to_view = false;
                                *zoom = 1.0;
                                cx.notify();
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("img-zin-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::ZoomIn)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(t) = this.find_tab_mut(this.active_pane, tab_id)
                                && let TabKind::Image {
                                    zoom, fit_to_view, ..
                                } = &mut t.kind
                            {
                                *fit_to_view = false;
                                *zoom = (*zoom + 0.2).clamp(0.2, 4.0);
                                cx.notify();
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("img-copy-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .icon(IconName::Copy)
                        .label(format!("Copy ![[{name_for_embed}]]"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let tag = format!("![[{name_for_embed}|center|600]]");
                            cx.write_to_clipboard(ClipboardItem::new_string(tag.clone()));
                            this.toast_ok(format!("Copied {tag}"), window, cx);
                        })),
                ),
        );

    let img_view = img(path)
        .object_fit(ObjectFit::Contain)
        .when(fit, |this| this.max_w(relative(1.)).max_h(relative(1.)))
        .when(!fit, |this| this.w(px(720.0 * zoom)))
        .rounded(px(8.));

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_DEEP))
        .when(!zen_mode, |this| this.child(header))
        .child(
            div()
                .id(SharedString::from(format!("img-scroll-{tab_id}")))
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scrollbar()
                .flex()
                .items_center()
                .justify_center()
                .p_6()
                .child(img_view),
        )
        .into_any_element()
}
