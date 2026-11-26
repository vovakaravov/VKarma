use super::VKarmaMobileApp;
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use crate::vault::{app_storage_dir, can_read_dir};
use gpui::{prelude::*, *};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::{Icon, IconName, Sizable as _, h_flex, v_flex};
use std::path::{Path, PathBuf};

fn set_icon(name: IconName, sz: f32, color: u32) -> impl IntoElement {
    Icon::new(name).size(px(sz)).text_color(hex_color(color))
}

fn render_root_candidate_row(
    id_prefix: &str,
    ix: usize,
    label: String,
    path: PathBuf,
    is_active: bool,
    on_pick: impl Fn(&mut VKarmaMobileApp, PathBuf, &mut Context<VKarmaMobileApp>) + 'static,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let p_str = path.to_string_lossy().into_owned();
    div()
        .id(SharedString::from(format!("{id_prefix}-{ix}")))
        .w_full()
        .px_3()
        .py_2()
        .rounded(px(8.))
        .bg(hex_color(if is_active {
            kd::BG_PANEL
        } else {
            kd::BG_MAIN
        }))
        .border_1()
        .border_color(hex_color(if is_active {
            kd::ACCENT
        } else {
            kd::BORDER_SUBTLE
        }))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            on_pick(this, path.clone(), cx);
        }))
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hex_color(kd::FG))
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(hex_color(kd::FG_MUTED))
                        .truncate()
                        .child(p_str),
                ),
        )
}

pub fn render_settings_tab(
    app: &VKarmaMobileApp,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let header = h_flex()
        .w_full()
        .px_3()
        .py_2p5()
        .bg(hex_color(kd::BG_DEEP))
        .border_b_1()
        .border_color(hex_color(kd::BORDER_SUBTLE))
        .justify_between()
        .items_center()
        .gap_2()
        .child(
            Button::new("set-back-prev")
                .ghost()
                .small()
                .label("← Notes")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_settings(window, cx);
                })),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_base()
                .font_weight(FontWeight::BOLD)
                .text_color(hex_color(kd::FG))
                .truncate()
                .child("Roots & Settings"),
        )
        .child(
            Button::new("set-recheck")
                .secondary()
                .small()
                .label("Rescan")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.recheck_storage_and_refresh(cx);
                })),
        );

    let mut vault_card = v_flex()
        .w_full()
        .p_3p5()
        .gap_2()
        .rounded(px(12.))
        .bg(hex_color(kd::BG_SIDEBAR))
        .border_1()
        .border_color(hex_color(kd::BORDER_SUBTLE))
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hex_color(kd::FG))
                        .child("Notes Vault Root"),
                )
                .child(
                    Button::new("set-browse-vault")
                        .primary()
                        .small()
                        .label("Browse / Set Root")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_folder_picker(window, cx);
                        })),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(hex_color(kd::ACCENT_HOVER))
                .child(format!("Active: {}", app.vault_root.display())),
        );
    for (ix, (lbl, path)) in app.vault_candidates.iter().enumerate() {
        vault_card = vault_card.child(render_root_candidate_row(
            "vroot",
            ix,
            lbl.clone(),
            path.clone(),
            app.vault_root == *path,
            |this, p, cx| this.set_vault_root(p, cx),
            cx,
        ));
    }

    let body = v_flex()
        .w_full()
        .p_3p5()
        .gap_3()
        .child(vault_card)
        .child(
            v_flex()
                .w_full()
                .p_3p5()
                .gap_2()
                .rounded(px(12.))
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_1()
                .border_color(hex_color(kd::BORDER_SUBTLE))
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(hex_color(kd::FG))
                                .child("Storage Permission"),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded(px(6.))
                                .bg(if app.has_storage_access {
                                    hex_alpha(0x87a98728)
                                } else {
                                    hex_alpha(0xc4b28a28)
                                })
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(hex_color(if app.has_storage_access {
                                    kd::GREEN
                                } else {
                                    kd::YELLOW
                                }))
                                .child(if app.has_storage_access {
                                    "All Files Granted"
                                } else {
                                    "Scoped Fallback"
                                }),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(hex_color(kd::FG_SUBTLE))
                        .child("Android All Files access lets VKarma read & save notes in /sdcard folders directly."),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .p_3p5()
                .gap_1p5()
                .rounded(px(12.))
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_1()
                .border_color(hex_color(kd::BORDER_SUBTLE))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hex_color(kd::ACCENT_HOVER))
                        .child("VKarma Mobile · Bleeding Edge GPUI"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(hex_color(kd::FG_SUBTLE))
                        .child("Engine: gpui-pre 0.3.10 + gpui-kit + gpui-mobile (Vulkan/wgpu · aarch64)"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(hex_color(kd::FG_MUTED))
                        .child("Format-compatible with Desktop VKarma Markdown vaults."),
                ),
        );

    v_flex()
        .size_full()
        .child(header)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(
                    div()
                        .id("settings-scroll")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&app.settings_scroll)
                        .child(body),
                )
                .child(Scrollbar::vertical(&app.settings_scroll)),
        )
        .when(app.safe_area.bottom > 0.0, |col| {
            col.child(
                div()
                    .w_full()
                    .h(px(app.safe_area.bottom))
                    .flex_shrink_0()
                    .bg(hex_color(kd::BG_DEEP)),
            )
        })
}

fn picker_quick_jumps() -> Vec<(&'static str, PathBuf)> {
    let mut jumps = Vec::new();
    #[cfg(target_os = "android")]
    {
        for (lbl, raw) in [
            ("/sdcard", "/sdcard"),
            ("Documents", "/sdcard/Documents"),
            ("Books", "/sdcard/Books"),
            ("Download", "/sdcard/Download"),
        ] {
            let p = PathBuf::from(raw);
            if can_read_dir(&p) {
                jumps.push((lbl, p));
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for (lbl, p) in [
            ("~", home.clone()),
            ("Documents", home.join("Documents")),
            ("Books", home.join("Books")),
        ] {
            if can_read_dir(&p) {
                jumps.push((lbl, p));
            }
        }
    }
    let internal = app_storage_dir("");
    if can_read_dir(&internal) {
        jumps.push(("App Storage", internal));
    }
    jumps
}

pub fn render_folder_picker_modal(
    app: &VKarmaMobileApp,
    bottom_inset: f32,
    cx: &mut Context<VKarmaMobileApp>,
) -> impl IntoElement {
    let Some(picker) = app.folder_picker.as_ref() else {
        return div().into_any_element();
    };

    let current_dir = picker.browsing_dir.clone();
    let current_display = current_dir.to_string_lossy().into_owned();
    let match_label = format!("{} notes", picker.direct_match_count);
    let subdirs = picker.subdirs.clone();
    let error = picker.error.clone();
    let create_candidate = picker.create_candidate.clone();
    let quick_jumps = picker_quick_jumps();
    let can_go_up = current_dir.parent().is_some_and(can_read_dir)
        || (cfg!(target_os = "android") && current_dir != Path::new("/sdcard"));

    v_flex()
        .id("folder-picker-backdrop")
        .size_full()
        .absolute()
        .inset_0()
        .occlude()
        .p_3()
        .bg(hex_alpha(0x000000b8))
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_scroll(|_, _, cx| {
            cx.stop_propagation();
        })
        .on_click(cx.listener(|this, _, window, cx| {
            this.close_folder_picker(window, cx);
        }))
        .child(
            v_flex()
                .id("folder-picker-card")
                .flex_1()
                .min_h_0()
                .w_full()
                .max_w(px(520.))
                .mx_auto()
                .rounded(px(14.))
                .overflow_hidden()
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_1()
                .border_color(hex_color(kd::BORDER))
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .on_scroll(|_, _, cx| {
                    cx.stop_propagation();
                })
                .on_click(|_, _, cx| {
                    cx.stop_propagation();
                })
                .child(
                    h_flex()
                        .w_full()
                        .px_3p5()
                        .py_2p5()
                        .bg(hex_color(kd::BG_DEEP))
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .justify_between()
                        .items_center()
                        .gap_2()
                        .child(
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .items_center()
                                .gap_2()
                                .child(set_icon(IconName::Folder, 16., kd::ACCENT_HOVER))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(hex_color(kd::FG))
                                        .truncate()
                                        .child("Set Notes Vault Root"),
                                ),
                        )
                        .child(
                            div()
                                .id("fp-close-x")
                                .w(px(30.))
                                .h(px(30.))
                                .rounded(px(7.))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.close_folder_picker(window, cx);
                                }))
                                .child(set_icon(IconName::Close, 16., kd::FG_SUBTLE)),
                        ),
                )
                .child(
                    v_flex()
                        .w_full()
                        .px_3()
                        .py_2p5()
                        .gap_2()
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .child(
                            h_flex()
                                .w_full()
                                .gap_1p5()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            Input::new(&app.folder_picker_input)
                                                .small()
                                                .cleanable(true),
                                        ),
                                )
                                .child(
                                    Button::new("fp-apply-path")
                                        .primary()
                                        .small()
                                        .label("Set")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_folder_picker_input(window, cx);
                                        })),
                                ),
                        )
                        .when_some(error, |col, msg| {
                            col.child(
                                h_flex()
                                    .w_full()
                                    .px_2p5()
                                    .py_1p5()
                                    .rounded(px(7.))
                                    .bg(hex_alpha(0xe4687624))
                                    .border_1()
                                    .border_color(hex_alpha(0xe4687660))
                                    .justify_between()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_xs()
                                            .text_color(hex_color(kd::RED))
                                            .child(msg),
                                    )
                                    .when(create_candidate.is_some(), |row| {
                                        row.child(
                                            Button::new("fp-create-dir")
                                                .primary()
                                                .small()
                                                .label("Create & Use")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.create_and_apply_folder_picker_path(
                                                        window,
                                                        cx,
                                                    );
                                                })),
                                        )
                                    }),
                            )
                        })
                        .when(!quick_jumps.is_empty(), |col| {
                            col.child(
                                h_flex()
                                    .w_full()
                                    .gap_1p5()
                                    .flex_wrap()
                                    .children(quick_jumps.into_iter().enumerate().map(
                                        |(ix, (lbl, path))| {
                                            let active = current_dir == path;
                                            div()
                                                .id(SharedString::from(format!("fp-jmp-{ix}")))
                                                .px_2()
                                                .py_1()
                                                .rounded(px(6.))
                                                .cursor_pointer()
                                                .bg(if active {
                                                    accent_alpha(0x34)
                                                } else {
                                                    hex_color(kd::BG_PANEL)
                                                })
                                                .border_1()
                                                .border_color(hex_color(if active {
                                                    kd::ACCENT
                                                } else {
                                                    kd::BORDER_SUBTLE
                                                }))
                                                .text_size(px(11.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(hex_color(if active {
                                                    kd::ACCENT_HOVER
                                                } else {
                                                    kd::FG
                                                }))
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    this.browse_folder_picker_to(
                                                        path.clone(),
                                                        window,
                                                        cx,
                                                    );
                                                }))
                                                .child(lbl)
                                        },
                                    )),
                            )
                        })
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .id("fp-go-up")
                                        .px_2()
                                        .py_1()
                                        .rounded(px(6.))
                                        .bg(hex_color(kd::BG_PANEL))
                                        .border_1()
                                        .border_color(hex_color(kd::BORDER_SUBTLE))
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(hex_color(if can_go_up {
                                            kd::FG
                                        } else {
                                            kd::FG_MUTED
                                        }))
                                        .when(can_go_up, |d| {
                                            d.cursor_pointer().on_click(cx.listener(
                                                |this, _, window, cx| {
                                                    this.folder_picker_go_up(window, cx);
                                                },
                                            ))
                                        })
                                        .child(set_icon(
                                            IconName::ArrowLeft,
                                            12.,
                                            if can_go_up { kd::FG } else { kd::FG_MUTED },
                                        ))
                                        .child("Up"),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(11.))
                                        .text_color(hex_color(kd::FG_SUBTLE))
                                        .truncate()
                                        .child(current_display),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(11.))
                                        .text_color(hex_color(kd::ACCENT_HOVER))
                                        .child(match_label),
                                ),
                        ),
                )
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .bg(hex_color(kd::BG_MAIN))
                        .child(
                            div()
                                .id("fp-subdir-scroll")
                                .size_full()
                                .overflow_y_scroll()
                                .track_scroll(&app.folder_picker_scroll)
                                .child(if subdirs.is_empty() {
                                    div()
                                        .w_full()
                                        .p_4()
                                        .text_xs()
                                        .text_color(hex_color(kd::FG_MUTED))
                                        .child("No subfolders inside this directory.")
                                        .into_any_element()
                                } else {
                                    v_flex()
                                        .w_full()
                                        .p_2()
                                        .gap_1()
                                        .children(subdirs.into_iter().enumerate().map(
                                            |(ix, (name, path))| {
                                                div()
                                                    .id(SharedString::from(format!("fp-dir-{ix}")))
                                                    .w_full()
                                                    .min_h(px(42.))
                                                    .px_3()
                                                    .py_2()
                                                    .rounded(px(8.))
                                                    .cursor_pointer()
                                                    .bg(hex_color(kd::BG_SIDEBAR))
                                                    .border_1()
                                                    .border_color(hex_color(kd::BORDER_SUBTLE))
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap_2()
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.browse_folder_picker_to(
                                                                path.clone(),
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    ))
                                                    .child(
                                                        h_flex()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .items_center()
                                                            .gap_2()
                                                            .child(set_icon(
                                                                IconName::Folder,
                                                                15.,
                                                                kd::ACCENT_HOVER,
                                                            ))
                                                            .child(
                                                                div()
                                                                    .flex_1()
                                                                    .min_w_0()
                                                                    .text_xs()
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .text_color(hex_color(kd::FG))
                                                                    .truncate()
                                                                    .child(name),
                                                            ),
                                                    )
                                                    .child(set_icon(
                                                        IconName::ChevronRight,
                                                        14.,
                                                        kd::FG_MUTED,
                                                    ))
                                            },
                                        ))
                                        .into_any_element()
                                }),
                        )
                        .child(Scrollbar::vertical(&app.folder_picker_scroll)),
                )
                .child(
                    h_flex()
                        .w_full()
                        .px_3p5()
                        .py_2p5()
                        .bg(hex_color(kd::BG_DEEP))
                        .border_t_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .justify_between()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("fp-cancel")
                                .secondary()
                                .small()
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.close_folder_picker(window, cx);
                                })),
                        )
                        .child(
                            Button::new("fp-use-folder")
                                .primary()
                                .small()
                                .label("Use This Folder")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.apply_folder_picker_input(window, cx);
                                })),
                        ),
                ),
        )
        .when(bottom_inset > 0.0, |col| {
            col.child(div().w_full().h(px(bottom_inset)).flex_shrink_0())
        })
        .into_any_element()
}
