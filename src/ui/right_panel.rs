use super::{RightPanelTab, TabKind, VKarmaApp};
use crate::theme::{hex_color, kd};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement, StatefulInteractiveElement as _,
    Styled, div, px,
};

pub fn render_right_panel(app: &VKarmaApp, cx: &mut Context<VKarmaApp>) -> impl IntoElement {
    let sidebar_bg = hex_color(kd::BG_SIDEBAR);
    let border = hex_color(kd::BORDER_SUBTLE);
    let fg_muted = hex_color(kd::FG_MUTED);

    let header = h_flex()
        .h(px(36.))
        .w_full()
        .px_2()
        .gap_1()
        .items_center()
        .border_b_1()
        .border_color(border)
        .child(
            Button::new("rp-tab-outline")
                .with_variant(if app.right_panel_tab == RightPanelTab::Outline {
                    ButtonVariant::Secondary
                } else {
                    ButtonVariant::Ghost
                })
                .xsmall()
                .label("Outline")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.right_panel_tab = RightPanelTab::Outline;
                    cx.notify();
                })),
        )
        .child(
            Button::new("rp-tab-backlinks")
                .with_variant(if app.right_panel_tab == RightPanelTab::Backlinks {
                    ButtonVariant::Secondary
                } else {
                    ButtonVariant::Ghost
                })
                .xsmall()
                .label("Backlinks")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.right_panel_tab = RightPanelTab::Backlinks;
                    cx.notify();
                })),
        );

    let active_tab = app.active_tab_ref();

    let body = match app.right_panel_tab {
        RightPanelTab::Outline => {
            let headings = match active_tab {
                Some(t) => match &t.kind {
                    TabKind::Markdown { headings, .. }
                    | TabKind::Typst { headings, .. }
                    | TabKind::Fountain { headings, .. } => headings.clone(),
                    TabKind::Office { doc, .. } => doc.headings.clone(),
                    TabKind::Epub { view, .. } => view.read(cx).outline_headings(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            };

            if headings.is_empty() {
                div()
                    .p_3()
                    .text_xs()
                    .text_color(fg_muted)
                    .child("No headings in active document.")
                    .into_any_element()
            } else {
                v_flex()
                    .gap_0p5()
                    .children(headings.into_iter().enumerate().map(|(ix, h)| {
                        let indent = ((h.level.saturating_sub(1)) as f32) * 12.0 + 8.0;
                        let line = h.line;
                        div()
                            .id(("outline-heading", ix))
                            .w_full()
                            .py_1()
                            .pl(px(indent))
                            .pr_2()
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(|s| s.bg(hex_color(kd::BG_PANEL)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.jump_to_heading_line(line, window, cx);
                            }))
                            .child(
                                h_flex()
                                    .justify_between()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(if h.level == 1 {
                                                hex_color(kd::BLUE)
                                            } else {
                                                hex_color(kd::FG)
                                            })
                                            .truncate()
                                            .child(h.title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(fg_muted)
                                            .child(format!("L{line}")),
                                    ),
                            )
                    }))
                    .into_any_element()
            }
        }
        RightPanelTab::Backlinks => {
            let backlinks = active_tab
                .and_then(|t| app.vault.backlinks.get(&t.path))
                .cloned()
                .unwrap_or_default();

            if backlinks.is_empty() {
                div()
                    .p_3()
                    .text_xs()
                    .text_color(fg_muted)
                    .child("No incoming links to active note.")
                    .into_any_element()
            } else {
                v_flex()
                    .gap_1()
                    .children(backlinks.into_iter().enumerate().map(|(ix, bl)| {
                        let src = bl.source_path.clone();
                        div()
                            .id(("backlink-item", ix))
                            .w_full()
                            .p_2()
                            .rounded(px(5.))
                            .bg(hex_color(kd::BG_MAIN))
                            .border_1()
                            .border_color(border)
                            .cursor_pointer()
                            .hover(|s| s.bg(hex_color(kd::BG_PANEL)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let pane = this.active_pane;
                                this.open_file(src.clone(), pane, window, cx);
                            }))
                            .child(
                                v_flex()
                                    .gap_0p5()
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                                    .text_color(hex_color(kd::LINK))
                                                    .truncate()
                                                    .child(bl.source_name),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(10.))
                                                    .text_color(fg_muted)
                                                    .child(format!("L{}", bl.line_num)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(hex_color(kd::FG_SUBTLE))
                                            .truncate()
                                            .child(bl.snippet),
                                    ),
                            )
                    }))
                    .into_any_element()
            }
        }
    };

    v_flex()
        .size_full()
        .bg(sidebar_bg)
        .border_l_1()
        .border_color(border)
        .child(header)
        .child(
            div()
                .id("right-panel-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .p_1p5()
                .child(body),
        )
}
