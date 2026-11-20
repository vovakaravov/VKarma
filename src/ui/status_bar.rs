use super::{TabKind, VKarmaApp};
use crate::spreadsheet::cell_ref;
use crate::theme::{hex_color, kd};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex};
use gpui_kit::{Context, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder as _, px};

pub fn render_status_bar(app: &VKarmaApp, cx: &mut Context<VKarmaApp>) -> impl IntoElement {
    let bar_bg = hex_color(kd::BG_DEEP);
    let border = hex_color(kd::BORDER_SUBTLE);
    let fg_muted = hex_color(kd::FG_MUTED);

    let file_info = match app.active_tab_ref() {
        Some(tab) => match &tab.kind {
            TabKind::Markdown {
                word_count,
                char_count,
                view_mode,
                source_mode,
                is_mdx,
                ..
            } => {
                let mode = if *view_mode == crate::config::MdViewMode::Reading {
                    "Read"
                } else if *source_mode {
                    "Source"
                } else if *is_mdx {
                    "MDX Live Preview"
                } else {
                    "Live Preview"
                };
                let read_mins = if *word_count == 0 {
                    0
                } else {
                    (word_count + 219) / 220
                };
                format!(
                    "{} · {word_count} words · ~{read_mins} min · {char_count} chars · {mode}",
                    tab.rel_path
                )
            }
            TabKind::Fountain {
                parsed,
                paper_light,
                ..
            } => {
                let paper = if *paper_light {
                    "Light Script"
                } else {
                    "Dark Script"
                };
                format!(
                    "{} · {} scenes · ~{:.1} pages · {} words · {paper}",
                    tab.rel_path, parsed.scene_count, parsed.estimated_pages, parsed.word_count
                )
            }
            TabKind::Typst {
                compile_ms,
                compiling,
                pages,
                last_error,
                ..
            } => {
                let status = if *compiling {
                    "Compiling…".to_string()
                } else if last_error.is_some() {
                    "Compile Error".to_string()
                } else if let Some(ms) = compile_ms {
                    format!("{}p · {ms}ms", pages.len())
                } else {
                    "Ready".to_string()
                };
                format!("{} · Typst · {status}", tab.rel_path)
            }
            TabKind::Spreadsheet {
                doc,
                active_row,
                active_col,
                ..
            } => {
                format!(
                    "{} · {} · Cell {}",
                    tab.rel_path,
                    doc.format.label(),
                    cell_ref(*active_row, *active_col)
                )
            }
            TabKind::Code {
                language, soft_wrap, ..
            } => {
                let wrap = if *soft_wrap { "Wrap" } else { "NoWrap" };
                format!("{} · {language} · {wrap}", tab.rel_path)
            }
            TabKind::Image {
                zoom,
                fit_to_view,
                file_size_bytes,
            } => {
                let z = if *fit_to_view {
                    "Fit".to_string()
                } else {
                    format!("{}%", (zoom * 100.0).round() as i32)
                };
                format!(
                    "{} · {:.1} KB · {z}",
                    tab.rel_path,
                    *file_size_bytes as f64 / 1024.0
                )
            }
        },
        None => format!(
            "Vault: {} files ({} notes)",
            app.vault.all_files.len(),
            app.vault.md_count
        ),
    };

    let saved_info = app
        .active_tab_ref()
        .and_then(|t| {
            if t.dirty {
                Some("Unsaved (Ctrl+S)".to_string())
            } else {
                t.last_saved.as_ref().map(|ts| format!("Saved {ts}"))
            }
        })
        .or_else(|| app.status_toast.clone());

    h_flex()
        .h(px(26.))
        .w_full()
        .px_2()
        .justify_between()
        .items_center()
        .bg(bar_bg)
        .border_t_1()
        .border_color(border)
        .text_xs()
        .text_color(fg_muted)
        .child(
            h_flex()
                .gap_3()
                .min_w_0()
                .child(
                    div()
                        .text_color(hex_color(kd::GREEN))
                        .child(format!("{} notes", app.vault.md_count)),
                )
                .child(div().truncate().child(file_info)),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .flex_shrink_0()
                .when_some(saved_info, |this, s| {
                    this.child(
                        div()
                            .text_color(if s.starts_with("Unsaved") {
                                hex_color(kd::YELLOW)
                            } else {
                                fg_muted
                            })
                            .child(s),
                    )
                })
                .child(
                    Button::new("status-autosave-btn")
                        .with_variant(if app.config.autosave {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .label(if app.config.autosave {
                            "Autosave: ON"
                        } else {
                            "Autosave: OFF"
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.config.autosave = !this.config.autosave;
                            let _ = this.config.save();
                            if this.config.autosave {
                                this.save_all_tabs(window, cx);
                            }
                            cx.notify();
                        })),
                ),
        )
}
