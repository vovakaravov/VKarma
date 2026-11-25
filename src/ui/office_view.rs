use super::find_bar::{FIND_INPUT_W, find_count_badge, floating_find_card};
use super::{OfficeFindMatch, PaneSide, TabKind, VKarmaApp};
use crate::formats::office::{OfficeBlock, OfficeRun};
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, Focusable as _, FontStyle, FontWeight, HighlightStyle,
    InteractiveElement as _, IntoElement, KeyDownEvent, ObjectFit, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, StyledImage as _, StyledText, UnderlineStyle, div, img,
    prelude::FluentBuilder as _, px,
};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn next_available_export_path(src: &Path, ext: &str) -> PathBuf {
    let base = src.with_extension(ext);
    if !base.exists() {
        return base;
    }
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("exported");
    let parent = src.parent().unwrap_or_else(|| Path::new("."));
    for i in 1..1000 {
        let cand = parent.join(format!("{stem}-{i}.{ext}"));
        if !cand.exists() {
            return cand;
        }
    }
    base
}

pub fn office_paragraph_text(runs: &[OfficeRun], list_prefix: Option<&str>) -> String {
    let mut full = String::new();
    if let Some(pref) = list_prefix {
        full.push_str(pref);
    }
    for r in runs {
        if !r.text.is_empty() {
            full.push_str(&r.text);
        }
    }
    full
}

fn collect_cell_matches(
    matches: &[OfficeFindMatch],
    active_idx: usize,
    block_ix: usize,
    row_ix: usize,
    col_ix: usize,
) -> (Vec<Range<usize>>, Option<usize>) {
    let mut ranges = Vec::new();
    let mut active_rel = None;
    for (ix, m) in matches.iter().enumerate() {
        if m.block_ix == block_ix && m.row_ix == row_ix && m.col_ix == col_ix {
            if ix == active_idx {
                active_rel = Some(ranges.len());
            }
            ranges.push(m.range.clone());
        }
    }
    (ranges, active_rel)
}

fn plain_match_highlights(ranges: &[Range<usize>], active_rel: Option<usize>) -> Vec<(Range<usize>, HighlightStyle)> {
    ranges
        .iter()
        .enumerate()
        .map(|(ix, r)| {
            let is_active = active_rel == Some(ix);
            (
                r.clone(),
                HighlightStyle {
                    background_color: Some(if is_active {
                        hex_alpha(0xe8a84988)
                    } else {
                        hex_alpha(0xc4b28a42)
                    }),
                    ..Default::default()
                },
            )
        })
        .collect()
}

fn build_styled_paragraph(
    runs: &[OfficeRun],
    list_prefix: Option<&str>,
    code_bg: gpui_kit::Hsla,
    code_fg: gpui_kit::Hsla,
) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    let mut full = String::new();
    let mut highlights = Vec::new();
    if let Some(pref) = list_prefix {
        full.push_str(pref);
    }
    for r in runs {
        if r.text.is_empty() {
            continue;
        }
        let start = full.len();
        full.push_str(&r.text);
        let end = full.len();
        if start < end && (r.bold || r.italic || r.underline || r.code) {
            highlights.push((
                start..end,
                HighlightStyle {
                    color: if r.code { Some(code_fg) } else { None },
                    font_weight: if r.bold { Some(FontWeight::BOLD) } else { None },
                    font_style: if r.italic {
                        Some(FontStyle::Italic)
                    } else {
                        None
                    },
                    background_color: if r.code { Some(code_bg) } else { None },
                    underline: if r.underline {
                        Some(UnderlineStyle {
                            thickness: px(1.),
                            color: None,
                            wavy: false,
                        })
                    } else {
                        None
                    },
                    ..Default::default()
                },
            ));
        }
    }
    (full, highlights)
}

pub fn render_office_tab(
    app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Office {
        doc,
        paper_light,
        font_scale,
        file_size_bytes,
        scroll_handle,
        find,
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let doc_path = tab.path.clone();
    let zen_mode = app.zen_mode;
    let light = *paper_light;
    let scale = *font_scale;
    let kb = *file_size_bytes as f64 / 1024.0;
    let col_max = if zen_mode { 680.0 } else { 780.0 };
    let scroll_handle = scroll_handle.clone();
    let find_open = find.open;
    let find_query = find.query.clone();
    let find_case_sensitive = find.case_sensitive;
    let find_input = find.input.clone();
    let find_matches = find.matches.clone();
    let find_active = find.active;
    let find_active_flag = find_open && !find_query.trim().is_empty();

    let outer_bg = hex_color(kd::BG_DEEP);
    let page_bg = if light {
        hex_color(0xfbfaf7)
    } else {
        hex_color(kd::BG_MAIN)
    };
    let text_fg = if light {
        hex_color(0x1f2328)
    } else {
        hex_color(kd::FG)
    };
    let muted_fg = if light {
        hex_color(0x57606a)
    } else {
        hex_color(kd::FG_MUTED)
    };
    let heading_fg = if light {
        hex_color(0x2d5a7b)
    } else {
        hex_color(kd::ACCENT)
    };
    let border_c = if light {
        hex_color(0xd8d3c8)
    } else {
        hex_color(kd::BORDER_SUBTLE)
    };
    let code_bg = if light {
        hex_color(0xede9df)
    } else {
        hex_color(kd::BG_SIDEBAR)
    };
    let code_fg = if light {
        hex_color(0xa54229)
    } else {
        hex_color(kd::ORANGE)
    };

    let path_md = doc_path.clone();
    let path_typ = doc_path;
    let doc_md = doc.clone();
    let doc_typ = doc.clone();

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
                        .text_color(hex_color(kd::GREEN))
                        .child(format!(
                            "{} · {} words · {} tables · {kb:.1} KB",
                            doc.format.label(),
                            doc.word_count,
                            doc.table_count
                        )),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new(SharedString::from(format!("off-find-{tab_id}")))
                        .with_variant(if find_open {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Search)
                        .tooltip("Find in Document (Ctrl+F)")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_office_find(side, tab_id, window, cx);
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("off-theme-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .icon(IconName::BookOpen)
                        .label(if light { "Paper" } else { "Dark" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                && let TabKind::Office { paper_light, .. } = &mut t.kind
                            {
                                *paper_light = !*paper_light;
                                cx.notify();
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("off-sm-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .label("A-")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                && let TabKind::Office { font_scale, .. } = &mut t.kind
                            {
                                *font_scale = (*font_scale - 0.1).clamp(0.75, 1.8);
                                cx.notify();
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("off-lg-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .label("A+")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                && let TabKind::Office { font_scale, .. } = &mut t.kind
                            {
                                *font_scale = (*font_scale + 0.1).clamp(0.75, 1.8);
                                cx.notify();
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("off-to-md-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .icon(IconName::FileText)
                        .label("Convert .md")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let dest = next_available_export_path(&path_md, "md");
                            match std::fs::write(&dest, doc_md.to_markdown()) {
                                Ok(()) => {
                                    this.refresh_vault_index(cx);
                                    this.open_file(dest.clone(), side, window, cx);
                                    this.toast_ok(format!("Created {}", dest.display()), window, cx);
                                }
                                Err(err) => this.toast_err(format!("{err}"), window, cx),
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("off-to-typ-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::BookOpen)
                        .label("Convert .typ")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let dest = next_available_export_path(&path_typ, "typ");
                            match std::fs::write(&dest, doc_typ.to_typst()) {
                                Ok(()) => {
                                    this.refresh_vault_index(cx);
                                    this.open_file(dest.clone(), side, window, cx);
                                    this.toast_ok(format!("Created {}", dest.display()), window, cx);
                                }
                                Err(err) => this.toast_err(format!("{err}"), window, cx),
                            }
                        })),
                ),
        );

    let block_els: Vec<AnyElement> = doc
        .blocks
        .iter()
        .enumerate()
        .map(|(blk_ix, blk)| match blk {
            OfficeBlock::Heading { level, text } => {
                let sz = match level {
                    1 => 24.0,
                    2 => 20.0,
                    3 => 17.0,
                    _ => 15.0,
                } * scale;
                let text_el: AnyElement = if find_active_flag {
                    let (ranges, active_rel) =
                        collect_cell_matches(&find_matches, find_active, blk_ix, 0, 0);
                    if ranges.is_empty() {
                        text.clone().into_any_element()
                    } else {
                        StyledText::new(text.clone())
                            .with_highlights(plain_match_highlights(&ranges, active_rel))
                            .into_any_element()
                    }
                } else {
                    text.clone().into_any_element()
                };
                div()
                    .pt_2()
                    .text_size(px(sz))
                    .font_weight(FontWeight::BOLD)
                    .text_color(heading_fg)
                    .child(text_el)
                    .into_any_element()
            }
            OfficeBlock::Paragraph { runs, list_prefix } => {
                let (full, base_highlights) =
                    build_styled_paragraph(runs, list_prefix.as_deref(), code_bg, code_fg);
                let highlights = if find_active_flag {
                    let (ranges, active_rel) =
                        collect_cell_matches(&find_matches, find_active, blk_ix, 0, 0);
                    if ranges.is_empty() {
                        base_highlights
                    } else {
                        crate::vault::merge_highlight_ranges(
                            &base_highlights,
                            &ranges,
                            active_rel,
                            |base_opt, is_active| {
                                let mut st = base_opt.cloned().unwrap_or_default();
                                st.background_color = Some(if is_active {
                                    hex_alpha(0xe8a84988)
                                } else {
                                    hex_alpha(0xc4b28a42)
                                });
                                st
                            },
                        )
                    }
                } else {
                    base_highlights
                };
                let pl = if list_prefix.is_some() { 14.0 } else { 0.0 };
                div()
                    .pl(px(pl))
                    .text_size(px(14.5 * scale))
                    .line_height(px(23.0 * scale))
                    .text_color(text_fg)
                    .child(StyledText::new(full).with_highlights(highlights))
                    .into_any_element()
            }
            OfficeBlock::Table { rows } => {
                let mut tbl = v_flex()
                    .w_full()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(border_c)
                    .overflow_hidden();
                for (rix, row) in rows.iter().enumerate() {
                    let row_bg = if rix == 0 {
                        if light {
                            hex_color(0xede9df)
                        } else {
                            hex_color(kd::BG_SIDEBAR)
                        }
                    } else if rix % 2 == 1 {
                        accent_alpha(0x0e)
                    } else {
                        gpui_kit::transparent_black()
                    };
                    tbl = tbl.child(
                        h_flex()
                            .w_full()
                            .bg(row_bg)
                            .border_b_1()
                            .border_color(border_c)
                            .children(row.iter().enumerate().map(|(cix, cell)| {
                                let cell_el: AnyElement = if find_active_flag {
                                    let (ranges, active_rel) = collect_cell_matches(
                                        &find_matches,
                                        find_active,
                                        blk_ix,
                                        rix,
                                        cix,
                                    );
                                    if ranges.is_empty() {
                                        cell.clone().into_any_element()
                                    } else {
                                        StyledText::new(cell.clone())
                                            .with_highlights(plain_match_highlights(
                                                &ranges,
                                                active_rel,
                                            ))
                                            .into_any_element()
                                    }
                                } else {
                                    cell.clone().into_any_element()
                                };
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .px_2p5()
                                    .py_1p5()
                                    .text_size(px(13.0 * scale))
                                    .when(rix == 0, |c| c.font_weight(FontWeight::SEMIBOLD))
                                    .text_color(text_fg)
                                    .child(cell_el)
                            })),
                    );
                }
                tbl.into_any_element()
            }
            OfficeBlock::Image { image } => div()
                .w_full()
                .flex()
                .justify_center()
                .py_2()
                .child(
                    img(Arc::clone(image))
                        .max_w(px(col_max - 64.0))
                        .max_h(px(420.))
                        .object_fit(ObjectFit::Contain)
                        .rounded(px(6.)),
                )
                .into_any_element(),
        })
        .collect();

    let find_card = if find_open {
        let total = find_matches.len();
        let trimmed_non_empty = !find_query.trim().is_empty();
        let count_text = if !trimmed_non_empty {
            String::new()
        } else if total == 0 {
            "0 / 0".to_string()
        } else {
            format!("{} / {}", find_active + 1, total)
        };
        let fi_clr = find_input.clone();
        let clear_btn = Button::new(SharedString::from(format!("off-find-x-{tab_id}")))
            .text()
            .xsmall()
            .tab_stop(false)
            .icon(IconName::Close)
            .tooltip("Clear")
            .on_click(cx.listener(move |this, _, window, cx| {
                fi_clr.update(cx, |s, cx| s.set_value("", window, cx));
                fi_clr.focus_handle(cx).focus(window, cx);
                this.clear_office_find_query(side, tab_id, cx);
            }));

        Some(
            floating_find_card(SharedString::from(format!("off-find-card-{tab_id}")))
                .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.key.as_str() == "escape" {
                        cx.stop_propagation();
                        this.close_office_find(side, tab_id, Some(window), cx);
                    } else if ev.keystroke.key.as_str() == "f3" {
                        cx.stop_propagation();
                        this.step_office_find(side, tab_id, !ev.keystroke.modifiers.shift, cx);
                    }
                }))
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(
                            div().w(px(FIND_INPUT_W)).child(
                                Input::new(&find_input)
                                    .xsmall()
                                    .prefix(
                                        Icon::new(IconName::Search)
                                            .xsmall()
                                            .text_color(hex_color(kd::FG_MUTED)),
                                    )
                                    .when(!find_query.is_empty(), |i| i.suffix(clear_btn)),
                            ),
                        )
                        .when(!count_text.is_empty(), |row| {
                            row.child(find_count_badge(count_text, trimmed_non_empty && total == 0))
                        })
                        .child(
                            Button::new(SharedString::from(format!("off-find-case-{tab_id}")))
                                .with_variant(if find_case_sensitive {
                                    ButtonVariant::Secondary
                                } else {
                                    ButtonVariant::Ghost
                                })
                                .xsmall()
                                .label("Aa")
                                .tooltip("Match Case")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_office_find_case(side, tab_id, cx);
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("off-find-prev-{tab_id}")))
                                .ghost()
                                .xsmall()
                                .icon(IconName::ChevronUp)
                                .tooltip("Previous Match (Shift+Enter)")
                                .disabled(total == 0)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.step_office_find(side, tab_id, false, cx);
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("off-find-next-{tab_id}")))
                                .ghost()
                                .xsmall()
                                .icon(IconName::ChevronDown)
                                .tooltip("Next Match (Enter)")
                                .disabled(total == 0)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.step_office_find(side, tab_id, true, cx);
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("off-find-close-{tab_id}")))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("Close (Esc)")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.close_office_find(side, tab_id, Some(window), cx);
                                })),
                        ),
                ),
        )
    } else {
        None
    };

    let scroll_el = div()
        .id(SharedString::from(format!("off-scroll-{tab_id}")))
        .track_scroll(&scroll_handle)
        .size_full()
        .overflow_y_scrollbar()
        .py_6()
        .px_6()
        .flex()
        .justify_center()
        .child(
            v_flex()
                .w_full()
                .max_w(px(col_max))
                .p_8()
                .gap_3()
                .rounded(px(10.))
                .bg(page_bg)
                .border_1()
                .border_color(border_c)
                .when_some(doc.title.as_ref(), |page, title| {
                    page.child(
                        div()
                            .pb_3()
                            .mb_2()
                            .border_b_1()
                            .border_color(border_c)
                            .text_xs()
                            .text_color(muted_fg)
                            .child(title.clone()),
                    )
                })
                .children(block_els),
        );

    v_flex()
        .size_full()
        .bg(outer_bg)
        .when(!zen_mode, |this| this.child(header))
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(scroll_el)
                .when_some(find_card, |d, card| d.child(card)),
        )
        .into_any_element()
}
