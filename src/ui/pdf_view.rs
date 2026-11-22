use super::{PaneSide, TabKind, VKarmaApp};
use crate::theme::{hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ObjectFit, ParentElement, SharedString, Styled,
    StyledImage as _, div, img, prelude::FluentBuilder as _, px,
};
use std::process::{Command, Stdio};

pub fn render_pdf_tab(
    side: PaneSide,
    tab: &super::DocumentTab,
    zen_mode: bool,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Pdf {
        zoom,
        pages,
        total_pages,
        render_ms,
        last_error,
        rendering,
        file_size_bytes,
        ..
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let open_path = tab.path.clone();
    let zoom = *zoom;
    let total_pages = *total_pages;
    let render_ms = *render_ms;
    let rendering = *rendering;
    let last_error = last_error.clone();
    let pages = pages.clone();
    let kb = *file_size_bytes as f64 / 1024.0;

    let status_pill = if rendering {
        div()
            .px_2()
            .py_0p5()
            .rounded(px(4.))
            .bg(hex_alpha(0xc4b28a26))
            .text_xs()
            .text_color(hex_color(kd::YELLOW))
            .child("Rendering…")
    } else if last_error.is_some() {
        div()
            .px_2()
            .py_0p5()
            .rounded(px(4.))
            .bg(hex_alpha(0xc4746e26))
            .text_xs()
            .text_color(hex_color(kd::RED))
            .child("Render Error")
    } else if let Some(ms) = render_ms {
        div()
            .px_2()
            .py_0p5()
            .rounded(px(4.))
            .bg(hex_alpha(0x87a98726))
            .text_xs()
            .text_color(hex_color(kd::GREEN))
            .child(format!("{total_pages}p · {kb:.1} KB · {ms}ms"))
    } else {
        div()
            .px_2()
            .py_0p5()
            .rounded(px(4.))
            .bg(hex_color(kd::BG_PANEL))
            .text_xs()
            .text_color(hex_color(kd::RED))
            .child(format!("{kb:.1} KB"))
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
                .child(status_pill),
        )
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .flex_shrink_0()
                .child(
                    h_flex()
                        .gap_0p5()
                        .items_center()
                        .child(
                            Button::new(SharedString::from(format!("pdf-zout-{tab_id}")))
                                .ghost()
                                .xsmall()
                                .icon(IconName::ZoomOut)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(t) = this.find_tab_mut(side, tab_id)
                                        && let TabKind::Pdf { zoom, .. } = &mut t.kind
                                    {
                                        *zoom = (*zoom - 0.15).clamp(0.4, 2.5);
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("pdf-zrst-{tab_id}")))
                                .ghost()
                                .xsmall()
                                .label(format!("{}%", (zoom * 100.0).round() as i32))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(t) = this.find_tab_mut(side, tab_id)
                                        && let TabKind::Pdf { zoom, .. } = &mut t.kind
                                    {
                                        *zoom = 1.0;
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("pdf-zin-{tab_id}")))
                                .ghost()
                                .xsmall()
                                .icon(IconName::ZoomIn)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(t) = this.find_tab_mut(side, tab_id)
                                        && let TabKind::Pdf { zoom, .. } = &mut t.kind
                                    {
                                        *zoom = (*zoom + 0.15).clamp(0.4, 2.5);
                                        cx.notify();
                                    }
                                })),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("pdf-reload-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::RefreshCw)
                        .label("Reload")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.reload_pdf_tab(side, tab_id, cx);
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("pdf-ext-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .icon(IconName::ExternalLink)
                        .label("Open External")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let spawn_res = Command::new("xdg-open")
                                .arg(&open_path)
                                .stdin(Stdio::null())
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .spawn();
                            if let Err(err) = spawn_res {
                                this.toast_err(
                                    format!("Failed to open external PDF viewer: {err}"),
                                    window,
                                    cx,
                                );
                            }
                        })),
                ),
        );

    let page_w = px(640.0 * zoom);
    let shown_pages = pages.len();
    let has_more_pages = !rendering && total_pages > shown_pages;

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_DEEP))
        .when(!zen_mode, |this| this.child(header))
        .when_some(last_error, |this, err| {
            this.child(
                div()
                    .w_full()
                    .max_h(px(140.))
                    .px_3()
                    .py_2()
                    .bg(hex_alpha(0x3b2322ee))
                    .border_b_1()
                    .border_color(hex_color(kd::RED))
                    .text_xs()
                    .text_color(hex_color(kd::RED))
                    .child(err),
            )
        })
        .child(
            div()
                .id(SharedString::from(format!("pdf-preview-scroll-{side:?}-{tab_id}")))
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scrollbar()
                .p_6()
                .child(if pages.is_empty() {
                    div()
                        .w_full()
                        .py_12()
                        .flex()
                        .justify_center()
                        .text_sm()
                        .text_color(hex_color(kd::FG_MUTED))
                        .child(if rendering {
                            "Rendering PDF pages…"
                        } else {
                            "No pages to display."
                        })
                        .into_any_element()
                } else {
                    v_flex()
                        .w_full()
                        .gap_5()
                        .items_center()
                        .children(pages.into_iter().enumerate().map(|(ix, page)| {
                            v_flex()
                                .gap_1p5()
                                .items_center()
                                .child(
                                    div()
                                        .bg(hex_color(0xffffff))
                                        .rounded(px(4.))
                                        .shadow_lg()
                                        .overflow_hidden()
                                        .child(
                                            img(page)
                                                .w(page_w)
                                                .object_fit(ObjectFit::Contain),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .child(format!("Page {} / {total_pages}", ix + 1)),
                                )
                        }))
                        .when(has_more_pages, |this| {
                            this.child(
                                h_flex()
                                    .gap_3()
                                    .items_center()
                                    .px_4()
                                    .py_2p5()
                                    .rounded(px(8.))
                                    .bg(hex_color(kd::BG_PANEL))
                                    .border_1()
                                    .border_color(hex_color(kd::BORDER_SUBTLE))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(hex_color(kd::FG_MUTED))
                                            .child(format!(
                                                "Showing first {shown_pages} of {total_pages} pages"
                                            )),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "pdf-load-all-{tab_id}"
                                        )))
                                        .primary()
                                        .small()
                                        .label(format!("Load All {total_pages} Pages"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.load_all_pdf_pages(side, tab_id, cx);
                                        })),
                                    ),
                            )
                        })
                        .into_any_element()
                }),
        )
        .into_any_element()
}
