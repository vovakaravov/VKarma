use super::{PaneSide, TabKind, VKarmaApp, modals};
use crate::spreadsheet::{CellKind, cell_ref, col_label};
use crate::theme::{accent_alpha, hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::scroll::{ScrollableElement as _, Scrollbar};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    ClipboardItem, Context, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, px, uniform_list,
};

const ROW_GUTTER_W: f32 = 52.0;
const CELL_W: f32 = 116.0;
const ROW_H: f32 = 26.0;

pub fn render_spreadsheet_tab(
    _app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    _window: &mut Window,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Spreadsheet {
        doc,
        active_sheet,
        active_row,
        active_col,
        formula_input,
        row_scroll,
        grid_focus,
        ..
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let sheet_idx = *active_sheet;
    let cur_row = *active_row;
    let cur_col = *active_col;
    let (num_rows, num_cols) = doc.grid_dimensions(sheet_idx);
    let sheet_names = doc.sheet_names();
    let active_cell_ref = cell_ref(cur_row, cur_col);
    let dirty = tab.dirty;
    let row_scroll_handle = row_scroll.clone();
    let grid_focus_handle = grid_focus.clone();
    let path_stem = tab
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export")
        .to_string();

    // Snapshot visible grid cleanly for the uniform_list closure
    let mut rows_snapshot: Vec<Vec<(String, CellKind)>> = Vec::with_capacity(num_rows as usize);
    for r in 1..=num_rows {
        let mut row_cells = Vec::with_capacity(num_cols as usize);
        for c in 1..=num_cols {
            let disp = doc.display_value(sheet_idx, r, c);
            let kind = doc.cell_kind(sheet_idx, r, c);
            row_cells.push((disp, kind));
        }
        rows_snapshot.push(row_cells);
    }

    let toolbar = h_flex()
        .h(px(36.))
        .w_full()
        .px_2()
        .gap_1p5()
        .items_center()
        .justify_between()
        .bg(hex_color(kd::BG_SIDEBAR))
        .border_b_1()
        .border_color(hex_color(kd::BORDER_SUBTLE))
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .when(dirty, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("xls-save-{tab_id}")))
                            .primary()
                            .xsmall()
                            .icon(IconName::Save)
                            .label("Save")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.save_tab(side, tab_id, window, cx);
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!("xls-add-row-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Plus)
                        .label("Row")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                && let TabKind::Spreadsheet { doc, active_sheet, active_row, .. } = &mut t.kind
                            {
                                match doc.insert_row(*active_sheet, *active_row) {
                                    Ok(()) => {
                                        t.dirty = true;
                                        cx.notify();
                                    }
                                    Err(e) => this.toast_err(format!("{e}"), window, cx),
                                }
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("xls-del-row-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Minus)
                        .label("Row")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                && let TabKind::Spreadsheet { doc, active_sheet, active_row, .. } = &mut t.kind
                            {
                                match doc.delete_row(*active_sheet, *active_row) {
                                    Ok(()) => {
                                        t.dirty = true;
                                        cx.notify();
                                    }
                                    Err(e) => this.toast_err(format!("{e}"), window, cx),
                                }
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("xls-add-col-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Plus)
                        .label("Col")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                && let TabKind::Spreadsheet { doc, active_sheet, active_col, .. } = &mut t.kind
                            {
                                match doc.insert_col(*active_sheet, *active_col) {
                                    Ok(()) => {
                                        t.dirty = true;
                                        cx.notify();
                                    }
                                    Err(e) => this.toast_err(format!("{e}"), window, cx),
                                }
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("xls-del-col-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Minus)
                        .label("Col")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(t) = this.find_tab_mut(side, tab_id)
                                && let TabKind::Spreadsheet { doc, active_sheet, active_col, .. } = &mut t.kind
                            {
                                match doc.delete_col(*active_sheet, *active_col) {
                                    Ok(()) => {
                                        t.dirty = true;
                                        cx.notify();
                                    }
                                    Err(e) => this.toast_err(format!("{e}"), window, cx),
                                }
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("xls-sum-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .label("=SUM()")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            insert_quick_formula(this, side, tab_id, "SUM", window, cx);
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("xls-avg-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .label("=AVG()")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            insert_quick_formula(this, side, tab_id, "AVERAGE", window, cx);
                        })),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new(SharedString::from(format!("xls-exp-xlsx-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .label("Export .xlsx")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let Some(tab) = this.find_tab(side, tab_id) else {
                                return;
                            };
                            let out_path = tab.path.with_extension("xlsx");
                            let res = if let TabKind::Spreadsheet { doc, .. } = &tab.kind {
                                doc.export_xlsx(&out_path)
                            } else {
                                Ok(())
                            };
                            match res {
                                Ok(()) => {
                                    this.refresh_vault_index(cx);
                                    this.toast_ok(
                                        format!("Exported {}", out_path.display()),
                                        window,
                                        cx,
                                    );
                                }
                                Err(e) => this.toast_err(format!("Export failed: {e}"), window, cx),
                            }
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("xls-exp-csv-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .label("Export .csv")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let Some(tab) = this.find_tab(side, tab_id) else {
                                return;
                            };
                            let out_path = tab
                                .path
                                .with_file_name(format!("{path_stem}-sheet{}.csv", sheet_idx + 1));
                            let res = if let TabKind::Spreadsheet { doc, active_sheet, .. } = &tab.kind
                            {
                                doc.export_csv(*active_sheet, &out_path)
                            } else {
                                Ok(())
                            };
                            match res {
                                Ok(()) => {
                                    this.refresh_vault_index(cx);
                                    this.toast_ok(
                                        format!("Exported {}", out_path.display()),
                                        window,
                                        cx,
                                    );
                                }
                                Err(e) => this.toast_err(format!("Export failed: {e}"), window, cx),
                            }
                        })),
                ),
        );

    let formula_bar = h_flex()
        .h(px(34.))
        .w_full()
        .px_2()
        .gap_2()
        .items_center()
        .bg(hex_color(kd::BG_MAIN))
        .border_b_1()
        .border_color(hex_color(kd::BORDER_SUBTLE))
        .child(
            div()
                .w(px(58.))
                .h(px(24.))
                .rounded(px(4.))
                .bg(hex_color(kd::BG_PANEL))
                .border_1()
                .border_color(hex_color(kd::BORDER))
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .font_weight(gpui_kit::FontWeight::BOLD)
                .text_color(hex_color(kd::ACCENT))
                .child(active_cell_ref),
        )
        .child(
            div()
                .text_xs()
                .italic()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(hex_color(kd::YELLOW))
                .child("fx"),
        )
        .child(
            div()
                .id(SharedString::from(format!("xls-fx-wrap-{tab_id}")))
                .flex_1()
                .min_w_0()
                .capture_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key.as_str() == "escape"
                        && let Some(t) = this.find_tab(side, tab_id)
                        && let TabKind::Spreadsheet {
                            doc,
                            active_sheet,
                            active_row,
                            active_col,
                            formula_input,
                            grid_focus,
                            ..
                        } = &t.kind
                    {
                        let orig = doc.raw_input(*active_sheet, *active_row, *active_col);
                        let gf = grid_focus.clone();
                        formula_input
                            .clone()
                            .update(cx, |s, cx| s.set_value(orig, window, cx));
                        gf.focus(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(Input::new(formula_input).small()),
        )
        .child(
            Button::new(SharedString::from(format!("xls-apply-fx-{tab_id}")))
                .secondary()
                .xsmall()
                .icon(IconName::Check)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.commit_formula_bar(side, tab_id, false, window, cx);
                })),
        );

    let total_width = ROW_GUTTER_W + (num_cols as f32) * CELL_W;
    let col_headers = h_flex()
        .h(px(ROW_H))
        .w(px(total_width))
        .bg(hex_color(kd::BG_PANEL))
        .border_b_1()
        .border_color(hex_color(kd::BORDER))
        .child(
            div()
                .w(px(ROW_GUTTER_W))
                .h_full()
                .flex_shrink_0()
                .border_r_1()
                .border_color(hex_color(kd::BORDER)),
        )
        .children((1..=num_cols).map(|c| {
            let is_col_active = c == cur_col;
            div()
                .w(px(CELL_W))
                .h_full()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .border_r_1()
                .border_color(hex_color(kd::BORDER_SUBTLE))
                .when(is_col_active, |d| d.bg(accent_alpha(0x26)))
                .text_xs()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(if is_col_active {
                    hex_color(kd::ACCENT)
                } else {
                    hex_color(kd::YELLOW)
                })
                .child(col_label(c))
        }));

    let app_entity = cx.entity();
    let grid_rows = uniform_list(
        SharedString::from(format!("sheet-grid-{tab_id}")),
        num_rows as usize,
        move |range, _, _| {
            range
                .map(|row_zero_idx| {
                    let row_1 = (row_zero_idx + 1) as i32;
                    let is_row_active = row_1 == cur_row;
                    let row_cells = &rows_snapshot[row_zero_idx];

                    h_flex()
                        .h(px(ROW_H))
                        .w(px(total_width))
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .child(
                            div()
                                .w(px(ROW_GUTTER_W))
                                .h_full()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(if is_row_active {
                                    accent_alpha(0x26)
                                } else {
                                    hex_color(kd::BG_SIDEBAR)
                                })
                                .border_r_1()
                                .border_color(hex_color(kd::BORDER))
                                .text_xs()
                                .text_color(if is_row_active {
                                    hex_color(kd::ACCENT)
                                } else {
                                    hex_color(kd::FG_MUTED)
                                })
                                .child(format!("{row_1}")),
                        )
                        .children(row_cells.iter().enumerate().map(|(c_zero, (val, kind))| {
                            let col_1 = (c_zero + 1) as i32;
                            let is_active_cell = is_row_active && col_1 == cur_col;
                            let app_click = app_entity.clone();
                            let disp = val.clone();

                            let text_color = match kind {
                                CellKind::Error => hex_color(kd::RED),
                                CellKind::Number => hex_color(kd::ORANGE),
                                CellKind::Formula => hex_color(kd::AQUA),
                                CellKind::Bool => hex_color(kd::VIOLET),
                                _ => hex_color(kd::FG),
                            };
                            let align_right = matches!(kind, CellKind::Number);

                            div()
                                .id(("cell", row_zero_idx * 1024 + c_zero))
                                .w(px(CELL_W))
                                .h_full()
                                .flex_shrink_0()
                                .px_1p5()
                                .flex()
                                .items_center()
                                .when(align_right, |d| d.justify_end())
                                .border_r_1()
                                .border_color(if is_active_cell {
                                    hex_color(kd::ACCENT)
                                } else {
                                    hex_color(kd::BORDER_SUBTLE)
                                })
                                .when(is_active_cell, |d| {
                                    d.bg(accent_alpha(0x24))
                                        .border_2()
                                        .border_color(hex_color(kd::ACCENT))
                                })
                                .when(!is_active_cell, |d| {
                                    d.hover(|s| s.bg(hex_alpha(0x28262b99)))
                                })
                                .cursor_pointer()
                                .on_mouse_down(
                                    gpui_kit::MouseButton::Left,
                                    move |ev, window, cx| {
                                        let dbl = ev.click_count >= 2;
                                        app_click.update(cx, |app, cx| {
                                            app.active_pane = side;
                                            app.select_spreadsheet_cell(
                                                side, tab_id, row_1, col_1, dbl, window, cx,
                                            );
                                        });
                                    },
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(text_color)
                                        .truncate()
                                        .child(disp),
                                )
                        }))
                        .into_any_element()
                })
                .collect()
        },
    )
    .size_full()
    .track_scroll(&row_scroll_handle);

    let grid_container = div()
        .id(SharedString::from(format!("xls-grid-focus-{tab_id}")))
        .size_full()
        .track_focus(&grid_focus_handle)
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            handle_grid_key(this, side, tab_id, event, window, cx);
        }))
        .overflow_x_scrollbar()
        .child(
            v_flex()
                .w(px(total_width))
                .h_full()
                .min_h_0()
                .child(col_headers)
                .child(div().flex_1().min_h_0().w_full().child(grid_rows)),
        );

    let grid_shell = div()
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .child(grid_container)
        .child(Scrollbar::vertical(&row_scroll_handle));

    let app_for_sheet_menu = cx.entity();
    let sheet_bar = h_flex()
        .h(px(30.))
        .w_full()
        .px_2()
        .gap_1()
        .items_center()
        .bg(hex_color(kd::BG_SIDEBAR))
        .border_t_1()
        .border_color(hex_color(kd::BORDER_SUBTLE))
        .children(sheet_names.into_iter().enumerate().map(|(ix, name)| {
            let s_idx = ix as u32;
            let is_cur = s_idx == sheet_idx;
            let app_ren = app_for_sheet_menu.clone();
            let app_del = app_for_sheet_menu.clone();
            let name_ren = name.clone();

            Button::new(SharedString::from(format!("sheet-tab-{tab_id}-{ix}")))
                .with_variant(if is_cur {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .xsmall()
                .label(name)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(t) = this.find_tab_mut(side, tab_id)
                        && let TabKind::Spreadsheet {
                            doc,
                            active_sheet,
                            active_row,
                            active_col,
                            formula_input,
                            ..
                        } = &mut t.kind
                    {
                        *active_sheet = s_idx;
                        *active_row = 1;
                        *active_col = 1;
                        let raw = doc.raw_input(s_idx, 1, 1);
                        formula_input
                            .clone()
                            .update(cx, |s, cx| s.set_value(raw, window, cx));
                        cx.notify();
                    }
                }))
                .context_menu(move |menu, _, _| {
                    let app_r = app_ren.clone();
                    let app_d = app_del.clone();
                    let n_r = name_ren.clone();
                    menu.item(
                        PopupMenuItem::new("Rename Sheet…")
                            .icon(IconName::Pencil)
                            .on_click(move |_, window, cx| {
                                modals::open_rename_sheet_dialog(
                                    app_r.clone(),
                                    side,
                                    tab_id,
                                    n_r.clone(),
                                    window,
                                    cx,
                                );
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Delete Sheet")
                            .icon(IconName::Trash)
                            .on_click(move |_, window, cx| {
                                app_d.update(cx, |app, cx| {
                                    if let Some(t) = app.find_tab_mut(side, tab_id)
                                        && let TabKind::Spreadsheet {
                                            doc,
                                            active_sheet,
                                            formula_input,
                                            ..
                                        } = &mut t.kind
                                    {
                                        match doc.delete_sheet(s_idx) {
                                            Ok(()) => {
                                                *active_sheet = (*active_sheet).min(
                                                    doc.sheet_names().len().saturating_sub(1) as u32,
                                                );
                                                t.dirty = true;
                                                let raw = doc.raw_input(*active_sheet, 1, 1);
                                                formula_input.clone().update(cx, |s, cx| {
                                                    s.set_value(raw, window, cx);
                                                });
                                                cx.notify();
                                            }
                                            Err(e) => app.toast_err(format!("{e}"), window, cx),
                                        }
                                    }
                                });
                            }),
                    )
                })
        }))
        .child(
            Button::new(SharedString::from(format!("sheet-add-{tab_id}")))
                .ghost()
                .xsmall()
                .icon(IconName::Plus)
                .tooltip("Add Sheet")
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(t) = this.find_tab_mut(side, tab_id)
                        && let TabKind::Spreadsheet { doc, active_sheet, .. } = &mut t.kind
                    {
                        match doc.add_sheet(None) {
                            Ok(_) => {
                                *active_sheet = doc.sheet_names().len().saturating_sub(1) as u32;
                                t.dirty = true;
                                cx.notify();
                            }
                            Err(e) => this.toast_err(format!("{e}"), window, cx),
                        }
                    }
                })),
        );

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .child(toolbar)
        .child(formula_bar)
        .child(grid_shell)
        .child(sheet_bar)
        .into_any_element()
}

fn insert_quick_formula(
    app: &mut VKarmaApp,
    side: PaneSide,
    tab_id: u64,
    func: &str,
    window: &mut Window,
    cx: &mut Context<VKarmaApp>,
) {
    let Some(t) = app.find_tab_mut(side, tab_id) else {
        return;
    };
    if let TabKind::Spreadsheet {
        active_row,
        active_col,
        formula_input,
        ..
    } = &t.kind
    {
        let start_ref = cell_ref(1, *active_col);
        let end_ref = cell_ref((*active_row - 1).max(1), *active_col);
        let formula = format!("={func}({start_ref}:{end_ref})");
        formula_input.clone().update(cx, |s, cx| {
            s.set_value(formula, window, cx);
            s.focus(window, cx);
        });
    }
}

fn handle_grid_key(
    app: &mut VKarmaApp,
    side: PaneSide,
    tab_id: u64,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<VKarmaApp>,
) {
    let key = event.keystroke.key.as_str();
    let ctrl = event.keystroke.modifiers.control;
    let shift = event.keystroke.modifiers.shift;

    if ctrl && key == "c" {
        if let Some(t) = app.find_tab(side, tab_id)
            && let TabKind::Spreadsheet {
                doc,
                active_sheet,
                active_row,
                active_col,
                ..
            } = &t.kind
        {
            let val = doc.display_value(*active_sheet, *active_row, *active_col);
            cx.write_to_clipboard(ClipboardItem::new_string(val));
            cx.stop_propagation();
        }
        return;
    }

    if ctrl && key == "v" {
        if let Some(item) = cx.read_from_clipboard()
            && let Some(text) = item.text()
            && let Some(t) = app.find_tab_mut(side, tab_id)
            && let TabKind::Spreadsheet {
                doc,
                active_sheet,
                active_row,
                active_col,
                formula_input,
                ..
            } = &mut t.kind
        {
            if let Ok(n) = doc.paste_tsv(*active_sheet, *active_row, *active_col, &text)
                && n > 0
            {
                t.dirty = true;
                let raw = doc.raw_input(*active_sheet, *active_row, *active_col);
                formula_input
                    .clone()
                    .update(cx, |s, cx| s.set_value(raw, window, cx));
                cx.notify();
            }
            cx.stop_propagation();
        }
        return;
    }

    let Some(t) = app.find_tab(side, tab_id) else {
        return;
    };
    let TabKind::Spreadsheet {
        active_row,
        active_col,
        active_sheet,
        ..
    } = &t.kind
    else {
        return;
    };
    let cur_r = *active_row;
    let cur_c = *active_col;
    let sheet = *active_sheet;

    match key {
        "up" => {
            app.select_spreadsheet_cell(side, tab_id, (cur_r - 1).max(1), cur_c, false, window, cx);
            cx.stop_propagation();
        }
        "down" => {
            app.select_spreadsheet_cell(side, tab_id, cur_r + 1, cur_c, false, window, cx);
            cx.stop_propagation();
        }
        "left" => {
            app.select_spreadsheet_cell(side, tab_id, cur_r, (cur_c - 1).max(1), false, window, cx);
            cx.stop_propagation();
        }
        "right" => {
            app.select_spreadsheet_cell(side, tab_id, cur_r, cur_c + 1, false, window, cx);
            cx.stop_propagation();
        }
        "tab" => {
            let next_c = if shift { (cur_c - 1).max(1) } else { cur_c + 1 };
            app.select_spreadsheet_cell(side, tab_id, cur_r, next_c, false, window, cx);
            cx.stop_propagation();
        }
        "enter" | "f2" => {
            app.select_spreadsheet_cell(side, tab_id, cur_r, cur_c, true, window, cx);
            cx.stop_propagation();
        }
        "backspace" | "delete" => {
            if let Some(t) = app.find_tab_mut(side, tab_id)
                && let TabKind::Spreadsheet {
                    doc,
                    formula_input,
                    ..
                } = &mut t.kind
            {
                let _ = doc.set_cell(sheet, cur_r, cur_c, "");
                t.dirty = true;
                formula_input
                    .clone()
                    .update(cx, |s, cx| s.set_value("", window, cx));
                cx.notify();
            }
            cx.stop_propagation();
        }
        _ => {
            // Direct typing in a selected cell starts editing immediately in the formula bar
            if !ctrl
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform
                && let Some(ch) = event.keystroke.key_char.as_deref()
                && !ch.is_empty()
                && !ch.chars().any(char::is_control)
                && let Some(t) = app.find_tab(side, tab_id)
                && let TabKind::Spreadsheet { formula_input, .. } = &t.kind
            {
                let seed = ch.to_string();
                formula_input.clone().update(cx, |s, cx| {
                    s.set_value("", window, cx);
                    s.insert(seed, window, cx);
                    s.focus(window, cx);
                });
                cx.stop_propagation();
            }
        }
    }
}
