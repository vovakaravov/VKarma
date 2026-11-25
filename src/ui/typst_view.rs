use super::{PaneSide, TabKind, TypstViewMode, VKarmaApp};
use crate::theme::{hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::Editor;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ObjectFit, ParentElement, SharedString, Styled,
    StyledImage as _, div, img, prelude::FluentBuilder as _, px,
};

pub fn render_typst_tab(
    app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Typst {
        editor,
        find_bar,
        view_mode,
        zoom,
        pages,
        compile_ms,
        last_error,
        compiling,
        ..
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let dirty = tab.dirty;
    let view_mode = *view_mode;
    let find_is_open = find_bar.read(cx).is_open();
    let fb_btn = find_bar.clone();
    let zoom = *zoom;
    let compile_ms = *compile_ms;
    let compiling = *compiling;
    let last_error = last_error.clone();
    let pages = pages.clone();

    let status_pill = if compiling {
        div()
            .px_2()
            .py_0p5()
            .rounded(px(4.))
            .bg(hex_alpha(0xc4b28a26))
            .text_xs()
            .text_color(hex_color(kd::YELLOW))
            .child("Compiling…")
    } else if last_error.is_some() {
        div()
            .px_2()
            .py_0p5()
            .rounded(px(4.))
            .bg(hex_alpha(0xc4746e26))
            .text_xs()
            .text_color(hex_color(kd::RED))
            .child("Syntax Error")
    } else if let Some(ms) = compile_ms {
        div()
            .px_2()
            .py_0p5()
            .rounded(px(4.))
            .bg(hex_alpha(0x87a98726))
            .text_xs()
            .text_color(hex_color(kd::GREEN))
            .child(format!("{}p · {ms}ms", pages.len()))
    } else {
        div()
            .px_2()
            .py_0p5()
            .rounded(px(4.))
            .bg(hex_color(kd::BG_PANEL))
            .text_xs()
            .text_color(hex_color(kd::FG_MUTED))
            .child("Typst")
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
                        .p_0p5()
                        .rounded(px(6.))
                        .bg(hex_color(kd::BG_DEEP))
                        .border_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .gap_0p5()
                        .child(typ_mode_btn(
                            format!("typ-edit-{tab_id}"),
                            "Edit",
                            IconName::Pencil,
                            view_mode == TypstViewMode::Edit,
                            side,
                            tab_id,
                            TypstViewMode::Edit,
                            cx,
                        ))
                        .child(typ_mode_btn(
                            format!("typ-split-{tab_id}"),
                            "Split",
                            IconName::Split,
                            view_mode == TypstViewMode::Split,
                            side,
                            tab_id,
                            TypstViewMode::Split,
                            cx,
                        ))
                        .child(typ_mode_btn(
                            format!("typ-preview-{tab_id}"),
                            "Preview",
                            IconName::Eye,
                            view_mode == TypstViewMode::Preview,
                            side,
                            tab_id,
                            TypstViewMode::Preview,
                            cx,
                        )),
                )
                .child(
                    h_flex()
                        .gap_0p5()
                        .items_center()
                        .child(
                            Button::new(SharedString::from(format!("typ-zout-{tab_id}")))
                                .ghost()
                                .xsmall()
                                .icon(IconName::ZoomOut)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(t) = this.find_tab_mut(side, tab_id)
                                        && let TabKind::Typst { zoom, .. } = &mut t.kind
                                    {
                                        *zoom = (*zoom - 0.15).clamp(0.4, 2.5);
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("typ-zrst-{tab_id}")))
                                .ghost()
                                .xsmall()
                                .label(format!("{}%", (zoom * 100.0).round() as i32))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(t) = this.find_tab_mut(side, tab_id)
                                        && let TabKind::Typst { zoom, .. } = &mut t.kind
                                    {
                                        *zoom = 1.0;
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("typ-zin-{tab_id}")))
                                .ghost()
                                .xsmall()
                                .icon(IconName::ZoomIn)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(t) = this.find_tab_mut(side, tab_id)
                                        && let TabKind::Typst { zoom, .. } = &mut t.kind
                                    {
                                        *zoom = (*zoom + 0.15).clamp(0.4, 2.5);
                                        cx.notify();
                                    }
                                })),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("typ-find-{tab_id}")))
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
                                && let TabKind::Typst { view_mode, .. } = &mut t.kind
                                && *view_mode == TypstViewMode::Preview
                            {
                                *view_mode = TypstViewMode::Split;
                            }
                            fb_btn.update(cx, |b, cx| b.open(false, window, cx));
                            cx.notify();
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("typ-pdf-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .icon(IconName::ExternalLink)
                        .label("Export PDF")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.export_typst_current_pdf(side, tab_id, window, cx);
                        })),
                )
                .when(dirty, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("typ-save-{tab_id}")))
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

    let zen_mode = app.zen_mode;
    let editor_el = super::find_bar::wrap_editor_with_find(
        super::zen_editor_frame(
            Editor::new(editor)
                .size_full()
                .appearance(false)
                .bordered(false),
            zen_mode && view_mode == TypstViewMode::Edit,
        ),
        find_bar,
        cx,
    );

    let preview_el = render_typst_preview(side, tab_id, &pages, last_error, zoom, compiling);

    let body = match view_mode {
        TypstViewMode::Edit => editor_el,
        TypstViewMode::Preview => preview_el.into_any_element(),
        TypstViewMode::Split => {
            h_resizable(SharedString::from(format!("typ-split-{side:?}-{tab_id}")))
                .child(
                    resizable_panel()
                        .size_range(px(240.)..px(2000.))
                        .child(editor_el),
                )
                .child(
                    resizable_panel()
                        .size_range(px(240.)..px(2000.))
                        .child(preview_el),
                )
                .into_any_element()
        }
    };

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .when(!zen_mode, |this| this.child(header))
        .child(div().flex_1().min_h_0().w_full().child(body))
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn typ_mode_btn(
    id: String,
    label: &'static str,
    icon: IconName,
    active: bool,
    side: PaneSide,
    tab_id: u64,
    target_mode: TypstViewMode,
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
        .on_click(cx.listener(move |this, _, _, cx| {
            this.active_pane = side;
            if let Some(tab) = this.find_tab_mut(side, tab_id)
                && let TabKind::Typst { view_mode, .. } = &mut tab.kind
            {
                *view_mode = target_mode;
                cx.notify();
            }
        }))
}

fn render_typst_preview(
    side: PaneSide,
    tab_id: u64,
    pages: &[std::sync::Arc<gpui_kit::Image>],
    last_error: Option<String>,
    zoom: f32,
    compiling: bool,
) -> impl IntoElement {
    let total_pages = pages.len();
    let page_w = px(640.0 * zoom);

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_DEEP))
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
                .id(SharedString::from(format!("typ-preview-scroll-{side:?}-{tab_id}")))
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
                        .child(if compiling {
                            "Compiling Typst document…"
                        } else {
                            "No rendered pages yet."
                        })
                        .into_any_element()
                } else {
                    v_flex()
                        .w_full()
                        .gap_5()
                        .items_center()
                        .children(pages.iter().enumerate().map(|(ix, page)| {
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
                                            img(page.clone())
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
                        .into_any_element()
                }),
        )
}
