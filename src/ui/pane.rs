use super::{
    NewItemTemplate, PaneSide, TabKind, VKarmaApp, archive_view, audio_view, canvas_view,
    code_view, epub_view, font_view, fountain_view, kanban_view, markdown_view, mermaid_view,
    modals, office_view, pdf_view, spreadsheet_view, sqlite_view, subtitle_view, svg_view,
    tab_badge_and_color, typst_view,
};
use crate::theme::{hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
};

pub fn render_pane(
    app: &VKarmaApp,
    side: PaneSide,
    window: &mut Window,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let is_active_pane = app.active_pane == side;
    let border = hex_color(kd::BORDER_SUBTLE);
    let tab_bar_bg = hex_color(kd::BG_DEEP);

    let Some(pane) = app.pane(side) else {
        return div().into_any_element();
    };

    let tab_items = pane.tabs.iter().enumerate().map(|(ix, tab)| {
        let is_selected = ix == pane.active_tab;
        let (badge, badge_color) = tab_badge_and_color(tab);
        let tab_id_num = tab.id;
        let title = tab.title.clone();
        let dirty = tab.dirty;
        let el_id: SharedString = format!("pane-{side:?}-tab-{tab_id_num}").into();

        div()
            .id(el_id)
            .h(px(30.))
            .px_2p5()
            .rounded_t(px(6.))
            .flex()
            .items_center()
            .gap_1p5()
            .cursor_pointer()
            .border_t_2()
            .border_color(if is_selected && is_active_pane {
                hex_color(kd::ACCENT)
            } else {
                gpui_kit::transparent_black()
            })
            .bg(if is_selected {
                hex_color(kd::BG_MAIN)
            } else {
                tab_bar_bg
            })
            .hover(|s| {
                if is_selected {
                    s
                } else {
                    s.bg(hex_color(kd::BG_SIDEBAR))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.activate_tab(side, ix, window, cx);
            }))
            .on_mouse_down(
                gpui_kit::MouseButton::Middle,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.close_tab(side, ix, cx);
                }),
            )
            .child(
                div()
                    .px_1()
                    .rounded(px(3.))
                    .bg(hex_color(kd::BG_PANEL))
                    .text_size(px(9.5))
                    .font_weight(gpui_kit::FontWeight::BOLD)
                    .text_color(badge_color)
                    .child(badge),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if is_selected {
                        hex_color(kd::FG)
                    } else {
                        hex_color(kd::FG_MUTED)
                    })
                    .max_w(px(180.))
                    .truncate()
                    .child(title),
            )
            .when(dirty, |this| {
                this.child(
                    div()
                        .w(px(7.))
                        .h(px(7.))
                        .rounded_full()
                        .bg(hex_color(kd::YELLOW)),
                )
            })
            .child(
                div()
                    .id(SharedString::from(format!("close-tab-{tab_id_num}")))
                    .ml_0p5()
                    .p_0p5()
                    .rounded(px(4.))
                    .hover(|s| s.bg(hex_color(kd::BG_BTN)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.close_tab(side, ix, cx);
                    }))
                    .child(
                        Icon::new(IconName::X)
                            .xsmall()
                            .text_color(hex_color(kd::FG_MUTED)),
                    ),
            )
    });

    let tab_bar = h_flex()
        .h(px(34.))
        .w_full()
        .px_1p5()
        .items_end()
        .justify_between()
        .bg(tab_bar_bg)
        .border_b_1()
        .border_color(border)
        .child(
            h_flex()
                .id(SharedString::from(format!("tab-strip-{side:?}")))
                .flex_1()
                .min_w_0()
                .gap_1()
                .overflow_x_scrollbar()
                .children(tab_items),
        )
        .when(side == PaneSide::Right, |this| {
            this.child(
                Button::new("close-right-pane-btn")
                    .ghost()
                    .xsmall()
                    .icon(IconName::X)
                    .tooltip("Close Right Split")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.right_pane = None;
                        this.active_pane = PaneSide::Left;
                        this.persist_workspaces();
                        cx.notify();
                    })),
            )
        });

    let active_content = match pane.active() {
        Some(tab) => match &tab.kind {
            TabKind::Markdown { .. } => {
                markdown_view::render_markdown_tab(app, side, tab, cx).into_any_element()
            }
            TabKind::Canvas { .. } => {
                canvas_view::render_canvas_tab(app, side, tab, cx).into_any_element()
            }
            TabKind::Kanban { .. } => {
                kanban_view::render_kanban_tab(app, side, tab, cx).into_any_element()
            }
            TabKind::Subtitle { .. } => {
                subtitle_view::render_subtitle_tab(app, side, tab, cx).into_any_element()
            }
            TabKind::Svg { .. } => {
                svg_view::render_svg_tab(app, side, tab, cx).into_any_element()
            }
            TabKind::Mermaid { .. } => {
                mermaid_view::render_mermaid_tab(app, side, tab, cx).into_any_element()
            }
            TabKind::Office { .. } => {
                office_view::render_office_tab(app, side, tab, cx).into_any_element()
            }
            TabKind::Epub { .. } => {
                epub_view::render_epub_tab(app, tab, cx).into_any_element()
            }
            TabKind::Sqlite { .. } => {
                sqlite_view::render_sqlite_tab(app, tab, cx).into_any_element()
            }
            TabKind::Archive { .. } => {
                archive_view::render_archive_tab(app, tab, cx).into_any_element()
            }
            TabKind::Audio { .. } => {
                audio_view::render_audio_tab(app, tab, cx).into_any_element()
            }
            TabKind::Font { .. } => {
                font_view::render_font_tab(app, tab, cx).into_any_element()
            }
            TabKind::Typst { .. } => {
                typst_view::render_typst_tab(app, side, tab, cx).into_any_element()
            }
            TabKind::Fountain { .. } => {
                fountain_view::render_fountain_tab(app, side, tab, cx).into_any_element()
            }
            TabKind::Spreadsheet { .. } => {
                spreadsheet_view::render_spreadsheet_tab(app, side, tab, window, cx)
                    .into_any_element()
            }
            TabKind::Code { .. } => {
                code_view::render_code_tab(side, tab, app.zen_mode, cx).into_any_element()
            }
            TabKind::Image { .. } => {
                code_view::render_image_tab(tab, app.zen_mode, cx).into_any_element()
            }
            TabKind::Pdf { .. } => {
                pdf_view::render_pdf_tab(side, tab, app.zen_mode, cx).into_any_element()
            }
        },
        None => render_empty_pane(app, side, cx).into_any_element(),
    };

    v_flex()
        .id(SharedString::from(format!("workspace-pane-{side:?}")))
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .on_mouse_down(
            gpui_kit::MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                if this.active_pane != side {
                    this.active_pane = side;
                    cx.notify();
                }
            }),
        )
        .when(!app.zen_mode, |this| this.child(tab_bar))
        .child(div().flex_1().min_h_0().w_full().child(active_content))
        .into_any_element()
}

fn render_empty_pane(
    app: &VKarmaApp,
    side: PaneSide,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let ws_root = app.workspace_root.clone();
    let entity = cx.entity();

    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_6()
        .child(
            v_flex()
                .max_w(px(440.))
                .p_6()
                .gap_4()
                .rounded(px(10.))
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_1()
                .border_color(hex_color(kd::BORDER_SUBTLE))
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(gpui_kit::FontWeight::BOLD)
                                .text_color(hex_color(kd::ACCENT))
                                .child("VKarma"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(hex_color(kd::FG_MUTED))
                                .child(format!(
                                    "{} Notes indexed in {}",
                                    app.vault.md_count,
                                    app.workspace_root.display()
                                )),
                        ),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            Button::new(SharedString::from(format!("empty-quick-open-{side:?}")))
                                .primary()
                                .small()
                                .icon(IconName::Search)
                                .label("Quick Open File…  (Ctrl+P)")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.active_pane = side;
                                    this.open_quick_switcher(window, cx);
                                })),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new(SharedString::from(format!("empty-new-md-{side:?}")))
                                        .small()
                                        .flex_1()
                                        .icon(IconName::FileText)
                                        .label("New Note")
                                        .on_click({
                                            let dir = ws_root.clone();
                                            let ent = entity.clone();
                                            cx.listener(move |this, _, window, cx| {
                                                this.active_pane = side;
                                                modals::open_new_item_dialog(
                                                    ent.clone(),
                                                    dir.clone(),
                                                    NewItemTemplate::Markdown,
                                                    window,
                                                    cx,
                                                );
                                            })
                                        }),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("empty-new-typ-{side:?}")))
                                        .small()
                                        .flex_1()
                                        .icon(IconName::BookOpen)
                                        .label("New Typst")
                                        .on_click({
                                            let dir = ws_root.clone();
                                            let ent = entity.clone();
                                            cx.listener(move |this, _, window, cx| {
                                                this.active_pane = side;
                                                modals::open_new_item_dialog(
                                                    ent.clone(),
                                                    dir.clone(),
                                                    NewItemTemplate::Typst,
                                                    window,
                                                    cx,
                                                );
                                            })
                                        }),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("empty-new-xls-{side:?}")))
                                        .small()
                                        .flex_1()
                                        .icon(IconName::FileSpreadsheet)
                                        .label("New Sheet")
                                        .on_click({
                                            let dir = ws_root.clone();
                                            let ent = entity.clone();
                                            cx.listener(move |this, _, window, cx| {
                                                this.active_pane = side;
                                                modals::open_new_item_dialog(
                                                    ent.clone(),
                                                    dir.clone(),
                                                    NewItemTemplate::Spreadsheet,
                                                    window,
                                                    cx,
                                                );
                                            })
                                        }),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("empty-new-ftn-{side:?}")))
                                        .small()
                                        .flex_1()
                                        .icon(IconName::FileText)
                                        .label("New Script")
                                        .on_click({
                                            let dir = ws_root.clone();
                                            let ent = entity.clone();
                                            cx.listener(move |this, _, window, cx| {
                                                this.active_pane = side;
                                                modals::open_new_item_dialog(
                                                    ent.clone(),
                                                    dir.clone(),
                                                    NewItemTemplate::Fountain,
                                                    window,
                                                    cx,
                                                );
                                            })
                                        }),
                                ),
                        ),
                )
                .child(
                    div()
                        .pt_2()
                        .border_t_1()
                        .border_color(hex_alpha(0x39383680))
                        .text_xs()
                        .text_color(hex_color(kd::FG_MUTED))
                        .child("Ctrl+P Quick Open · Ctrl+E View Mode · Ctrl+M Source Mode · Ctrl+\\ Mix Panes Side-by-Side"),
                ),
        )
}
