use super::{TabKind, VKarmaApp};
use crate::formats::archive::format_bytes;
use crate::formats::font::FontInfo;
use crate::theme::{accent_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontTabMode {
    Specimen,
    Glyphs,
    Info,
}

pub struct FontView {
    pub info: FontInfo,
    pub mode: FontTabMode,
    sample_input: Entity<InputState>,
    glyph_filter: Entity<InputState>,
    _subs: [Subscription; 2],
    selected_glyph: Option<usize>,
    focus_handle: FocusHandle,
}

impl Focusable for FontView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl FontView {
    pub fn new(info: FontInfo, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _ = window
            .text_system()
            .add_fonts(vec![std::borrow::Cow::Owned(info.raw_bytes.clone())]);

        let sample_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value("The quick brown fox jumps over the lazy dog 0123456789")
        });
        let glyph_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter glyph char, hex, or name…"));
        let s1 = cx.subscribe(&sample_input, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        let s2 = cx.subscribe(&glyph_filter, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });

        Self {
            info,
            mode: FontTabMode::Specimen,
            sample_input,
            glyph_filter,
            _subs: [s1, s2],
            selected_glyph: None,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{} {} · {} glyphs · UPM {}",
            self.info.family, self.info.subfamily, self.info.num_glyphs, self.info.units_per_em
        )
    }
}

impl Render for FontView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let font_family: SharedString = self.info.family.clone().into();
        let body: gpui_kit::AnyElement = match self.mode {
            FontTabMode::Specimen => {
                let sample = self.sample_input.read(cx).value().to_string();
                let sample_disp = if sample.is_empty() {
                    "Aa Bb Cc 0123456789".to_string()
                } else {
                    sample
                };
                let sizes = [12.0f32, 16.0, 22.0, 30.0, 42.0, 56.0, 72.0];
                let pangram = "Sphinx of black quartz, judge my vow. 0123456789 !@#$%^&*() {}[] <>/\\ +-= ~`\"' — Typst, Markdown & GPUI Specimen.";

                v_flex()
                    .size_full()
                    .p_5()
                    .gap_4()
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(div().flex_1().child(Input::new(&self.sample_input).small()))
                            .child(
                                div()
                                    .px_2p5()
                                    .py_1()
                                    .rounded(px(6.))
                                    .bg(hex_color(kd::BG_SIDEBAR))
                                    .border_1()
                                    .border_color(hex_color(kd::BORDER_SUBTLE))
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child(format!(
                                        "Asc {} · Desc {} · Gap {} · UPM {}",
                                        self.info.ascender,
                                        self.info.descender,
                                        self.info.line_gap,
                                        self.info.units_per_em
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "fnt-spec-scroll-{:?}",
                                cx.entity_id()
                            )))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .child(
                                v_flex()
                                    .gap_3()
                                    .children(sizes.into_iter().map(|sz| {
                                        v_flex()
                                            .p_3()
                                            .gap_1()
                                            .rounded(px(8.))
                                            .bg(hex_color(kd::BG_SIDEBAR))
                                            .border_1()
                                            .border_color(hex_color(kd::BORDER_SUBTLE))
                                            .child(
                                                div()
                                                    .text_size(px(10.5))
                                                    .text_color(hex_color(kd::ACCENT))
                                                    .child(format!("{sz:.0} px")),
                                            )
                                            .child(
                                                div()
                                                    .font_family(font_family.clone())
                                                    .text_size(px(sz))
                                                    .text_color(hex_color(kd::FG))
                                                    .child(sample_disp.clone()),
                                            )
                                    }))
                                    .child(
                                        div()
                                            .p_4()
                                            .rounded(px(8.))
                                            .bg(hex_color(kd::BG_SIDEBAR))
                                            .border_1()
                                            .border_color(hex_color(kd::BORDER_SUBTLE))
                                            .font_family(font_family.clone())
                                            .text_size(px(18.))
                                            .line_height(px(28.))
                                            .text_color(hex_color(kd::FG_SUBTLE))
                                            .child(pangram),
                                    ),
                            ),
                    )
                    .into_any_element()
            }
            FontTabMode::Glyphs => {
                let q = self.glyph_filter.read(cx).value().trim().to_ascii_lowercase();
                let selected_ix = self.selected_glyph;

                let matching: Vec<(usize, &crate::formats::font::FontGlyphInfo)> = self
                    .info
                    .glyphs
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| {
                        if q.is_empty() {
                            return true;
                        }
                        let hex = format!("{:04x}", g.codepoint);
                        let ch_s = g.ch.to_string().to_ascii_lowercase();
                        ch_s.contains(&q)
                            || hex.contains(&q)
                            || g.name
                                .as_deref()
                                .is_some_and(|n| n.to_ascii_lowercase().contains(&q))
                    })
                    .take(600)
                    .collect();

                let inspector = selected_ix.and_then(|ix| self.info.glyphs.get(ix)).map(|g| {
                    let ch_copy = g.ch.to_string();
                    h_flex()
                        .p_2p5()
                        .rounded(px(7.))
                        .bg(hex_color(kd::BG_SIDEBAR))
                        .border_1()
                        .border_color(hex_color(kd::ACCENT))
                        .justify_between()
                        .items_center()
                        .gap_3()
                        .child(
                            h_flex()
                                .gap_3()
                                .items_center()
                                .child(
                                    div()
                                        .w(px(40.))
                                        .h(px(40.))
                                        .rounded(px(6.))
                                        .bg(hex_color(kd::BG_DEEP))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .font_family(font_family.clone())
                                        .text_size(px(24.))
                                        .text_color(hex_color(kd::FG))
                                        .child(g.ch.to_string()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(hex_color(kd::FG))
                                        .child(format!(
                                            "U+{:04X} · GID {} · {} · Advance {}",
                                            g.codepoint,
                                            g.glyph_id,
                                            g.name.as_deref().unwrap_or("(unnamed)"),
                                            g.advance
                                                .map(|a| a.to_string())
                                                .unwrap_or_else(|| "—".to_string())
                                        )),
                                ),
                        )
                        .child(
                            Button::new("fnt-copy-glyph")
                                .secondary()
                                .xsmall()
                                .icon(IconName::Copy)
                                .label("Copy Glyph")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        ch_copy.clone(),
                                    ));
                                })),
                        )
                });

                v_flex()
                    .size_full()
                    .p_4()
                    .gap_3()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().w(px(280.)).child(Input::new(&self.glyph_filter).xsmall()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child(format!(
                                        "Showing {} of {} Unicode glyphs",
                                        matching.len(),
                                        self.info.glyphs.len()
                                    )),
                            ),
                    )
                    .when_some(inspector, |col, bar| col.child(bar))
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "fnt-glyphs-scroll-{:?}",
                                cx.entity_id()
                            )))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .children(matching.into_iter().map(|(orig_ix, g)| {
                                        let is_sel = selected_ix == Some(orig_ix);
                                        v_flex()
                                            .id(SharedString::from(format!("fnt-g-{orig_ix}")))
                                            .w(px(66.))
                                            .h(px(68.))
                                            .p_1()
                                            .rounded(px(6.))
                                            .bg(if is_sel {
                                                accent_alpha(0x28)
                                            } else {
                                                hex_color(kd::BG_SIDEBAR)
                                            })
                                            .border_1()
                                            .border_color(if is_sel {
                                                hex_color(kd::ACCENT)
                                            } else {
                                                hex_color(kd::BORDER_SUBTLE)
                                            })
                                            .items_center()
                                            .justify_between()
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.selected_glyph = Some(orig_ix);
                                                cx.notify();
                                            }))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .font_family(font_family.clone())
                                                    .text_size(px(22.))
                                                    .text_color(hex_color(kd::FG))
                                                    .child(g.ch.to_string()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(9.5))
                                                    .text_color(hex_color(kd::FG_MUTED))
                                                    .child(format!("{:04X}", g.codepoint)),
                                            )
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            FontTabMode::Info => {
                let meta_rows: Vec<(&'static str, String)> = vec![
                    ("Family", self.info.family.clone()),
                    ("Subfamily", self.info.subfamily.clone()),
                    ("Full Name", self.info.full_name.clone()),
                    (
                        "Version",
                        self.info.version.clone().unwrap_or_else(|| "—".to_string()),
                    ),
                    (
                        "Designer",
                        self.info.designer.clone().unwrap_or_else(|| "—".to_string()),
                    ),
                    (
                        "License",
                        self.info.license.clone().unwrap_or_else(|| "—".to_string()),
                    ),
                    (
                        "Copyright",
                        self.info.copyright.clone().unwrap_or_else(|| "—".to_string()),
                    ),
                    (
                        "Metrics",
                        format!(
                            "{} glyphs · {} UPM · Asc {} · Desc {} · LineGap {}",
                            self.info.num_glyphs,
                            self.info.units_per_em,
                            self.info.ascender,
                            self.info.descender,
                            self.info.line_gap
                        ),
                    ),
                ];

                div()
                    .id(SharedString::from(format!(
                        "fnt-info-scroll-{:?}",
                        cx.entity_id()
                    )))
                    .size_full()
                    .overflow_y_scrollbar()
                    .p_5()
                    .child(
                        v_flex()
                            .max_w(px(760.))
                            .gap_4()
                            .child(
                                v_flex()
                                    .rounded(px(8.))
                                    .bg(hex_color(kd::BG_SIDEBAR))
                                    .border_1()
                                    .border_color(hex_color(kd::BORDER_SUBTLE))
                                    .overflow_hidden()
                                    .children(meta_rows.into_iter().map(|(k, v)| {
                                        h_flex()
                                            .px_4()
                                            .py_2()
                                            .gap_3()
                                            .border_b_1()
                                            .border_color(hex_color(kd::BORDER_SUBTLE))
                                            .child(
                                                div()
                                                    .w(px(120.))
                                                    .flex_shrink_0()
                                                    .text_xs()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(hex_color(kd::ACCENT))
                                                    .child(k),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_xs()
                                                    .text_color(hex_color(kd::FG))
                                                    .child(v),
                                            )
                                    })),
                            )
                            .when(!self.info.axes.is_empty(), |col| {
                                col.child(
                                    v_flex()
                                        .p_4()
                                        .gap_2()
                                        .rounded(px(8.))
                                        .bg(hex_color(kd::BG_SIDEBAR))
                                        .border_1()
                                        .border_color(hex_color(kd::BORDER_SUBTLE))
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(hex_color(kd::AQUA))
                                                .child("Variable Font Axes"),
                                        )
                                        .children(self.info.axes.iter().map(|ax| {
                                            div()
                                                .text_xs()
                                                .text_color(hex_color(kd::FG))
                                                .child(format!(
                                                    "{}: min {} / default {} / max {}",
                                                    ax.tag, ax.min, ax.default, ax.max
                                                ))
                                        })),
                                )
                            })
                            .when(!self.info.features.is_empty(), |col| {
                                col.child(
                                    v_flex()
                                        .p_4()
                                        .gap_2()
                                        .rounded(px(8.))
                                        .bg(hex_color(kd::BG_SIDEBAR))
                                        .border_1()
                                        .border_color(hex_color(kd::BORDER_SUBTLE))
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(hex_color(kd::VIOLET))
                                                .child(format!(
                                                    "OpenType Features ({})",
                                                    self.info.features.len()
                                                )),
                                        )
                                        .child(
                                            h_flex().flex_wrap().gap_1p5().children(
                                                self.info.features.iter().map(|tag| {
                                                    div()
                                                        .px_2()
                                                        .py_0p5()
                                                        .rounded(px(4.))
                                                        .bg(hex_color(kd::BG_DEEP))
                                                        .text_xs()
                                                        .text_color(hex_color(kd::YELLOW))
                                                        .child(tag.clone())
                                                }),
                                            ),
                                        ),
                                )
                            }),
                    )
                    .into_any_element()
            }
        };

        div()
            .id(SharedString::from(format!("font-view-{:?}", cx.entity_id())))
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .track_focus(&self.focus_handle)
            .child(body)
            .into_any_element()
    }
}

pub fn render_font_tab(
    app: &VKarmaApp,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Font {
        view,
        file_size_bytes,
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let zen_mode = app.zen_mode;
    let sz = format_bytes(*file_size_bytes);
    let fv = view.read(cx);
    let mode = fv.mode;
    let summary = fv.summary();
    let typst_snippet = format!("#set text(font: \"{}\")", fv.info.family);

    let v_spec = view.clone();
    let v_glyph = view.clone();
    let v_info = view.clone();

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
                        .child(format!("{summary} · {sz}")),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new(SharedString::from(format!("fnt-m-spec-{tab_id}")))
                        .with_variant(if mode == FontTabMode::Specimen {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .label("Specimen")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_spec.update(cx, |v, cx| {
                                v.mode = FontTabMode::Specimen;
                                cx.notify();
                            });
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("fnt-m-gly-{tab_id}")))
                        .with_variant(if mode == FontTabMode::Glyphs {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .label("Glyphs")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_glyph.update(cx, |v, cx| {
                                v.mode = FontTabMode::Glyphs;
                                cx.notify();
                            });
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("fnt-m-inf-{tab_id}")))
                        .with_variant(if mode == FontTabMode::Info {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .label("Info")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_info.update(cx, |v, cx| {
                                v.mode = FontTabMode::Info;
                                cx.notify();
                            });
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("fnt-copy-typ-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .icon(IconName::Copy)
                        .label("Copy Typst Font")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                typst_snippet.clone(),
                            ));
                            this.toast_ok(format!("Copied {typst_snippet}"), window, cx);
                        })),
                ),
        );

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .when(!zen_mode, |this| this.child(header))
        .child(div().flex_1().min_h_0().w_full().child(view.clone()))
        .into_any_element()
}
