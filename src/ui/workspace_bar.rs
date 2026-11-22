use super::{VKarmaApp, modals};
use crate::config::WorkspacePreset;
use crate::theme::{hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex};
use gpui_kit::{
    AppContext as _, Context, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use std::path::Path;

#[derive(Clone)]
struct DraggedWorkspace {
    from_idx: usize,
    name: String,
    accent: Hsla,
}

struct DraggedWorkspacePreview {
    name: String,
    accent: Hsla,
}

impl Render for DraggedWorkspacePreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .h(px(28.))
            .px_3()
            .gap_1p5()
            .items_center()
            .rounded(px(6.))
            .bg(hex_color(kd::BG_SIDEBAR))
            .border_1()
            .border_color(self.accent)
            .opacity(0.92)
            .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(self.accent))
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(hex_color(kd::FG))
                    .child(self.name.clone()),
            )
    }
}

pub fn preset_accent_color(preset: WorkspacePreset) -> Hsla {
    match preset {
        WorkspacePreset::Notes => hex_color(kd::BLUE),
        WorkspacePreset::Writing => hex_color(kd::ORANGE),
        WorkspacePreset::Code => hex_color(kd::YELLOW),
        WorkspacePreset::Sheets => hex_color(kd::GREEN),
        WorkspacePreset::Typst => hex_color(kd::AQUA),
    }
}

fn format_short_root(root: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rel) = root.strip_prefix(&home)
    {
        let s = rel.to_string_lossy();
        if s.is_empty() {
            return "~".to_string();
        }
        return format!("~/{s}");
    }
    root.display().to_string()
}

pub fn render_workspace_bar(app: &VKarmaApp, cx: &mut Context<VKarmaApp>) -> impl IntoElement {
    let deep_bg = hex_color(kd::BG_DEEP);
    let active_bg = hex_color(kd::BG_SIDEBAR);
    let panel_bg = hex_color(kd::BG_PANEL);
    let border = hex_color(kd::BORDER_SUBTLE);
    let fg = hex_color(kd::FG);
    let fg_muted = hex_color(kd::FG_MUTED);

    let app_entity = cx.entity();
    let total = app.workspaces.len();
    let active_idx = app.active_workspace;
    let active_root_label = app
        .workspaces
        .get(active_idx)
        .map(|w| format_short_root(&w.root))
        .unwrap_or_default();
    let default_add_root = app.workspace_root.clone();

    let ws_tabs = app.workspaces.iter().enumerate().map(|(idx, ws)| {
        let is_active = idx == active_idx;
        let accent = preset_accent_color(ws.preset);
        let dirty = app.workspace_has_dirty_tabs(idx);
        let ws_id = ws.id.clone();
        let ws_name = ws.name.clone();
        let ws_name_drag = ws.name.clone();
        let ws_name_menu = ws.name.clone();
        let ws_name_ren = ws.name.clone();
        let ws_root_tip = ws.root.display().to_string();
        let app_menu = app_entity.clone();

        div()
            .id(SharedString::from(format!("ws-tab-{ws_id}-{idx}")))
            .h(px(28.))
            .px_3()
            .rounded_t(px(6.))
            .flex()
            .items_center()
            .gap_1p5()
            .cursor_pointer()
            .flex_shrink_0()
            .border_t_2()
            .border_color(if is_active {
                accent
            } else {
                gpui_kit::transparent_black()
            })
            .bg(if is_active { active_bg } else { deep_bg })
            .hover(move |s| if is_active { s } else { s.bg(panel_bg) })
            .on_drag(
                DraggedWorkspace {
                    from_idx: idx,
                    name: ws_name_drag,
                    accent,
                },
                |drag, _, _, cx| {
                    cx.new(|_| DraggedWorkspacePreview {
                        name: drag.name.clone(),
                        accent: drag.accent,
                    })
                },
            )
            .drag_over::<DraggedWorkspace>(move |style, drag, _, _| {
                if drag.from_idx == idx {
                    style
                } else if drag.from_idx > idx {
                    style
                        .border_l_2()
                        .border_color(hex_color(kd::BLUE))
                        .bg(panel_bg)
                } else {
                    style
                        .border_r_2()
                        .border_color(hex_color(kd::BLUE))
                        .bg(panel_bg)
                }
            })
            .on_drop(cx.listener(move |this, drag: &DraggedWorkspace, _, cx| {
                this.reorder_workspace(drag.from_idx, idx, cx);
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.switch_workspace(idx, window, cx);
            }))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.remove_workspace(idx, window, cx);
                }),
            )
            .context_menu(move |menu, _, _| {
                let app_ren = app_menu.clone();
                let app_dir = app_menu.clone();
                let app_dup = app_menu.clone();
                let app_ml = app_menu.clone();
                let app_mr = app_menu.clone();
                let app_del = app_menu.clone();
                let rename_cur = ws_name_ren.clone();

                menu.label(format!("{ws_name_menu}  ({ws_root_tip})"))
                    .item(
                        PopupMenuItem::new("Rename Workspace…")
                            .icon(IconName::Pencil)
                            .on_click(move |_, window, cx| {
                                modals::open_rename_workspace_dialog(
                                    app_ren.clone(),
                                    idx,
                                    rename_cur.clone(),
                                    window,
                                    cx,
                                );
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Change Folder…")
                            .icon(IconName::FolderOpen)
                            .on_click(move |_, window, cx| {
                                app_dir.update(cx, |app, cx| {
                                    app.prompt_rebind_workspace_folder(idx, window, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Duplicate Workspace")
                            .icon(IconName::Copy)
                            .on_click(move |_, window, cx| {
                                app_dup.update(cx, |app, cx| {
                                    app.duplicate_workspace(idx, window, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Move Left")
                            .icon(IconName::ChevronLeft)
                            .disabled(idx == 0)
                            .on_click(move |_, _, cx| {
                                app_ml.update(cx, |app, cx| {
                                    app.move_workspace(idx, -1, cx);
                                });
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Move Right")
                            .icon(IconName::ChevronRight)
                            .disabled(idx + 1 >= total)
                            .on_click(move |_, _, cx| {
                                app_mr.update(cx, |app, cx| {
                                    app.move_workspace(idx, 1, cx);
                                });
                            }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("Delete Workspace")
                            .icon(IconName::Trash)
                            .disabled(total <= 1)
                            .on_click(move |_, window, cx| {
                                app_del.update(cx, |app, cx| {
                                    app.remove_workspace(idx, window, cx);
                                });
                            }),
                    )
            })
            .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(accent))
            .child(
                div()
                    .text_xs()
                    .font_weight(if is_active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if is_active { fg } else { fg_muted })
                    .child(ws_name),
            )
            .when(dirty, |this| {
                this.child(
                    div()
                        .w(px(6.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(hex_color(kd::YELLOW)),
                )
            })
    });

    h_flex()
        .h(px(34.))
        .w_full()
        .px_2()
        .gap_2()
        .items_end()
        .justify_between()
        .bg(deep_bg)
        .border_b_1()
        .border_color(border)
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_1p5()
                .items_end()
                .child(
                    div()
                        .pb(px(6.))
                        .px_1p5()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(hex_color(kd::BLUE))
                        .flex_shrink_0()
                        .child("VKarma"),
                )
                .child(
                    h_flex()
                        .id("workspace-tab-strip")
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .items_end()
                        .overflow_x_scrollbar()
                        .children(ws_tabs)
                        .child(
                            div()
                                .pb(px(3.))
                                .flex_shrink_0()
                                .on_drop(cx.listener(move |this, drag: &DraggedWorkspace, _, cx| {
                                    let last = this.workspaces.len().saturating_sub(1);
                                    this.reorder_workspace(drag.from_idx, last, cx);
                                }))
                                .child(
                                    Button::new("ws-add-btn")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Plus)
                                        .tooltip("New Workspace (Ctrl+Alt+N)")
                                        .on_click({
                                            let app_add = app_entity.clone();
                                            move |_, window, cx| {
                                                modals::open_new_workspace_dialog(
                                                    app_add.clone(),
                                                    default_add_root.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .h_full()
                                .drag_over::<DraggedWorkspace>(move |style, _, _, _| {
                                    style.border_l_2().border_color(hex_color(kd::BLUE))
                                })
                                .on_drop(cx.listener(move |this, drag: &DraggedWorkspace, _, cx| {
                                    let last = this.workspaces.len().saturating_sub(1);
                                    this.reorder_workspace(drag.from_idx, last, cx);
                                })),
                        ),
                ),
        )
        .child(
            div()
                .pb(px(6.))
                .pl_2()
                .flex_shrink_0()
                .max_w(px(340.))
                .truncate()
                .text_size(px(11.))
                .text_color(fg_muted)
                .child(active_root_label),
        )
}
