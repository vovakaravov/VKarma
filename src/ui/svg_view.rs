use super::find_bar::{EditorFindBar, wrap_editor_with_find};
use super::{PaneSide, TabKind, VKarmaApp};
use crate::formats::svg_engine::{SvgRenderResult, render_svg_to_image};
use crate::theme::{hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ObjectFit, ParentElement, Render, SharedString,
    Styled, StyledImage as _, Subscription, Window, canvas, div, fill, img, point,
    prelude::FluentBuilder as _, px, size,
};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

pub enum SvgViewEvent {
    Changed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgViewMode {
    Preview,
    Split,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgBgMode {
    Checkerboard,
    Dark,
    Light,
}

pub struct SvgView {
    editor: Entity<EditorState>,
    find_bar: Entity<EditorFindBar>,
    _ed_sub: Subscription,
    pub view_mode: SvgViewMode,
    pub bg_mode: SvgBgMode,
    pub zoom: f32,
    doc_path: PathBuf,
    pub rendered: Option<SvgRenderResult>,
    pub last_error: Option<String>,
    focus_handle: FocusHandle,
}

impl EventEmitter<SvgViewEvent> for SvgView {}

impl Focusable for SvgView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SvgView {
    pub fn new(
        raw: String,
        doc_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (rendered, last_error) = match render_svg_to_image(&raw, 2048) {
            Ok(res) => (Some(res), None),
            Err(err) => (None, Some(err)),
        };
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .searchable(false)
                .language("html")
                .soft_wrap(false)
                .line_number(true)
                .default_value(raw)
        });
        let find_bar = cx.new(|cx| EditorFindBar::new(editor.clone(), window, cx));
        let ed_sub = cx.subscribe(&editor, |this, ed, event, cx| {
            if matches!(event, InputEvent::Change) {
                let txt = ed.read(cx).value().to_string();
                match render_svg_to_image(&txt, 2048) {
                    Ok(res) => {
                        this.rendered = Some(res);
                        this.last_error = None;
                    }
                    Err(err) => {
                        this.last_error = Some(err);
                    }
                }
                cx.emit(SvgViewEvent::Changed);
                cx.notify();
            }
        });

        Self {
            editor,
            find_bar,
            _ed_sub: ed_sub,
            view_mode: SvgViewMode::Split,
            bg_mode: SvgBgMode::Checkerboard,
            zoom: 1.0,
            doc_path,
            rendered,
            last_error,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn is_find_open(&self, cx: &App) -> bool {
        self.view_mode != SvgViewMode::Preview && self.find_bar.read(cx).is_open()
    }

    pub fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.view_mode == SvgViewMode::Preview {
            self.view_mode = SvgViewMode::Split;
        }
        self.find_bar.update(cx, |b, cx| b.open(replace, window, cx));
        cx.notify();
    }

    pub fn close_find(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.find_bar.update(cx, |b, cx| b.close(window, cx));
        cx.notify();
    }

    pub fn relocate_doc(&mut self, new_path: PathBuf) {
        self.doc_path = new_path;
    }

    pub fn value(&self, cx: &App) -> String {
        self.editor.read(cx).value().to_string()
    }

    pub fn summary(&self) -> String {
        if let Some(ref res) = self.rendered {
            format!(
                "{:.0}x{:.0} · {} elems · {}%",
                res.width,
                res.height,
                res.element_count,
                (self.zoom * 100.0).round() as i32
            )
        } else {
            "SVG Error".to_string()
        }
    }

    pub fn export_png(&self) -> anyhow::Result<PathBuf> {
        let Some(ref res) = self.rendered else {
            anyhow::bail!("No rendered SVG image available to export");
        };
        let out_path = self.doc_path.with_extension("png");
        fs::write(&out_path, &res.png_bytes)?;
        Ok(out_path)
    }
}

pub(crate) fn render_svg_preview_box(
    rendered: Option<&SvgRenderResult>,
    last_error: Option<&str>,
    bg_mode: SvgBgMode,
    zoom: f32,
    scroll_id: SharedString,
) -> AnyElement {
    let checker_canvas = (bg_mode == SvgBgMode::Checkerboard).then(|| {
        canvas(
            |bounds, _, _| bounds,
            |bounds, _, window, _| {
                let ox = f32::from(bounds.origin.x);
                let oy = f32::from(bounds.origin.y);
                let w = f32::from(bounds.size.width);
                let h = f32::from(bounds.size.height);
                let tile = 14.0f32;
                let c_a = hex_color(0x232222);
                let c_b = hex_color(0x2d2c2b);
                let cols = (w / tile).ceil() as usize;
                let rows = (h / tile).ceil() as usize;
                for r in 0..rows.min(180) {
                    for c in 0..cols.min(240) {
                        let color = if (r + c) % 2 == 0 { c_a } else { c_b };
                        window.paint_quad(fill(
                            Bounds {
                                origin: point(px(ox + c as f32 * tile), px(oy + r as f32 * tile)),
                                size: size(px(tile), px(tile)),
                            },
                            color,
                        ));
                    }
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full()
    });

    let solid_bg = match bg_mode {
        SvgBgMode::Checkerboard | SvgBgMode::Dark => hex_color(kd::BG_DEEP),
        SvgBgMode::Light => hex_color(0xf4f1ea),
    };

    let content: AnyElement = if let Some(err) = last_error {
        div()
            .p_6()
            .max_w(px(520.))
            .rounded(px(8.))
            .bg(hex_color(kd::BG_SIDEBAR))
            .border_1()
            .border_color(hex_color(kd::RED))
            .text_xs()
            .text_color(hex_color(kd::RED))
            .child(err.to_string())
            .into_any_element()
    } else if let Some(res) = rendered {
        let disp_w = (res.width * zoom).clamp(48.0, 2800.0);
        let disp_h = (res.height * zoom).clamp(48.0, 2800.0);
        img(Arc::clone(&res.image))
            .w(px(disp_w))
            .h(px(disp_h))
            .object_fit(ObjectFit::Contain)
            .into_any_element()
    } else {
        div()
            .text_xs()
            .text_color(hex_color(kd::FG_MUTED))
            .child("Rendering vector graphic…")
            .into_any_element()
    };

    div()
        .relative()
        .size_full()
        .bg(solid_bg)
        .when_some(checker_canvas, |this, ch| this.child(ch))
        .child(
            div()
                .id(scroll_id)
                .size_full()
                .overflow_y_scrollbar()
                .flex()
                .items_center()
                .justify_center()
                .p_6()
                .child(content),
        )
        .into_any_element()
}

impl Render for SvgView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview = render_svg_preview_box(
            self.rendered.as_ref(),
            self.last_error.as_deref(),
            self.bg_mode,
            self.zoom,
            SharedString::from(format!("svg-scroll-{:?}", cx.entity_id())),
        );
        let editor_pane = wrap_editor_with_find(
            Editor::new(&self.editor)
                .size_full()
                .appearance(false)
                .bordered(false)
                .into_any_element(),
            &self.find_bar,
            cx,
        );

        match self.view_mode {
            SvgViewMode::Source => div()
                .size_full()
                .bg(hex_color(kd::BG_MAIN))
                .child(editor_pane)
                .into_any_element(),
            SvgViewMode::Preview => preview,
            SvgViewMode::Split => h_resizable("svg-split-pane")
                .child(
                    resizable_panel()
                        .size_range(px(220.)..px(1400.))
                        .child(editor_pane),
                )
                .child(resizable_panel().child(preview))
                .into_any_element(),
        }
    }
}

pub fn render_svg_tab(
    app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Svg { view, .. } = &tab.kind else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let dirty = tab.dirty;
    let zen_mode = app.zen_mode;
    let sv = view.read(cx);
    let view_mode = sv.view_mode;
    let bg_mode = sv.bg_mode;
    let summary = sv.summary();
    let find_open = sv.is_find_open(cx);

    let v_prev = view.clone();
    let v_split = view.clone();
    let v_src = view.clone();
    let v_bg = view.clone();
    let v_zout = view.clone();
    let v_z100 = view.clone();
    let v_zin = view.clone();
    let v_exp = view.clone();

    let bg_label = match bg_mode {
        SvgBgMode::Checkerboard => "Checker",
        SvgBgMode::Dark => "Dark",
        SvgBgMode::Light => "Light",
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
                .child(
                    div()
                        .text_xs()
                        .text_color(hex_color(kd::PINK))
                        .child(summary),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new(SharedString::from(format!("svg-m-prev-{tab_id}")))
                        .with_variant(if view_mode == SvgViewMode::Preview {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Eye)
                        .label("Preview")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_prev.update(cx, |v, cx| {
                                v.view_mode = SvgViewMode::Preview;
                                cx.notify();
                            });
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("svg-m-split-{tab_id}")))
                        .with_variant(if view_mode == SvgViewMode::Split {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Split)
                        .label("Split")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_split.update(cx, |v, cx| {
                                v.view_mode = SvgViewMode::Split;
                                cx.notify();
                            });
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("svg-m-src-{tab_id}")))
                        .with_variant(if view_mode == SvgViewMode::Source {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Code)
                        .label("Source")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_src.update(cx, |v, cx| {
                                v.view_mode = SvgViewMode::Source;
                                cx.notify();
                            });
                        })),
                )
                .when(view_mode != SvgViewMode::Source, |row| {
                    row.child(
                        Button::new(SharedString::from(format!("svg-bg-{tab_id}")))
                            .secondary()
                            .xsmall()
                            .label(bg_label)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_bg.update(cx, |v, cx| {
                                    v.bg_mode = match v.bg_mode {
                                        SvgBgMode::Checkerboard => SvgBgMode::Dark,
                                        SvgBgMode::Dark => SvgBgMode::Light,
                                        SvgBgMode::Light => SvgBgMode::Checkerboard,
                                    };
                                    cx.notify();
                                });
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("svg-zout-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::ZoomOut)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_zout.update(cx, |v, cx| {
                                    v.zoom = (v.zoom - 0.2).clamp(0.2, 5.0);
                                    cx.notify();
                                });
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("svg-z100-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .label("100%")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_z100.update(cx, |v, cx| {
                                    v.zoom = 1.0;
                                    cx.notify();
                                });
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("svg-zin-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::ZoomIn)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_zin.update(cx, |v, cx| {
                                    v.zoom = (v.zoom + 0.2).clamp(0.2, 5.0);
                                    cx.notify();
                                });
                            })),
                    )
                })
                .child({
                    let v_find = view.clone();
                    Button::new(SharedString::from(format!("svg-find-{tab_id}")))
                        .with_variant(if find_open {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Search)
                        .tooltip("Find & Replace (Ctrl+F)")
                        .on_click(cx.listener(move |_, _, window, cx| {
                            v_find.update(cx, |v, cx| v.open_find(false, window, cx));
                        }))
                })
                .child(
                    Button::new(SharedString::from(format!("svg-png-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .icon(IconName::ExternalLink)
                        .label("Export PNG")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let res = v_exp.read(cx).export_png();
                            match res {
                                Ok(out) => {
                                    this.refresh_vault_index(cx);
                                    this.toast_ok(
                                        format!("Exported {}", out.display()),
                                        window,
                                        cx,
                                    );
                                }
                                Err(err) => this.toast_err(format!("{err}"), window, cx),
                            }
                        })),
                )
                .when(dirty && !app.config.autosave, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("svg-save-{tab_id}")))
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

    let body = super::zen_editor_frame(view.clone().into_any_element(), zen_mode);

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_DEEP))
        .when(!zen_mode, |this| this.child(header))
        .child(div().flex_1().min_h_0().w_full().child(body))
        .into_any_element()
}
