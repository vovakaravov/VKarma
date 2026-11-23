use super::{FountainViewMode, PaneSide, TabKind, VKarmaApp};
use crate::fountain::{FountainDocument, FountainElement};
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::Editor;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    Context, InteractiveElement as _, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder as _, px, relative,
};

pub fn render_fountain_tab(
    app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Fountain {
        editor,
        view_mode,
        paper_light,
        show_notes,
        parsed,
        ..
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let dirty = tab.dirty;
    let view_mode = *view_mode;
    let paper_light = *paper_light;
    let show_notes = *show_notes;
    let scenes = parsed.scene_count;
    let est_pages = parsed.estimated_pages;
    let est_mins = est_pages.round().max(1.0) as usize;
    let doc = parsed.clone();

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
                        .text_color(hex_color(kd::ORANGE))
                        .child(format!(
                            "{scenes} scenes · ~{est_pages:.1}p · ~{est_mins} min"
                        )),
                ),
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
                        .child(ftn_mode_btn(
                            format!("ftn-edit-{tab_id}"),
                            "Edit",
                            IconName::Pencil,
                            view_mode == FountainViewMode::Edit,
                            side,
                            tab_id,
                            FountainViewMode::Edit,
                            cx,
                        ))
                        .child(ftn_mode_btn(
                            format!("ftn-split-{tab_id}"),
                            "Split",
                            IconName::Split,
                            view_mode == FountainViewMode::Split,
                            side,
                            tab_id,
                            FountainViewMode::Split,
                            cx,
                        ))
                        .child(ftn_mode_btn(
                            format!("ftn-script-{tab_id}"),
                            "Script",
                            IconName::BookOpen,
                            view_mode == FountainViewMode::Script,
                            side,
                            tab_id,
                            FountainViewMode::Script,
                            cx,
                        )),
                )
                .when(view_mode != FountainViewMode::Edit, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("ftn-paper-{tab_id}")))
                            .with_variant(if paper_light {
                                ButtonVariant::Secondary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .xsmall()
                            .label(if paper_light {
                                "Paper: Light"
                            } else {
                                "Paper: Dark"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(t) = this.find_tab_mut(side, tab_id)
                                    && let TabKind::Fountain { paper_light, .. } = &mut t.kind
                                {
                                    *paper_light = !*paper_light;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("ftn-notes-{tab_id}")))
                            .with_variant(if show_notes {
                                ButtonVariant::Secondary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .xsmall()
                            .label("Notes")
                            .tooltip("Toggle Sections, Synopses & [[Notes]] in Preview")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(t) = this.find_tab_mut(side, tab_id)
                                    && let TabKind::Fountain { show_notes, .. } = &mut t.kind
                                {
                                    *show_notes = !*show_notes;
                                    cx.notify();
                                }
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("ftn-pdf-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .icon(IconName::ExternalLink)
                        .label("Export PDF")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.export_fountain_current_pdf(side, tab_id, window, cx);
                        })),
                )
                .when(dirty, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("ftn-save-{tab_id}")))
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
    let effective_mode = if zen_mode && view_mode == FountainViewMode::Split {
        FountainViewMode::Edit
    } else {
        view_mode
    };

    let edit_el = super::zen_editor_frame(
        Editor::new(editor)
            .size_full()
            .appearance(false)
            .bordered(false)
            .text_size(px(13.5))
            .line_height(relative(1.55)),
        zen_mode && effective_mode == FountainViewMode::Edit,
    );

    let script_el = render_script_preview(side, tab_id, &doc, paper_light, show_notes, cx);

    let body = match effective_mode {
        FountainViewMode::Edit => edit_el,
        FountainViewMode::Script => script_el.into_any_element(),
        FountainViewMode::Split => {
            h_resizable(SharedString::from(format!("ftn-split-{side:?}-{tab_id}")))
                .child(
                    resizable_panel()
                        .size_range(px(240.)..px(2000.))
                        .child(edit_el),
                )
                .child(
                    resizable_panel()
                        .size_range(px(260.)..px(2000.))
                        .child(script_el),
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
fn ftn_mode_btn(
    id: String,
    label: &'static str,
    icon: IconName,
    active: bool,
    side: PaneSide,
    tab_id: u64,
    target_mode: FountainViewMode,
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
                && let TabKind::Fountain { view_mode, .. } = &mut tab.kind
            {
                *view_mode = target_mode;
                cx.notify();
            }
        }))
}

struct ScriptPalette {
    sheet_bg: gpui_kit::Hsla,
    sheet_border: gpui_kit::Hsla,
    ink: gpui_kit::Hsla,
    muted: gpui_kit::Hsla,
    heading_fg: gpui_kit::Hsla,
    heading_bg: gpui_kit::Hsla,
    char_fg: gpui_kit::Hsla,
    paren_fg: gpui_kit::Hsla,
    trans_fg: gpui_kit::Hsla,
    note_bg: gpui_kit::Hsla,
    note_fg: gpui_kit::Hsla,
}

fn script_palette(paper_light: bool) -> ScriptPalette {
    if paper_light {
        ScriptPalette {
            sheet_bg: hex_color(0xf5f2ea),
            sheet_border: hex_color(0xd8d2c5),
            ink: hex_color(0x1b1918),
            muted: hex_color(0x66615a),
            heading_fg: hex_color(0x111010),
            heading_bg: hex_color(0xe9e4d8),
            char_fg: hex_color(0x2d4a5e),
            paren_fg: hex_color(0x5a554e),
            trans_fg: hex_color(0x7c3e3a),
            note_bg: hex_color(0xebe5d5),
            note_fg: hex_color(0x524c42),
        }
    } else {
        ScriptPalette {
            sheet_bg: hex_color(0x1a1524),
            sheet_border: hex_color(kd::BORDER_SUBTLE),
            ink: hex_color(kd::FG),
            muted: hex_color(kd::FG_MUTED),
            heading_fg: hex_color(kd::ACCENT),
            heading_bg: accent_alpha(0x1c),
            char_fg: hex_color(kd::YELLOW),
            paren_fg: hex_color(kd::FG_SUBTLE),
            trans_fg: hex_color(kd::ORANGE),
            note_bg: hex_alpha(0x8992a71f),
            note_fg: hex_color(kd::VIOLET),
        }
    }
}

fn render_script_preview(
    side: PaneSide,
    tab_id: u64,
    doc: &FountainDocument,
    paper_light: bool,
    show_notes: bool,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let pal = script_palette(paper_light);
    let mono_font = cx.theme().mono_font_family.clone();

    let title_block = if !doc.title_page.is_empty() {
        let mut title = None;
        let mut credit = None;
        let mut author = None;
        let mut right_meta = Vec::new();
        let mut left_meta = Vec::new();

        for entry in &doc.title_page {
            let kl = entry.key.to_ascii_lowercase();
            let joined = entry.values.join("\n");
            match kl.as_str() {
                "title" => title = Some(joined),
                "credit" => credit = Some(joined),
                "author" | "authors" => author = Some(joined),
                "draft date" | "date" | "revision" => right_meta.push(joined),
                _ => left_meta.push(joined),
            }
        }

        Some(
            v_flex()
                .w_full()
                .pb_6()
                .mb_6()
                .gap_2()
                .border_b_1()
                .border_color(pal.sheet_border)
                .child(
                    v_flex()
                        .items_center()
                        .gap_1p5()
                        .py_4()
                        .when_some(title, |this, t| {
                            this.child(
                                div()
                                    .text_base()
                                    .font_weight(gpui_kit::FontWeight::BOLD)
                                    .text_color(pal.heading_fg)
                                    .text_center()
                                    .child(t.to_ascii_uppercase()),
                            )
                        })
                        .when_some(credit, |this, c| {
                            this.child(div().text_sm().text_color(pal.muted).text_center().child(c))
                        })
                        .when_some(author, |this, a| {
                            this.child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(pal.ink)
                                    .text_center()
                                    .child(a),
                            )
                        }),
                )
                .when(!left_meta.is_empty() || !right_meta.is_empty(), |this| {
                    this.child(
                        h_flex()
                            .justify_between()
                            .text_xs()
                            .text_color(pal.muted)
                            .child(div().child(left_meta.join(" · ")))
                            .child(div().child(right_meta.join(" · "))),
                    )
                }),
        )
    } else {
        None
    };

    let mut scene_counter = 0usize;
    let body_items: Vec<gpui_kit::AnyElement> = doc
        .elements
        .iter()
        .filter_map(|el| match el {
            FountainElement::SceneHeading {
                text, scene_number, ..
            } => {
                scene_counter += 1;
                let num = scene_number
                    .as_deref()
                    .map(str::to_string)
                    .unwrap_or_else(|| scene_counter.to_string());
                Some(
                    h_flex()
                        .w_full()
                        .mt_5()
                        .mb_2()
                        .px_2()
                        .py_1()
                        .rounded(px(4.))
                        .bg(pal.heading_bg)
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui_kit::FontWeight::BOLD)
                                .text_color(pal.muted)
                                .child(num),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui_kit::FontWeight::BOLD)
                                .text_color(pal.heading_fg)
                                .child(text.to_ascii_uppercase()),
                        )
                        .into_any_element(),
                )
            }
            FountainElement::Action { text, .. } => Some(
                div()
                    .w_full()
                    .my_1p5()
                    .text_sm()
                    .line_height(relative(1.5))
                    .text_color(pal.ink)
                    .child(text.clone())
                    .into_any_element(),
            ),
            FountainElement::Character { name, dual, .. } => {
                let display = if *dual {
                    format!("{name} ^")
                } else {
                    name.clone()
                };
                Some(
                    div()
                        .w_full()
                        .mt_3()
                        .pl(px(210.))
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::BOLD)
                        .text_color(pal.char_fg)
                        .child(display.to_ascii_uppercase())
                        .into_any_element(),
                )
            }
            FountainElement::Parenthetical { text, .. } => Some(
                div()
                    .w_full()
                    .pl(px(165.))
                    .pr(px(130.))
                    .text_sm()
                    .italic()
                    .text_color(pal.paren_fg)
                    .child(text.clone())
                    .into_any_element(),
            ),
            FountainElement::Dialogue { text, .. } => Some(
                div()
                    .w_full()
                    .pl(px(110.))
                    .pr(px(100.))
                    .mb_2()
                    .text_sm()
                    .line_height(relative(1.45))
                    .text_color(pal.ink)
                    .child(text.clone())
                    .into_any_element(),
            ),
            FountainElement::Transition { text, .. } => Some(
                div()
                    .w_full()
                    .my_3()
                    .flex()
                    .justify_end()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::BOLD)
                    .text_color(pal.trans_fg)
                    .child(text.to_ascii_uppercase())
                    .into_any_element(),
            ),
            FountainElement::Centered { text, .. } => Some(
                div()
                    .w_full()
                    .my_3()
                    .text_center()
                    .text_sm()
                    .text_color(pal.ink)
                    .child(text.clone())
                    .into_any_element(),
            ),
            FountainElement::Lyric { text, .. } => Some(
                div()
                    .w_full()
                    .my_1()
                    .pl(px(40.))
                    .italic()
                    .text_sm()
                    .text_color(pal.muted)
                    .child(text.clone())
                    .into_any_element(),
            ),
            FountainElement::PageBreak { .. } => Some(
                div()
                    .w_full()
                    .my_5()
                    .py_1()
                    .border_t_1()
                    .border_color(pal.sheet_border)
                    .text_center()
                    .text_xs()
                    .text_color(pal.muted)
                    .child("— PAGE BREAK —")
                    .into_any_element(),
            ),
            FountainElement::Section { level, text, .. } if show_notes => Some(
                div()
                    .w_full()
                    .mt_4()
                    .px_2p5()
                    .py_1()
                    .rounded(px(4.))
                    .bg(pal.note_bg)
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::BOLD)
                    .text_color(pal.note_fg)
                    .child(format!("{} {text}", "#".repeat(*level)))
                    .into_any_element(),
            ),
            FountainElement::Synopsis { text, .. } if show_notes => Some(
                div()
                    .w_full()
                    .my_1()
                    .px_2p5()
                    .py_1()
                    .rounded(px(4.))
                    .bg(pal.note_bg)
                    .italic()
                    .text_xs()
                    .text_color(pal.note_fg)
                    .child(format!("Synopsis: {text}"))
                    .into_any_element(),
            ),
            FountainElement::Note { text, .. } if show_notes => Some(
                div()
                    .w_full()
                    .my_1()
                    .px_2p5()
                    .py_1()
                    .rounded(px(4.))
                    .bg(pal.note_bg)
                    .text_xs()
                    .text_color(pal.note_fg)
                    .child(format!("[[ {text} ]]"))
                    .into_any_element(),
            ),
            FountainElement::Section { .. }
            | FountainElement::Synopsis { .. }
            | FountainElement::Note { .. } => None,
        })
        .collect();

    div()
        .id(SharedString::from(format!("ftn-preview-scroll-{side:?}-{tab_id}")))
        .size_full()
        .bg(hex_color(kd::BG_DEEP))
        .overflow_y_scrollbar()
        .p_6()
        .child(
            v_flex()
                .w_full()
                .max_w(px(720.))
                .mx_auto()
                .p_8()
                .rounded(px(8.))
                .bg(pal.sheet_bg)
                .border_1()
                .border_color(pal.sheet_border)
                .font_family(mono_font)
                .when_some(title_block, |this, tb| this.child(tb))
                .children(body_items),
        )
}
