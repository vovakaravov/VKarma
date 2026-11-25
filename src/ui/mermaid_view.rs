use super::find_bar::{EditorFindBar, wrap_editor_with_find};
use super::svg_view::{SvgBgMode, render_svg_preview_box};
use super::{PaneSide, TabKind, TypstViewMode, VKarmaApp};
use crate::formats::mermaid::mermaid_to_svg;
use crate::formats::svg_engine::{SvgRenderResult, render_svg_to_image};
use crate::theme::{hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ParentElement, Render, SharedString, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use std::fs;
use std::path::PathBuf;

pub enum MermaidViewEvent {
    Changed,
}

pub struct MermaidView {
    editor: Entity<EditorState>,
    find_bar: Entity<EditorFindBar>,
    _ed_sub: Subscription,
    pub view_mode: TypstViewMode,
    pub dark_theme: bool,
    pub zoom: f32,
    doc_path: PathBuf,
    svg_text: Option<String>,
    rendered: Option<SvgRenderResult>,
    last_error: Option<String>,
    focus_handle: FocusHandle,
}

impl EventEmitter<MermaidViewEvent> for MermaidView {}

impl Focusable for MermaidView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl MermaidView {
    pub fn new(
        raw: String,
        doc_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (svg_text, rendered, last_error) = compile_mermaid(&raw, true);
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .searchable(false)
                .language("markdown")
                .soft_wrap(false)
                .line_number(true)
                .default_value(raw)
        });
        let find_bar = cx.new(|cx| EditorFindBar::new(editor.clone(), window, cx));
        let ed_sub = cx.subscribe(&editor, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.recompile(cx);
                cx.emit(MermaidViewEvent::Changed);
            }
        });

        Self {
            editor,
            find_bar,
            _ed_sub: ed_sub,
            view_mode: TypstViewMode::Split,
            dark_theme: true,
            zoom: 1.0,
            doc_path,
            svg_text,
            rendered,
            last_error,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn is_find_open(&self, cx: &App) -> bool {
        self.view_mode != TypstViewMode::Preview && self.find_bar.read(cx).is_open()
    }

    pub fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.view_mode == TypstViewMode::Preview {
            self.view_mode = TypstViewMode::Split;
        }
        self.find_bar.update(cx, |b, cx| b.open(replace, window, cx));
        cx.notify();
    }

    pub fn close_find(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.find_bar.update(cx, |b, cx| b.close(window, cx));
        cx.notify();
    }

    fn recompile(&mut self, cx: &mut Context<Self>) {
        let raw = self.editor.read(cx).value().to_string();
        let (svg_text, rendered, last_error) = compile_mermaid(&raw, self.dark_theme);
        if svg_text.is_some() {
            self.svg_text = svg_text;
        }
        if rendered.is_some() {
            self.rendered = rendered;
        }
        self.last_error = last_error;
        cx.notify();
    }

    pub fn relocate_doc(&mut self, new_path: PathBuf) {
        self.doc_path = new_path;
    }

    pub fn value(&self, cx: &App) -> String {
        self.editor.read(cx).value().to_string()
    }

    pub fn summary(&self) -> String {
        if self.last_error.is_some() {
            "Diagram Error".to_string()
        } else if let Some(ref res) = self.rendered {
            format!(
                "{:.0}x{:.0} · {}%",
                res.width,
                res.height,
                (self.zoom * 100.0).round() as i32
            )
        } else {
            "Mermaid".to_string()
        }
    }

    pub fn export_svg(&self) -> anyhow::Result<PathBuf> {
        let Some(ref svg) = self.svg_text else {
            anyhow::bail!("No rendered SVG available to export");
        };
        let out_path = self.doc_path.with_extension("svg");
        fs::write(&out_path, svg)?;
        Ok(out_path)
    }

    pub fn export_png(&self) -> anyhow::Result<PathBuf> {
        let Some(ref res) = self.rendered else {
            anyhow::bail!("No rendered PNG available to export");
        };
        let out_path = self.doc_path.with_extension("png");
        fs::write(&out_path, &res.png_bytes)?;
        Ok(out_path)
    }
}

fn compile_mermaid(
    raw: &str,
    dark_theme: bool,
) -> (Option<String>, Option<SvgRenderResult>, Option<String>) {
    match mermaid_to_svg(raw, dark_theme) {
        Ok(svg) => match render_svg_to_image(&svg, 2048) {
            Ok(res) => (Some(svg), Some(res), None),
            Err(err) => (Some(svg), None, Some(err)),
        },
        Err(err) => (None, None, Some(err)),
    }
}

impl Render for MermaidView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview = render_svg_preview_box(
            self.rendered.as_ref(),
            self.last_error.as_deref(),
            if self.dark_theme {
                SvgBgMode::Dark
            } else {
                SvgBgMode::Light
            },
            self.zoom,
            SharedString::from(format!("mmd-scroll-{:?}", cx.entity_id())),
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
            TypstViewMode::Edit => div()
                .size_full()
                .bg(hex_color(kd::BG_MAIN))
                .child(editor_pane)
                .into_any_element(),
            TypstViewMode::Preview => preview,
            TypstViewMode::Split => h_resizable("mmd-split-pane")
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

pub fn render_mermaid_tab(
    app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Mermaid { view, .. } = &tab.kind else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let dirty = tab.dirty;
    let zen_mode = app.zen_mode;
    let mv = view.read(cx);
    let view_mode = mv.view_mode;
    let dark_theme = mv.dark_theme;
    let summary = mv.summary();
    let find_open = mv.is_find_open(cx);

    let v_edit = view.clone();
    let v_split = view.clone();
    let v_prev = view.clone();
    let v_theme = view.clone();
    let v_zout = view.clone();
    let v_z100 = view.clone();
    let v_zin = view.clone();
    let v_svg = view.clone();
    let v_png = view.clone();

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
                        .text_color(hex_color(kd::AQUA))
                        .child(summary),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new(SharedString::from(format!("mmd-m-edit-{tab_id}")))
                        .with_variant(if view_mode == TypstViewMode::Edit {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Pencil)
                        .label("Edit")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_edit.update(cx, |v, cx| {
                                v.view_mode = TypstViewMode::Edit;
                                cx.notify();
                            });
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("mmd-m-split-{tab_id}")))
                        .with_variant(if view_mode == TypstViewMode::Split {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Split)
                        .label("Split")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_split.update(cx, |v, cx| {
                                v.view_mode = TypstViewMode::Split;
                                cx.notify();
                            });
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("mmd-m-prev-{tab_id}")))
                        .with_variant(if view_mode == TypstViewMode::Preview {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Eye)
                        .label("Preview")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            v_prev.update(cx, |v, cx| {
                                v.view_mode = TypstViewMode::Preview;
                                cx.notify();
                            });
                        })),
                )
                .when(view_mode != TypstViewMode::Edit, |row| {
                    row.child(
                        Button::new(SharedString::from(format!("mmd-theme-{tab_id}")))
                            .secondary()
                            .xsmall()
                            .label(if dark_theme { "Dark" } else { "Light" })
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_theme.update(cx, |v, cx| {
                                    v.dark_theme = !v.dark_theme;
                                    v.recompile(cx);
                                });
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("mmd-zout-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::ZoomOut)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_zout.update(cx, |v, cx| {
                                    v.zoom = (v.zoom - 0.15).clamp(0.2, 4.0);
                                    cx.notify();
                                });
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("mmd-z100-{tab_id}")))
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
                        Button::new(SharedString::from(format!("mmd-zin-{tab_id}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::ZoomIn)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                v_zin.update(cx, |v, cx| {
                                    v.zoom = (v.zoom + 0.15).clamp(0.2, 4.0);
                                    cx.notify();
                                });
                            })),
                    )
                })
                .child({
                    let v_find = view.clone();
                    Button::new(SharedString::from(format!("mmd-find-{tab_id}")))
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
                    Button::new(SharedString::from(format!("mmd-svg-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .label("SVG")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            match v_svg.read(cx).export_svg() {
                                Ok(out) => {
                                    this.refresh_vault_index(cx);
                                    this.toast_ok(format!("Exported {}", out.display()), window, cx);
                                }
                                Err(err) => this.toast_err(format!("{err}"), window, cx),
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("mmd-png-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .label("PNG")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            match v_png.read(cx).export_png() {
                                Ok(out) => {
                                    this.refresh_vault_index(cx);
                                    this.toast_ok(format!("Exported {}", out.display()), window, cx);
                                }
                                Err(err) => this.toast_err(format!("{err}"), window, cx),
                            }
                        })),
                )
                .when(dirty, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("mmd-save-{tab_id}")))
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
