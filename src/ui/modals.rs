use super::{NewItemTemplate, PaneSide, QuickSwitcherState, TabKind, VKarmaApp, file_kind_color};
use crate::config::WorkspacePreset;
use crate::spreadsheet::SpreadsheetDoc;
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use crate::vault::{create_new_file, create_new_folder, delete_entry};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, PathPromptOptions, SharedString, StatefulInteractiveElement as _, Styled,
    Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

pub fn render_quick_switcher_overlay(
    app: &VKarmaApp,
    qs: &QuickSwitcherState,
    cx: &mut gpui_kit::Context<VKarmaApp>,
) -> impl IntoElement {
    let matches = app.vault.fuzzy_search_files(&qs.query, 15);
    let selected = qs.selected.min(matches.len().saturating_sub(1));

    div()
        .id("quick-switcher-backdrop")
        .absolute()
        .inset_0()
        .bg(hex_alpha(0x0e0b14a8))
        .flex()
        .justify_center()
        .items_start()
        .pt(px(72.))
        .on_mouse_down(
            gpui_kit::MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.quick_switcher = None;
                cx.notify();
            }),
        )
        .child(
            v_flex()
                .id("quick-switcher-box")
                .w(px(580.))
                .max_h(px(460.))
                .rounded(px(10.))
                .bg(hex_color(kd::BG_SIDEBAR))
                .border_1()
                .border_color(hex_color(kd::ACCENT))
                .shadow_lg()
                .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .capture_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                    let key = event.keystroke.key.as_str();
                    match key {
                        "escape" => {
                            this.quick_switcher = None;
                            cx.notify();
                            cx.stop_propagation();
                        }
                        "down" => {
                            if let Some(qs) = &mut this.quick_switcher {
                                let count = this.vault.fuzzy_search_files(&qs.query, 15).len();
                                if count > 0 {
                                    qs.selected = (qs.selected + 1).min(count - 1);
                                    cx.notify();
                                }
                            }
                            cx.stop_propagation();
                        }
                        "up" => {
                            if let Some(qs) = &mut this.quick_switcher {
                                qs.selected = qs.selected.saturating_sub(1);
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }
                        _ => {}
                    }
                }))
                .child(
                    div()
                        .p_3()
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .child(
                            Input::new(&qs.query_input)
                                .prefix(
                                    gpui_kit::component::Icon::new(IconName::Search)
                                        .small()
                                        .text_color(hex_color(kd::ACCENT)),
                                ),
                        ),
                )
                .child(
                    div()
                        .id("quick-switcher-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .p_1p5()
                        .child(if matches.is_empty() {
                            div()
                                .p_4()
                                .text_xs()
                                .text_color(hex_color(kd::FG_MUTED))
                                .child("No matching files in workspace.")
                                .into_any_element()
                        } else {
                            v_flex()
                                .gap_0p5()
                                .children(matches.into_iter().enumerate().map(|(ix, file)| {
                                    let is_sel = ix == selected;
                                    let path = file.path.clone();
                                    let path_r = file.path.clone();
                                    let badge = file.kind.short_badge();
                                    let color = file_kind_color(file.kind);

                                    div()
                                        .id(("qs-item", ix))
                                        .w_full()
                                        .px_2p5()
                                        .py_1p5()
                                        .rounded(px(6.))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap_2()
                                        .cursor_pointer()
                                        .when(is_sel, |s| s.bg(accent_alpha(0x2e)))
                                        .hover(|s| s.bg(hex_color(kd::BG_PANEL)))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.quick_switcher = None;
                                            let pane = this.active_pane;
                                            this.open_file(path.clone(), pane, window, cx);
                                        }))
                                        .on_mouse_down(
                                            gpui_kit::MouseButton::Middle,
                                            cx.listener(move |this, _, window, cx| {
                                                this.quick_switcher = None;
                                                this.open_file(
                                                    path_r.clone(),
                                                    PaneSide::Right,
                                                    window,
                                                    cx,
                                                );
                                            }),
                                        )
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .min_w_0()
                                                .items_center()
                                                .child(
                                                    div()
                                                        .px_1()
                                                        .rounded(px(4.))
                                                        .bg(hex_color(kd::BG_PANEL))
                                                        .text_size(px(10.))
                                                        .font_weight(gpui_kit::FontWeight::BOLD)
                                                        .text_color(color)
                                                        .child(badge),
                                                )
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(hex_color(kd::FG))
                                                        .truncate()
                                                        .child(file.name.clone()),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(hex_color(kd::FG_MUTED))
                                                .truncate()
                                                .child(file.rel_path.clone()),
                                        )
                                }))
                                .into_any_element()
                        }),
                )
                .child(
                    h_flex()
                        .px_3()
                        .py_1p5()
                        .justify_between()
                        .border_t_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .bg(hex_color(kd::BG_DEEP))
                        .text_size(px(11.))
                        .text_color(hex_color(kd::FG_MUTED))
                        .child("Enter: Open in active pane · Ctrl+Enter / Middle-click: Open in right split")
                        .child("Esc: Close"),
                ),
        )
}

fn subscribe_submit_on_enter(
    input: &Entity<InputState>,
    submit: Rc<dyn Fn(&mut Window, &mut App)>,
    window: &mut Window,
    cx: &mut App,
) -> Subscription {
    window.subscribe(input, cx, move |_, event: &InputEvent, window, cx| {
        if matches!(event, InputEvent::PressEnter { .. }) {
            submit(window, cx);
        }
    })
}

pub fn open_new_item_dialog(
    app_entity: Entity<VKarmaApp>,
    parent_dir: PathBuf,
    template: NewItemTemplate,
    window: &mut Window,
    cx: &mut App,
) {
    let name_input = cx.new(|cx| InputState::new(window, cx).placeholder(template.placeholder()));
    let initial_focus = Cell::new(true);
    let submitted = Rc::new(Cell::new(false));

    let submit: Rc<dyn Fn(&mut Window, &mut App)> = {
        let input_ok = name_input.clone();
        let app_ok = app_entity.clone();
        let dir_ok = parent_dir.clone();
        let done = submitted.clone();
        Rc::new(move |window, cx| {
            if done.get() {
                return;
            }
            let raw_name = input_ok.read(cx).value().trim().to_string();
            if raw_name.is_empty() {
                return;
            }
            let final_name = match template.extension() {
                Some(ext) if !raw_name.contains('.') => format!("{raw_name}.{ext}"),
                _ => raw_name,
            };
            let dest = dir_ok.join(&final_name);

            let res = match template {
                NewItemTemplate::Folder => create_new_folder(&dest),
                NewItemTemplate::Markdown => {
                    let title = dest
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Note");
                    create_new_file(&dest, &format!("# {title}\n\n"))
                }
                NewItemTemplate::Canvas => {
                    let mut doc = crate::formats::canvas::CanvasDoc::default();
                    let title = dest
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Canvas");
                    doc.add_text_node(0.0, 0.0, format!("# {title}\n\nDouble-click to edit card."));
                    create_new_file(&dest, &doc.to_json_pretty())
                }
                NewItemTemplate::Kanban => {
                    let title = dest
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Board");
                    let starter = format!(
                        "# {title}\n\n## Todo\n- [ ] First task\n\n## Doing\n\n## Done\n"
                    );
                    create_new_file(&dest, &starter)
                }
                NewItemTemplate::Mermaid => {
                    let starter = "flowchart TD\n    A[Start] --> B{Ready?}\n    B -->|Yes| C([Launch])\n    B -->|No| D[(Revise)]\n";
                    create_new_file(&dest, starter)
                }
                NewItemTemplate::Svg => {
                    let starter = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"240\" height=\"160\" viewBox=\"0 0 240 160\">\n  <rect x=\"20\" y=\"20\" width=\"200\" height=\"120\" rx=\"14\" fill=\"#222020\" stroke=\"#8ba4b0\" stroke-width=\"3\"/>\n  <text x=\"120\" y=\"86\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"18\" fill=\"#c5c9c5\">VKarma SVG</text>\n</svg>\n";
                    create_new_file(&dest, starter)
                }
                NewItemTemplate::Typst => {
                    let title = dest
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Document");
                    create_new_file(
                        &dest,
                        &format!(
                            "#set page(paper: \"a4\", margin: 2cm)\n#set text(size: 11pt)\n\n= {title}\n\nWrite Typst content here.\n"
                        ),
                    )
                }
                NewItemTemplate::Spreadsheet => {
                    SpreadsheetDoc::new_empty(&dest).and_then(|mut doc| doc.save(&dest))
                }
                NewItemTemplate::Fountain => {
                    let title = dest
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Untitled Screenplay");
                    let starter = format!(
                        "Title: {title}\nCredit: Written by\nAuthor: \nDraft date: \n\n===\n\nFADE IN:\n\nINT. LOCATION - DAY\n\nOpening beat.\n\nCHARACTER\nFirst line of dialogue.\n\nFADE OUT.\n"
                    );
                    create_new_file(&dest, &starter)
                }
            };

            match res {
                Ok(()) => {
                    done.set(true);
                    window.close_dialog(cx);
                    app_ok.update(cx, |app, cx| {
                        app.refresh_vault_index(cx);
                        if template != NewItemTemplate::Folder {
                            let pane = app.active_pane;
                            app.open_file(dest, pane, window, cx);
                        }
                    });
                }
                Err(err) => {
                    app_ok.update(cx, |app, cx| {
                        app.toast_err(format!("{err}"), window, cx);
                    });
                }
            }
        })
    };
    let enter_sub = subscribe_submit_on_enter(&name_input, submit.clone(), window, cx);

    window.open_dialog(cx, move |dialog, window, cx| {
        let _ = &enter_sub;
        if initial_focus.replace(false) {
            name_input.focus_handle(cx).focus(window, cx);
        }

        let submit_btn = submit.clone();

        dialog
            .title(template.title())
            .w(px(460.))
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(hex_color(kd::FG_MUTED))
                            .child(format!("Create inside: {}", parent_dir.display())),
                    )
                    .child(Input::new(&name_input)),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("new-item-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("new-item-ok")
                            .primary()
                            .label("Create")
                            .on_click(move |_, window, cx| {
                                submit_btn(window, cx);
                            }),
                    ),
            )
    });
}

pub fn open_delete_confirm_dialog(
    app_entity: Entity<VKarmaApp>,
    target: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("item")
        .to_string();
    let is_dir = target.is_dir();

    window.open_dialog(cx, move |dialog, _, _| {
        let app_ok = app_entity.clone();
        let del_path = target.clone();

        dialog
            .title(if is_dir { "Delete Folder?" } else { "Delete File?" })
            .w(px(440.))
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(hex_color(kd::FG))
                            .child(format!("Permanently delete '{name}'?")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(hex_color(kd::RED))
                            .child(format!("Path: {}", del_path.display())),
                    ),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("del-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("del-confirm")
                            .danger()
                            .label("Delete")
                            .on_click(move |_, window, cx| match delete_entry(&del_path) {
                                Ok(()) => {
                                    window.close_dialog(cx);
                                    app_ok.update(cx, |app, cx| {
                                        app.left_pane.tabs.retain(|t| t.path != del_path);
                                        if app.left_pane.active_tab >= app.left_pane.tabs.len() {
                                            app.left_pane.active_tab =
                                                app.left_pane.tabs.len().saturating_sub(1);
                                        }
                                        if let Some(rp) = &mut app.right_pane {
                                            rp.tabs.retain(|t| t.path != del_path);
                                            if rp.active_tab >= rp.tabs.len() {
                                                rp.active_tab = rp.tabs.len().saturating_sub(1);
                                            }
                                            if rp.tabs.is_empty() {
                                                app.right_pane = None;
                                                app.active_pane = PaneSide::Left;
                                            }
                                        }
                                        app.refresh_vault_index(cx);
                                    });
                                }
                                Err(err) => {
                                    app_ok.update(cx, |app, cx| {
                                        app.toast_err(format!("{err}"), window, cx);
                                    });
                                }
                            }),
                    ),
            )
    });
}

pub fn open_rename_sheet_dialog(
    app_entity: Entity<VKarmaApp>,
    side: PaneSide,
    tab_id: u64,
    old_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let name_input = cx.new(|cx| InputState::new(window, cx).default_value(old_name.clone()));
    let initial_focus = Cell::new(true);
    let submitted = Rc::new(Cell::new(false));

    let submit: Rc<dyn Fn(&mut Window, &mut App)> = {
        let input_ok = name_input.clone();
        let app_ok = app_entity.clone();
        let old = old_name.clone();
        let done = submitted.clone();
        Rc::new(move |window, cx| {
            if done.get() {
                return;
            }
            let new_name = input_ok.read(cx).value().trim().to_string();
            let res = app_ok.update(cx, |app, _| {
                if let Some(tab) = app.find_tab_mut(side, tab_id)
                    && let TabKind::Spreadsheet { doc, .. } = &mut tab.kind
                {
                    let r = doc.rename_sheet(&old, &new_name);
                    if r.is_ok() {
                        tab.dirty = true;
                    }
                    return r;
                }
                Ok(())
            });
            match res {
                Ok(()) => {
                    done.set(true);
                    window.close_dialog(cx);
                    app_ok.update(cx, |_, cx| cx.notify());
                }
                Err(err) => {
                    app_ok.update(cx, |app, cx| {
                        app.toast_err(format!("{err}"), window, cx);
                    });
                }
            }
        })
    };
    let enter_sub = subscribe_submit_on_enter(&name_input, submit.clone(), window, cx);

    window.open_dialog(cx, move |dialog, window, cx| {
        let _ = &enter_sub;
        if initial_focus.replace(false) {
            name_input.focus_handle(cx).focus(window, cx);
        }
        let submit_btn = submit.clone();

        dialog
            .title(format!("Rename Sheet '{old_name}'"))
            .w(px(400.))
            .child(Input::new(&name_input))
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("rensheet-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("rensheet-ok")
                            .primary()
                            .label("Rename")
                            .on_click(move |_, window, cx| {
                                submit_btn(window, cx);
                            }),
                    ),
            )
    });
}

pub fn open_new_workspace_dialog(
    app_entity: Entity<VKarmaApp>,
    default_root: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    let name_input =
        cx.new(|cx| InputState::new(window, cx).placeholder("Workspace name (defaults to folder name)"));
    let selected_root = Rc::new(RefCell::new(default_root));
    let selected_preset = Rc::new(Cell::new(WorkspacePreset::Notes));
    let initial_focus = Cell::new(true);
    let submitted = Rc::new(Cell::new(false));

    let submit: Rc<dyn Fn(&mut Window, &mut App)> = {
        let input_ok = name_input.clone();
        let root_ok = selected_root.clone();
        let preset_ok = selected_preset.clone();
        let app_ok = app_entity.clone();
        let done = submitted.clone();
        Rc::new(move |window, cx| {
            if done.get() {
                return;
            }
            let typed = input_ok.read(cx).value().trim().to_string();
            let root = root_ok.borrow().clone();
            if !root.is_dir() {
                app_ok.update(cx, |app, cx| {
                    app.toast_err(
                        format!("Folder not found: {}", root.display()),
                        window,
                        cx,
                    );
                });
                return;
            }
            let name = if typed.is_empty() {
                root.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Workspace")
                    .to_string()
            } else {
                typed
            };
            done.set(true);
            window.close_dialog(cx);
            app_ok.update(cx, |app, cx| {
                app.add_workspace(name, root, preset_ok.get(), window, cx);
            });
        })
    };
    let enter_sub = subscribe_submit_on_enter(&name_input, submit.clone(), window, cx);

    window.open_dialog(cx, move |dialog, window, cx| {
        let _ = &enter_sub;
        if initial_focus.replace(false) {
            name_input.focus_handle(cx).focus(window, cx);
        }

        let current_root = selected_root.borrow().clone();
        let current_preset = selected_preset.get();

        let root_pick = selected_root.clone();
        let app_pick = app_entity.clone();
        let submit_btn = submit.clone();

        let preset_pills = WorkspacePreset::all().iter().copied().map(|preset| {
            let is_sel = preset == current_preset;
            let preset_state = selected_preset.clone();
            let app_refresh = app_entity.clone();
            Button::new(SharedString::from(format!("new-ws-preset-{}", preset.label())))
                .with_variant(if is_sel {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .xsmall()
                .label(preset.label())
                .on_click(move |_, window, cx| {
                    preset_state.set(preset);
                    window.refresh();
                    cx.notify(app_refresh.entity_id());
                })
        });

        dialog
            .title("New Workspace")
            .w(px(500.))
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child("Name"),
                            )
                            .child(Input::new(&name_input)),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child("Root Folder"),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .px_2p5()
                                            .py_1p5()
                                            .rounded(px(6.))
                                            .bg(hex_color(kd::BG_PANEL))
                                            .border_1()
                                            .border_color(hex_color(kd::BORDER_SUBTLE))
                                            .text_xs()
                                            .text_color(hex_color(kd::FG))
                                            .truncate()
                                            .child(current_root.display().to_string()),
                                    )
                                    .child(
                                        Button::new("new-ws-pick-folder")
                                            .small()
                                            .outline()
                                            .icon(IconName::FolderOpen)
                                            .label("Choose Folder…")
                                            .on_click(move |_, window, cx| {
                                                let rx = cx.prompt_for_paths(PathPromptOptions {
                                                    files: false,
                                                    directories: true,
                                                    multiple: false,
                                                    prompt: Some(
                                                        "Select Workspace Root Folder".into(),
                                                    ),
                                                });
                                                let root_state = root_pick.clone();
                                                let view_id = app_pick.entity_id();
                                                window
                                                    .spawn(cx, async move |cx| {
                                                        if let Ok(Ok(Some(paths))) = rx.await
                                                            && let Some(dir) =
                                                                paths.into_iter().next()
                                                        {
                                                            *root_state.borrow_mut() = dir;
                                                            let _ = cx.update(|window, cx| {
                                                                window.refresh();
                                                                cx.notify(view_id);
                                                            });
                                                        }
                                                    })
                                                    .detach();
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child("Layout Preset"),
                            )
                            .child(h_flex().gap_1().flex_wrap().children(preset_pills))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child(current_preset.description()),
                            ),
                    ),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("new-ws-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("new-ws-create")
                            .primary()
                            .label("Create Workspace")
                            .on_click(move |_, window, cx| {
                                submit_btn(window, cx);
                            }),
                    ),
            )
    });
}

pub fn open_rename_workspace_dialog(
    app_entity: Entity<VKarmaApp>,
    ws_idx: usize,
    current_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let name_input = cx.new(|cx| InputState::new(window, cx).default_value(current_name.clone()));
    let initial_focus = Cell::new(true);
    let submitted = Rc::new(Cell::new(false));

    let submit: Rc<dyn Fn(&mut Window, &mut App)> = {
        let input_ok = name_input.clone();
        let app_ok = app_entity.clone();
        let done = submitted.clone();
        Rc::new(move |window, cx| {
            if done.get() {
                return;
            }
            let new_name = input_ok.read(cx).value().trim().to_string();
            if new_name.is_empty() {
                return;
            }
            done.set(true);
            window.close_dialog(cx);
            app_ok.update(cx, |app, cx| {
                app.rename_workspace(ws_idx, new_name, window, cx);
            });
        })
    };
    let enter_sub = subscribe_submit_on_enter(&name_input, submit.clone(), window, cx);

    window.open_dialog(cx, move |dialog, window, cx| {
        let _ = &enter_sub;
        if initial_focus.replace(false) {
            name_input.focus_handle(cx).focus(window, cx);
        }
        let submit_btn = submit.clone();

        dialog
            .title(format!("Rename Workspace '{current_name}'"))
            .w(px(420.))
            .child(Input::new(&name_input))
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("ren-ws-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("ren-ws-ok")
                            .primary()
                            .label("Rename")
                            .on_click(move |_, window, cx| {
                                submit_btn(window, cx);
                            }),
                    ),
            )
    });
}
