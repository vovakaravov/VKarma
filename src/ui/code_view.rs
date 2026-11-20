use super::{PaneSide, TabKind, VKarmaApp};
use crate::theme::{hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::Editor;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    ClipboardItem, Context, InteractiveElement as _, IntoElement, ObjectFit, ParentElement,
    SharedString, Styled, StyledImage as _, div, img, prelude::FluentBuilder as _, px, relative,
};

pub fn render_code_tab(
    side: PaneSide,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Code {
        editor,
        language,
        badge,
        soft_wrap,
        ..
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let dirty = tab.dirty;
    let wrap = *soft_wrap;

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
                .gap_1p5()
                .items_center()
                .child(
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

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .child(header)
        .child(
            div().flex_1().min_h_0().w_full().child(
                Editor::new(editor)
                    .size_full()
                    .appearance(false)
                    .bordered(false),
            ),
        )
        .into_any_element()
}

pub fn render_image_tab(
    tab: &super::DocumentTab,
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
        .child(header)
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
