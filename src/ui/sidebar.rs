use super::{
    NewItemTemplate, PaneSide, SidebarTab, VKarmaApp, file_kind_color, modals,
};
use crate::theme::{accent_alpha, hex_color, kd};
use crate::vault::{TreeNode, duplicate_file};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AppContext as _, ClickEvent, Context, Focusable as _, FontWeight, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone)]
struct DraggedTreeEntry {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

struct DraggedTreePreview {
    name: String,
    is_dir: bool,
}

impl Render for DraggedTreePreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .h(px(26.))
            .px_2p5()
            .gap_1p5()
            .items_center()
            .rounded(px(5.))
            .bg(hex_color(kd::BG_SIDEBAR))
            .border_1()
            .border_color(hex_color(kd::ACCENT))
            .opacity(0.92)
            .child(
                Icon::new(if self.is_dir {
                    IconName::Folder
                } else {
                    IconName::FileText
                })
                .xsmall()
                .text_color(if self.is_dir {
                    hex_color(kd::YELLOW)
                } else {
                    hex_color(kd::ACCENT)
                }),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(hex_color(kd::FG))
                    .child(self.name.clone()),
            )
    }
}

fn can_drop_onto_dir(src: &Path, dest_dir: &Path) -> bool {
    if src == dest_dir || dest_dir.starts_with(src) {
        return false;
    }
    src.parent() != Some(dest_dir)
}

pub fn render_sidebar(app: &VKarmaApp, cx: &mut Context<VKarmaApp>) -> impl IntoElement {
    let sidebar_bg = hex_color(kd::BG_SIDEBAR);
    let border = hex_color(kd::BORDER_SUBTLE);
    let fg_muted = hex_color(kd::FG_MUTED);
    let ws_root = app.workspace_root.clone();

    let header = h_flex()
        .h(px(36.))
        .w_full()
        .px_2()
        .gap_1()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(border)
        .child(
            h_flex()
                .gap_1()
                .child(
                    Button::new("sb-tab-files")
                        .with_variant(if app.sidebar_tab == SidebarTab::Files {
                            ButtonVariant::Secondary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .label("Files")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sidebar_tab = SidebarTab::Files;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("sb-tab-search")
                        .with_variant(if app.sidebar_tab == SidebarTab::Search {
                            ButtonVariant::Secondary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(IconName::Search)
                        .label("Search")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.sidebar_tab = SidebarTab::Search;
                            this.search_input.focus_handle(cx).focus(window, cx);
                            cx.notify();
                        })),
                ),
        )
        .child(
            h_flex()
                .gap_0p5()
                .child(
                    Button::new("sb-hidden-toggle")
                        .with_variant(if app.config.show_hidden_files {
                            ButtonVariant::Secondary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .xsmall()
                        .icon(if app.config.show_hidden_files {
                            IconName::Eye
                        } else {
                            IconName::EyeOff
                        })
                        .tooltip("Toggle Hidden Files")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.config.show_hidden_files = !this.config.show_hidden_files;
                            let _ = this.config.save();
                            this.refresh_vault_index(cx);
                        })),
                )
                .child(
                    Button::new("sb-refresh")
                        .ghost()
                        .xsmall()
                        .icon(IconName::RefreshCw)
                        .tooltip("Refresh Vault Index")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.refresh_vault_index(cx);
                        })),
                ),
        );

    let body = match app.sidebar_tab {
        SidebarTab::Files => {
            let quick_create = h_flex()
                .px_2()
                .py_1p5()
                .gap_1()
                .border_b_1()
                .border_color(border)
                .child(
                    Button::new("sb-new-note")
                        .ghost()
                        .xsmall()
                        .icon(IconName::FilePlus)
                        .tooltip("New Markdown Note")
                        .on_click({
                            let dir = ws_root.clone();
                            cx.listener(move |_, _, window, cx| {
                                modals::open_new_item_dialog(
                                    cx.entity(),
                                    dir.clone(),
                                    NewItemTemplate::Markdown,
                                    window,
                                    cx,
                                );
                            })
                        }),
                )
                .child(
                    Button::new("sb-new-typ")
                        .ghost()
                        .xsmall()
                        .icon(IconName::BookOpen)
                        .tooltip("New Typst Document")
                        .on_click({
                            let dir = ws_root.clone();
                            cx.listener(move |_, _, window, cx| {
                                modals::open_new_item_dialog(
                                    cx.entity(),
                                    dir.clone(),
                                    NewItemTemplate::Typst,
                                    window,
                                    cx,
                                );
                            })
                        }),
                )
                .child(
                    Button::new("sb-new-xlsx")
                        .ghost()
                        .xsmall()
                        .icon(IconName::FileSpreadsheet)
                        .tooltip("New Spreadsheet (.xlsx)")
                        .on_click({
                            let dir = ws_root.clone();
                            cx.listener(move |_, _, window, cx| {
                                modals::open_new_item_dialog(
                                    cx.entity(),
                                    dir.clone(),
                                    NewItemTemplate::Spreadsheet,
                                    window,
                                    cx,
                                );
                            })
                        }),
                )
                .child(
                    Button::new("sb-new-fountain")
                        .ghost()
                        .xsmall()
                        .icon(IconName::FileText)
                        .tooltip("New Screenplay (.fountain)")
                        .on_click({
                            let dir = ws_root.clone();
                            cx.listener(move |_, _, window, cx| {
                                modals::open_new_item_dialog(
                                    cx.entity(),
                                    dir.clone(),
                                    NewItemTemplate::Fountain,
                                    window,
                                    cx,
                                );
                            })
                        }),
                )
                .child(
                    Button::new("sb-new-folder")
                        .ghost()
                        .xsmall()
                        .icon(IconName::FolderPlus)
                        .tooltip("New Folder")
                        .on_click({
                            let dir = ws_root.clone();
                            cx.listener(move |_, _, window, cx| {
                                modals::open_new_item_dialog(
                                    cx.entity(),
                                    dir.clone(),
                                    NewItemTemplate::Folder,
                                    window,
                                    cx,
                                );
                            })
                        }),
                );

            let active_file = app.active_tab_ref().map(|t| t.path.clone());
            let mut rows = Vec::new();
            collect_tree_rows(app, &app.vault.tree, 0, active_file.as_ref(), &mut rows, cx);
            let root_check = ws_root.clone();
            let root_drop = ws_root.clone();

            v_flex()
                .flex_1()
                .min_h_0()
                .track_focus(&app.sidebar_focus)
                .child(quick_create)
                .child(
                    div()
                        .id("vault-file-tree-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .p_1()
                        .rounded(px(4.))
                        .drag_over::<DraggedTreeEntry>(move |style, drag, _, _| {
                            if can_drop_onto_dir(&drag.path, &root_check) {
                                style
                                    .bg(accent_alpha(0x14))
                                    .border_1()
                                    .border_color(hex_color(kd::ACCENT))
                            } else {
                                style
                            }
                        })
                        .on_drop(cx.listener(move |this, drag: &DraggedTreeEntry, window, cx| {
                            this.move_fs_entry(drag.path.clone(), root_drop.clone(), window, cx);
                        }))
                        .child(v_flex().gap_0p5().children(rows)),
                )
                .into_any_element()
        }
        SidebarTab::Search => {
            let hit_rows = app.search_hits.iter().enumerate().map(|(ix, hit)| {
                let path = hit.path.clone();
                let rel = hit.rel_path.clone();
                let line = hit.line_text.clone();
                let line_num = hit.line_num;
                let is_title = hit.is_title_match;

                div()
                    .id(("search-hit", ix))
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .hover(|s| s.bg(hex_color(kd::BG_PANEL)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let pane = this.active_pane;
                        this.open_file(path.clone(), pane, window, cx);
                    }))
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_xs()
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .text_color(hex_color(kd::ACCENT))
                                            .truncate()
                                            .child(rel),
                                    )
                                    .child(if is_title {
                                        div()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .text_color(hex_color(kd::GREEN))
                                            .child("Title")
                                    } else {
                                        div()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(fg_muted)
                                            .child(format!("L{line_num}"))
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_SUBTLE))
                                    .truncate()
                                    .child(line),
                            ),
                    )
            });

            let has_query = app.search_input.read(cx).value().trim().chars().count() >= 2;
            v_flex()
                .flex_1()
                .min_h_0()
                .child(
                    div()
                        .p_2()
                        .border_b_1()
                        .border_color(border)
                        .child(
                            Input::new(&app.search_input)
                                .small()
                                .prefix(Icon::new(IconName::Search).xsmall().text_color(fg_muted)),
                        ),
                )
                .child(
                    div()
                        .id("vault-search-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .p_1()
                        .child(if app.search_hits.is_empty() {
                            div()
                                .p_3()
                                .text_xs()
                                .text_color(fg_muted)
                                .child(if has_query {
                                    "No title or content matches found."
                                } else {
                                    "Type 2+ characters to search titles and contents across notes, Typst docs, CSVs, and configs."
                                })
                                .into_any_element()
                        } else {
                            v_flex().gap_0p5().children(hit_rows).into_any_element()
                        }),
                )
                .into_any_element()
        }
    };

    v_flex()
        .size_full()
        .bg(sidebar_bg)
        .border_r_1()
        .border_color(border)
        .child(header)
        .child(body)
}

fn collect_tree_rows(
    app: &VKarmaApp,
    nodes: &[TreeNode],
    depth: usize,
    active_file: Option<&PathBuf>,
    out: &mut Vec<gpui_kit::AnyElement>,
    cx: &mut Context<VKarmaApp>,
) {
    for node in nodes {
        let indent_px = (depth as f32) * 14.0 + 6.0;
        let node_path = node.path.clone();
        let node_name = node.name.clone();
        let is_expanded = app.expanded_dirs.contains(&node_path);
        let is_active = active_file.is_some_and(|p| p == &node_path);
        let is_selected = app.selected_tree_path.as_ref() == Some(&node_path);
        let rename_input = app
            .inline_rename
            .as_ref()
            .filter(|r| r.target_path == node_path)
            .map(|r| r.input.clone());
        let app_entity = cx.entity();

        if node.is_dir {
            let dir_for_menu = node_path.clone();
            let dir_for_click = node_path.clone();
            let dir_for_drag = node_path.clone();
            let dir_for_hover = node_path.clone();
            let dir_for_drop = node_path.clone();
            let name_for_drag = node_name.clone();
            let row_id: SharedString = format!("tree-dir-{}", node.rel_path).into();

            if let Some(ren_input) = rename_input {
                let row = h_flex()
                    .id(row_id)
                    .w_full()
                    .h(px(26.))
                    .pl(px(indent_px))
                    .pr_2()
                    .gap_1p5()
                    .items_center()
                    .rounded(px(4.))
                    .bg(hex_color(kd::BG_PANEL))
                    .child(
                        Icon::new(if is_expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall()
                        .text_color(hex_color(kd::FG_MUTED)),
                    )
                    .child(
                        Icon::new(IconName::FolderOpen)
                            .xsmall()
                            .text_color(hex_color(kd::YELLOW)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                                if ev.keystroke.key == "escape" {
                                    cx.stop_propagation();
                                    this.cancel_inline_rename(window, cx);
                                }
                            }))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(Input::new(&ren_input).xsmall()),
                    );
                out.push(row.into_any_element());
                if is_expanded {
                    collect_tree_rows(app, &node.children, depth + 1, active_file, out, cx);
                }
                continue;
            }

            let row = div()
                .id(row_id)
                .w_full()
                .h(px(26.))
                .pl(px(indent_px))
                .pr_2()
                .rounded(px(4.))
                .flex()
                .items_center()
                .gap_1p5()
                .cursor_pointer()
                .when(is_selected, |s| s.bg(accent_alpha(0x24)))
                .hover(move |s| {
                    if is_selected {
                        s.bg(accent_alpha(0x35))
                    } else {
                        s.bg(hex_color(kd::BG_PANEL))
                    }
                })
                .on_drag(
                    DraggedTreeEntry {
                        path: dir_for_drag,
                        name: name_for_drag,
                        is_dir: true,
                    },
                    |drag, _, _, cx| {
                        cx.new(|_| DraggedTreePreview {
                            name: drag.name.clone(),
                            is_dir: drag.is_dir,
                        })
                    },
                )
                .drag_over::<DraggedTreeEntry>(move |style, drag, _, _| {
                    if can_drop_onto_dir(&drag.path, &dir_for_hover) {
                        style
                            .bg(accent_alpha(0x38))
                            .border_1()
                            .border_color(hex_color(kd::ACCENT))
                    } else {
                        style
                    }
                })
                .on_drop(cx.listener(move |this, drag: &DraggedTreeEntry, window, cx| {
                    cx.stop_propagation();
                    this.move_fs_entry(drag.path.clone(), dir_for_drop.clone(), window, cx);
                }))
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    this.selected_tree_path = Some(dir_for_click.clone());
                    if ev.click_count() >= 2 {
                        // Revert click-1 expand flip before starting inline rename.
                        if !this.expanded_dirs.remove(&dir_for_click) {
                            this.expanded_dirs.insert(dir_for_click.clone());
                        }
                        this.start_inline_rename(dir_for_click.clone(), window, cx);
                    } else {
                        if !this.expanded_dirs.remove(&dir_for_click) {
                            this.expanded_dirs.insert(dir_for_click.clone());
                        }
                        this.sidebar_focus.focus(window, cx);
                        cx.notify();
                    }
                }))
                .context_menu(move |menu, _, _| {
                    let app_md = app_entity.clone();
                    let app_typ = app_entity.clone();
                    let app_xls = app_entity.clone();
                    let app_fnt = app_entity.clone();
                    let app_fld = app_entity.clone();
                    let app_ren = app_entity.clone();
                    let app_del = app_entity.clone();

                    let d1 = dir_for_menu.clone();
                    let d2 = dir_for_menu.clone();
                    let d3 = dir_for_menu.clone();
                    let d_fnt = dir_for_menu.clone();
                    let d4 = dir_for_menu.clone();
                    let d5 = dir_for_menu.clone();
                    let d6 = dir_for_menu.clone();
                    let d7 = dir_for_menu.clone();

                    menu.label(node_name.clone())
                        .item(
                            PopupMenuItem::new("New Note Here…")
                                .icon(IconName::FilePlus)
                                .on_click(move |_, window, cx| {
                                    modals::open_new_item_dialog(
                                        app_md.clone(),
                                        d1.clone(),
                                        NewItemTemplate::Markdown,
                                        window,
                                        cx,
                                    );
                                }),
                        )
                        .item(
                            PopupMenuItem::new("New Typst Here…")
                                .icon(IconName::BookOpen)
                                .on_click(move |_, window, cx| {
                                    modals::open_new_item_dialog(
                                        app_typ.clone(),
                                        d2.clone(),
                                        NewItemTemplate::Typst,
                                        window,
                                        cx,
                                    );
                                }),
                        )
                        .item(
                            PopupMenuItem::new("New Sheet Here…")
                                .icon(IconName::FileSpreadsheet)
                                .on_click(move |_, window, cx| {
                                    modals::open_new_item_dialog(
                                        app_xls.clone(),
                                        d3.clone(),
                                        NewItemTemplate::Spreadsheet,
                                        window,
                                        cx,
                                    );
                                }),
                        )
                        .item(
                            PopupMenuItem::new("New Screenplay Here…")
                                .icon(IconName::FileText)
                                .on_click(move |_, window, cx| {
                                    modals::open_new_item_dialog(
                                        app_fnt.clone(),
                                        d_fnt.clone(),
                                        NewItemTemplate::Fountain,
                                        window,
                                        cx,
                                    );
                                }),
                        )
                        .item(
                            PopupMenuItem::new("New Canvas Here…")
                                .icon(IconName::Columns2)
                                .on_click({
                                    let app_cnv = app_entity.clone();
                                    let d_cnv = dir_for_menu.clone();
                                    move |_, window, cx| {
                                        modals::open_new_item_dialog(
                                            app_cnv.clone(),
                                            d_cnv.clone(),
                                            NewItemTemplate::Canvas,
                                            window,
                                            cx,
                                        );
                                    }
                                }),
                        )
                        .item(
                            PopupMenuItem::new("New Kanban Here…")
                                .icon(IconName::Check)
                                .on_click({
                                    let app_kbn = app_entity.clone();
                                    let d_kbn = dir_for_menu.clone();
                                    move |_, window, cx| {
                                        modals::open_new_item_dialog(
                                            app_kbn.clone(),
                                            d_kbn.clone(),
                                            NewItemTemplate::Kanban,
                                            window,
                                            cx,
                                        );
                                    }
                                }),
                        )
                        .item(
                            PopupMenuItem::new("New Mermaid Diagram…")
                                .icon(IconName::Split)
                                .on_click({
                                    let app_mmd = app_entity.clone();
                                    let d_mmd = dir_for_menu.clone();
                                    move |_, window, cx| {
                                        modals::open_new_item_dialog(
                                            app_mmd.clone(),
                                            d_mmd.clone(),
                                            NewItemTemplate::Mermaid,
                                            window,
                                            cx,
                                        );
                                    }
                                }),
                        )
                        .item(
                            PopupMenuItem::new("New SVG Graphic…")
                                .icon(IconName::Code)
                                .on_click({
                                    let app_svg = app_entity.clone();
                                    let d_svg = dir_for_menu.clone();
                                    move |_, window, cx| {
                                        modals::open_new_item_dialog(
                                            app_svg.clone(),
                                            d_svg.clone(),
                                            NewItemTemplate::Svg,
                                            window,
                                            cx,
                                        );
                                    }
                                }),
                        )
                        .item(
                            PopupMenuItem::new("New Folder Here…")
                                .icon(IconName::FolderPlus)
                                .on_click(move |_, window, cx| {
                                    modals::open_new_item_dialog(
                                        app_fld.clone(),
                                        d4.clone(),
                                        NewItemTemplate::Folder,
                                        window,
                                        cx,
                                    );
                                }),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new("Rename")
                                .icon(IconName::Pencil)
                                .on_click(move |_, window, cx| {
                                    app_ren.update(cx, |app, cx| {
                                        app.start_inline_rename(d5.clone(), window, cx);
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Reveal in File Manager")
                                .icon(IconName::ExternalLink)
                                .on_click(move |_, _, _| {
                                    let _ = Command::new("xdg-open").arg(&d6).spawn();
                                }),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new("Delete Folder…")
                                .icon(IconName::Trash)
                                .on_click(move |_, window, cx| {
                                    modals::open_delete_confirm_dialog(
                                        app_del.clone(),
                                        d7.clone(),
                                        window,
                                        cx,
                                    );
                                }),
                        )
                })
                .child(
                    Icon::new(if is_expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .xsmall()
                    .text_color(hex_color(kd::FG_MUTED)),
                )
                .child(
                    Icon::new(if is_expanded {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    })
                    .xsmall()
                    .text_color(hex_color(kd::YELLOW)),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .text_color(hex_color(kd::FG))
                        .truncate()
                        .child(node.name.clone()),
                );

            out.push(row.into_any_element());
            if is_expanded {
                collect_tree_rows(app, &node.children, depth + 1, active_file, out, cx);
            }
        } else {
            let kind = node.kind.unwrap_or(crate::vault::FileKind::PlainText);
            let badge_color = file_kind_color(kind);
            let badge = kind.short_badge();
            let path_click = node_path.clone();
            let path_mid = node_path.clone();
            let path_menu = node_path.clone();
            let path_drag = node_path.clone();
            let name_drag = node_name.clone();
            let parent_dir = node_path.parent().map(Path::to_path_buf);
            let row_id: SharedString = format!("tree-file-{}", node.rel_path).into();

            if let Some(ren_input) = rename_input {
                let row = h_flex()
                    .id(row_id)
                    .w_full()
                    .h(px(25.))
                    .pl(px(indent_px + 14.0))
                    .pr_2()
                    .gap_1p5()
                    .items_center()
                    .rounded(px(4.))
                    .bg(hex_color(kd::BG_PANEL))
                    .child(
                        div()
                            .w(px(6.))
                            .h(px(6.))
                            .rounded_full()
                            .flex_shrink_0()
                            .bg(badge_color),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                                if ev.keystroke.key == "escape" {
                                    cx.stop_propagation();
                                    this.cancel_inline_rename(window, cx);
                                }
                            }))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(Input::new(&ren_input).xsmall()),
                    );
                out.push(row.into_any_element());
                continue;
            }

            let row = div()
                .id(row_id)
                .w_full()
                .h(px(25.))
                .pl(px(indent_px + 14.0))
                .pr_2()
                .rounded(px(4.))
                .flex()
                .items_center()
                .justify_between()
                .gap_1p5()
                .cursor_pointer()
                .when(is_active, |s| s.bg(accent_alpha(0x29)))
                .when(!is_active && is_selected, |s| s.bg(accent_alpha(0x1e)))
                .hover(|s| {
                    if is_active {
                        s.bg(accent_alpha(0x35))
                    } else {
                        s.bg(hex_color(kd::BG_PANEL))
                    }
                })
                .on_drag(
                    DraggedTreeEntry {
                        path: path_drag,
                        name: name_drag,
                        is_dir: false,
                    },
                    |drag, _, _, cx| {
                        cx.new(|_| DraggedTreePreview {
                            name: drag.name.clone(),
                            is_dir: drag.is_dir,
                        })
                    },
                )
                .when_some(parent_dir, |el, pdir| {
                    let check_dir = pdir.clone();
                    let drop_dir = pdir;
                    el.drag_over::<DraggedTreeEntry>(move |style, drag, _, _| {
                        if can_drop_onto_dir(&drag.path, &check_dir) {
                            style.bg(accent_alpha(0x24))
                        } else {
                            style
                        }
                    })
                    .on_drop(cx.listener(move |this, drag: &DraggedTreeEntry, window, cx| {
                        cx.stop_propagation();
                        this.move_fs_entry(drag.path.clone(), drop_dir.clone(), window, cx);
                    }))
                })
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    this.selected_tree_path = Some(path_click.clone());
                    if ev.click_count() >= 2 {
                        this.start_inline_rename(path_click.clone(), window, cx);
                    } else {
                        let pane = this.active_pane;
                        this.open_file(path_click.clone(), pane, window, cx);
                    }
                }))
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _, window, cx| {
                        this.open_file(path_mid.clone(), PaneSide::Right, window, cx);
                    }),
                )
                .context_menu(move |menu, _, _| {
                    let app_r = app_entity.clone();
                    let app_ren = app_entity.clone();
                    let app_dup = app_entity.clone();
                    let app_del = app_entity.clone();

                    let p1 = path_menu.clone();
                    let p2 = path_menu.clone();
                    let p3 = path_menu.clone();
                    let p4 = path_menu.clone();
                    let p5 = path_menu.clone();

                    menu.label(node_name.clone())
                        .item(
                            PopupMenuItem::new("Open in Right Split")
                                .icon(IconName::Columns2)
                                .on_click(move |_, window, cx| {
                                    app_r.update(cx, |app, cx| {
                                        app.open_file(p1.clone(), PaneSide::Right, window, cx);
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Rename")
                                .icon(IconName::Pencil)
                                .on_click(move |_, window, cx| {
                                    app_ren.update(cx, |app, cx| {
                                        app.start_inline_rename(p2.clone(), window, cx);
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Duplicate File")
                                .icon(IconName::Copy)
                                .on_click(move |_, window, cx| {
                                    if let Ok(copy_path) = duplicate_file(&p3) {
                                        app_dup.update(cx, |app, cx| {
                                            app.refresh_vault_index(cx);
                                            let pane = app.active_pane;
                                            app.open_file(copy_path, pane, window, cx);
                                        });
                                    }
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Reveal in File Manager")
                                .icon(IconName::ExternalLink)
                                .on_click(move |_, _, _| {
                                    if let Some(parent) = p4.parent() {
                                        let _ = Command::new("xdg-open").arg(parent).spawn();
                                    }
                                }),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new("Delete File…")
                                .icon(IconName::Trash)
                                .on_click(move |_, window, cx| {
                                    modals::open_delete_confirm_dialog(
                                        app_del.clone(),
                                        p5.clone(),
                                        window,
                                        cx,
                                    );
                                }),
                        )
                })
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_1p5()
                        .items_center()
                        .child(
                            div()
                                .w(px(6.))
                                .h(px(6.))
                                .rounded_full()
                                .flex_shrink_0()
                                .bg(badge_color),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(if is_active {
                                    hex_color(kd::FG)
                                } else {
                                    hex_color(kd::FG_SUBTLE)
                                })
                                .truncate()
                                .child(node.name.clone()),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(badge_color)
                        .flex_shrink_0()
                        .child(badge),
                );

            out.push(row.into_any_element());
        }
    }
}
