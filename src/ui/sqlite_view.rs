use super::{TabKind, VKarmaApp};
use crate::formats::sqlite::{
    SqliteColumnInfo, SqliteMetadata, SqliteQueryResult, export_result_csv, load_metadata,
    load_table_page, run_sqlite_json,
};
use crate::theme::{accent_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px, relative,
};
use std::path::PathBuf;

const PAGE_SIZE: usize = 100;

pub struct SqliteView {
    pub db_path: PathBuf,
    pub meta: SqliteMetadata,
    selected_table: Option<String>,
    columns_info: Vec<SqliteColumnInfo>,
    pub result: Option<SqliteQueryResult>,
    page: usize,
    show_schema: bool,
    allow_write: bool,
    query_input: Entity<InputState>,
    _query_sub: Subscription,
    last_error: Option<String>,
    selected_cell: Option<String>,
    focus_handle: FocusHandle,
}

impl Focusable for SqliteView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SqliteView {
    pub fn new(db_path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (meta, init_err) = match load_metadata(&db_path) {
            Ok(m) => (m, None),
            Err(e) => (
                SqliteMetadata { tables: Vec::new() },
                Some(e),
            ),
        };
        let first_table = meta.tables.first().map(|t| t.name.clone());
        let query_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("SELECT * FROM … (Enter to run)")
        });
        let query_sub = cx.subscribe_in(&query_input, window, |this, _, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.run_custom_query(cx);
            }
        });

        let mut view = Self {
            db_path,
            meta,
            selected_table: None,
            columns_info: Vec::new(),
            result: None,
            page: 0,
            show_schema: false,
            allow_write: false,
            query_input,
            _query_sub: query_sub,
            last_error: init_err,
            selected_cell: None,
            focus_handle: cx.focus_handle(),
        };
        if let Some(tbl) = first_table {
            view.select_table(tbl, window, cx);
        }
        view
    }

    pub fn relocate_db(&mut self, new_path: PathBuf) {
        self.db_path = new_path;
    }

    pub fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match load_metadata(&self.db_path) {
            Ok(meta) => {
                self.meta = meta;
                self.last_error = None;
                let target = self
                    .selected_table
                    .clone()
                    .filter(|n| self.meta.tables.iter().any(|t| &t.name == n))
                    .or_else(|| self.meta.tables.first().map(|t| t.name.clone()));
                if let Some(tbl) = target {
                    self.select_table(tbl, window, cx);
                } else {
                    self.selected_table = None;
                    self.columns_info.clear();
                    self.result = None;
                }
            }
            Err(e) => self.last_error = Some(e),
        }
        cx.notify();
    }

    pub fn select_table(&mut self, table: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_table = Some(table.clone());
        self.page = 0;
        let sql_preview = format!("SELECT * FROM \"{}\" LIMIT {PAGE_SIZE};", table.replace('"', "\"\""));
        self.query_input
            .update(cx, |s, cx| s.set_value(sql_preview, window, cx));
        self.load_current_page(cx);
    }

    fn load_current_page(&mut self, cx: &mut Context<Self>) {
        let Some(ref table) = self.selected_table else {
            return;
        };
        let offset = self.page * PAGE_SIZE;
        match load_table_page(&self.db_path, table, PAGE_SIZE, offset) {
            Ok((cols, res)) => {
                self.columns_info = cols;
                self.result = Some(res);
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(e);
            }
        }
        cx.notify();
    }

    pub fn run_custom_query(&mut self, cx: &mut Context<Self>) {
        let sql = self.query_input.read(cx).value().trim().to_string();
        if sql.is_empty() {
            return;
        }
        match run_sqlite_json(&self.db_path, &sql, !self.allow_write) {
            Ok(res) => {
                self.result = Some(res);
                self.last_error = None;
                self.show_schema = false;
                if self.allow_write
                    && let Ok(meta) = load_metadata(&self.db_path)
                {
                    self.meta = meta;
                }
            }
            Err(e) => self.last_error = Some(e),
        }
        cx.notify();
    }

    pub fn export_csv(&self) -> anyhow::Result<PathBuf> {
        let Some(ref res) = self.result else {
            anyhow::bail!("No query rows available to export");
        };
        let stem = self
            .selected_table
            .as_deref()
            .unwrap_or("query");
        let parent = self
            .db_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let out_path = parent.join(format!("{stem}-export.csv"));
        export_result_csv(res, &out_path)?;
        Ok(out_path)
    }

    pub fn summary(&self) -> String {
        let rows = self.result.as_ref().map_or(0, |r| r.rows.len());
        format!("{} tables · {} rows", self.meta.tables.len(), rows)
    }
}

impl Render for SqliteView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_tbl = self.selected_table.clone();

        let sidebar_items = self.meta.tables.iter().enumerate().map(|(ix, tbl)| {
            let name = tbl.name.clone();
            let name_click = name.clone();
            let is_sel = selected_tbl.as_deref() == Some(name.as_str());
            let is_view = tbl.kind == "view";

            h_flex()
                .id(SharedString::from(format!("db-tbl-{ix}")))
                .w_full()
                .px_2p5()
                .py_1p5()
                .rounded(px(5.))
                .justify_between()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .bg(if is_sel {
                    accent_alpha(0x26)
                } else {
                    gpui_kit::transparent_black()
                })
                .hover(|s| s.bg(hex_color(kd::BG_PANEL)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_table(name_click.clone(), window, cx);
                }))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1p5()
                        .items_center()
                        .child(
                            div()
                                .px_1()
                                .rounded(px(3.))
                                .bg(hex_color(kd::BG_DEEP))
                                .text_size(px(9.5))
                                .font_weight(FontWeight::BOLD)
                                .text_color(if is_view {
                                    hex_color(kd::VIOLET)
                                } else {
                                    hex_color(kd::ACCENT)
                                })
                                .child(if is_view { "V" } else { "T" }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(if is_sel {
                                    hex_color(kd::FG)
                                } else {
                                    hex_color(kd::FG_SUBTLE)
                                })
                                .truncate()
                                .child(name),
                        ),
                )
                .when_some(tbl.row_count, |r, cnt| {
                    r.child(
                        div()
                            .text_size(px(10.))
                            .text_color(hex_color(kd::FG_MUTED))
                            .child(format!("{cnt}")),
                    )
                })
        });

        let create_sql = self
            .selected_table
            .as_deref()
            .and_then(|n| self.meta.tables.iter().find(|t| t.name == n))
            .map(|t| t.sql.clone())
            .unwrap_or_default();

        let main_body: gpui_kit::AnyElement = if let Some(ref err) = self.last_error {
            div()
                .p_6()
                .child(
                    div()
                        .p_4()
                        .rounded(px(8.))
                        .bg(hex_color(kd::BG_SIDEBAR))
                        .border_1()
                        .border_color(hex_color(kd::RED))
                        .text_xs()
                        .text_color(hex_color(kd::RED))
                        .child(err.clone()),
                )
                .into_any_element()
        } else if self.show_schema {
            div()
                .id(SharedString::from(format!("db-schema-{:?}", cx.entity_id())))
                .size_full()
                .overflow_y_scrollbar()
                .p_4()
                .child(
                    v_flex()
                        .gap_4()
                        .child(
                            v_flex()
                                .rounded(px(8.))
                                .border_1()
                                .border_color(hex_color(kd::BORDER_SUBTLE))
                                .overflow_hidden()
                                .child(
                                    h_flex()
                                        .bg(hex_color(kd::BG_SIDEBAR))
                                        .px_3()
                                        .py_2()
                                        .gap_3()
                                        .border_b_1()
                                        .border_color(hex_color(kd::BORDER_SUBTLE))
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(hex_color(kd::ACCENT))
                                        .child(div().w(px(180.)).child("Column"))
                                        .child(div().w(px(120.)).child("Type"))
                                        .child(div().w(px(60.)).child("Key"))
                                        .child(div().w(px(90.)).child("Nullable")),
                                )
                                .children(self.columns_info.iter().enumerate().map(|(ix, c)| {
                                    h_flex()
                                        .px_3()
                                        .py_1p5()
                                        .gap_3()
                                        .bg(if ix % 2 == 1 {
                                            accent_alpha(0x0c)
                                        } else {
                                            gpui_kit::transparent_black()
                                        })
                                        .border_b_1()
                                        .border_color(hex_color(kd::BORDER_SUBTLE))
                                        .text_xs()
                                        .child(
                                            div()
                                                .w(px(180.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(hex_color(kd::FG))
                                                .child(c.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .w(px(120.))
                                                .text_color(hex_color(kd::AQUA))
                                                .child(if c.col_type.is_empty() {
                                                    "ANY".to_string()
                                                } else {
                                                    c.col_type.clone()
                                                }),
                                        )
                                        .child(
                                            div()
                                                .w(px(60.))
                                                .text_color(hex_color(kd::YELLOW))
                                                .child(if c.pk { "PK" } else { "" }),
                                        )
                                        .child(
                                            div()
                                                .w(px(90.))
                                                .text_color(hex_color(kd::FG_MUTED))
                                                .child(if c.not_null { "NOT NULL" } else { "NULL" }),
                                        )
                                })),
                        )
                        .when(!create_sql.is_empty(), |col| {
                            col.child(
                                div()
                                    .p_3()
                                    .rounded(px(8.))
                                    .bg(hex_color(kd::BG_SIDEBAR))
                                    .border_1()
                                    .border_color(hex_color(kd::BORDER_SUBTLE))
                                    .text_xs()
                                    .text_color(hex_color(kd::GREEN))
                                    .child(create_sql),
                            )
                        }),
                )
                .into_any_element()
        } else if let Some(ref res) = self.result {
            if res.columns.is_empty() {
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(hex_color(kd::FG_MUTED))
                    .child(format!("Query executed cleanly ({}ms) — 0 rows returned.", res.duration_ms))
                    .into_any_element()
            } else {
                let col_w = 156.0f32;
                let total_w = 52.0 + (res.columns.len() as f32) * col_w;
                let hdr = h_flex()
                    .bg(hex_color(kd::BG_SIDEBAR))
                    .border_b_1()
                    .border_color(hex_color(kd::BORDER_SUBTLE))
                    .child(
                        div()
                            .w(px(52.))
                            .flex_shrink_0()
                            .px_2()
                            .py_1p5()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(hex_color(kd::FG_MUTED))
                            .child("#"),
                    )
                    .children(res.columns.iter().map(|c| {
                        div()
                            .w(px(col_w))
                            .flex_shrink_0()
                            .px_2()
                            .py_1p5()
                            .border_l_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(hex_color(kd::ACCENT))
                            .truncate()
                            .child(c.clone())
                    }));

                let rows_ui = res.rows.iter().enumerate().map(|(rix, cells)| {
                    let row_num = self.page * PAGE_SIZE + rix + 1;
                    h_flex()
                        .bg(if rix % 2 == 1 {
                            accent_alpha(0x0c)
                        } else {
                            gpui_kit::transparent_black()
                        })
                        .border_b_1()
                        .border_color(hex_color(kd::BORDER_SUBTLE))
                        .child(
                            div()
                                .w(px(52.))
                                .flex_shrink_0()
                                .px_2()
                                .py_1()
                                .text_xs()
                                .text_color(hex_color(kd::FG_MUTED))
                                .child(format!("{row_num}")),
                        )
                        .children(cells.iter().enumerate().map(|(cix, cell)| {
                            let copy_str = cell.clone().unwrap_or_else(|| "NULL".to_string());
                            let is_null = cell.is_none();
                            div()
                                .id(SharedString::from(format!("db-cell-{rix}-{cix}")))
                                .w(px(col_w))
                                .flex_shrink_0()
                                .px_2()
                                .py_1()
                                .border_l_1()
                                .border_color(hex_color(kd::BORDER_SUBTLE))
                                .text_xs()
                                .when(is_null, |d| d.italic().text_color(hex_color(kd::FG_MUTED)))
                                .when(!is_null, |d| d.text_color(hex_color(kd::FG)))
                                .truncate()
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_str.clone(),
                                    ));
                                    this.selected_cell = Some(copy_str.clone());
                                    cx.notify();
                                }))
                                .child(cell.clone().unwrap_or_else(|| "NULL".to_string()))
                        }))
                });

                div()
                    .id(SharedString::from(format!("db-tbl-x-{:?}", cx.entity_id())))
                    .size_full()
                    .overflow_x_scrollbar()
                    .child(
                        v_flex()
                            .w(px(total_w))
                            .min_w(relative(1.))
                            .h_full()
                            .min_h_0()
                            .child(hdr)
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "db-tbl-y-{:?}",
                                        cx.entity_id()
                                    )))
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .overflow_y_scrollbar()
                                    .child(v_flex().children(rows_ui)),
                            ),
                    )
                    .into_any_element()
            }
        } else {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(hex_color(kd::FG_MUTED))
                .child("Select a table or run a SQL query.")
                .into_any_element()
        };

        let row_cnt = self.result.as_ref().map_or(0, |r| r.rows.len());

        h_flex()
            .id(SharedString::from(format!("sqlite-view-{:?}", cx.entity_id())))
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .track_focus(&self.focus_handle)
            .child(
                v_flex()
                    .w(px(210.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(hex_color(kd::BG_SIDEBAR))
                    .border_r_1()
                    .border_color(hex_color(kd::BORDER_SUBTLE))
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(hex_color(kd::FG_MUTED))
                            .child(format!("Tables & Views ({})", self.meta.tables.len())),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "db-side-scroll-{:?}",
                                cx.entity_id()
                            )))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .p_1p5()
                            .child(v_flex().gap_0p5().children(sidebar_items)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        h_flex()
                            .px_3()
                            .py_2()
                            .gap_1p5()
                            .items_center()
                            .border_b_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .child(div().flex_1().child(Input::new(&self.query_input).xsmall()))
                            .child(
                                Button::new("db-run-btn")
                                    .primary()
                                    .xsmall()
                                    .label("Run")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.run_custom_query(cx);
                                    })),
                            )
                            .child(
                                Button::new("db-ro-btn")
                                    .with_variant(if self.allow_write {
                                        ButtonVariant::Primary
                                    } else {
                                        ButtonVariant::Secondary
                                    })
                                    .xsmall()
                                    .label(if self.allow_write { "RW" } else { "Read-Only" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.allow_write = !this.allow_write;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("db-schema-btn")
                                    .with_variant(if self.show_schema {
                                        ButtonVariant::Primary
                                    } else {
                                        ButtonVariant::Ghost
                                    })
                                    .xsmall()
                                    .label(if self.show_schema { "Data" } else { "Schema" })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_schema = !this.show_schema;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("db-prev-pg")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::ChevronLeft)
                                    .disabled(self.page == 0)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if this.page > 0 {
                                            this.page -= 1;
                                            this.load_current_page(cx);
                                        }
                                    })),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(hex_color(kd::FG_MUTED))
                                    .child(format!("P{}", self.page + 1)),
                            )
                            .child(
                                Button::new("db-next-pg")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::ChevronRight)
                                    .disabled(row_cnt < PAGE_SIZE)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.page += 1;
                                        this.load_current_page(cx);
                                    })),
                            ),
                    )
                    .child(div().flex_1().min_h_0().w_full().child(main_body))
                    .when_some(self.selected_cell.as_ref(), |col, val| {
                        col.child(
                            h_flex()
                                .px_3()
                                .py_1p5()
                                .gap_2()
                                .bg(hex_color(kd::BG_SIDEBAR))
                                .border_t_1()
                                .border_color(hex_color(kd::BORDER_SUBTLE))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(hex_color(kd::ACCENT))
                                        .child("Copied:"),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(hex_color(kd::FG))
                                        .truncate()
                                        .child(val.clone()),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }
}

pub fn render_sqlite_tab(
    app: &VKarmaApp,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Sqlite {
        view,
        file_size_bytes,
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let zen_mode = app.zen_mode;
    let kb = *file_size_bytes as f64 / 1024.0;
    let summary = view.read(cx).summary();

    let v_ref = view.clone();
    let v_exp = view.clone();

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
                        .child(format!("{summary} · {kb:.1} KB")),
                ),
        )
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(
                    Button::new(SharedString::from(format!("db-refresh-{tab_id}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::RefreshCw)
                        .label("Refresh")
                        .on_click(cx.listener(move |_, _, window, cx| {
                            v_ref.update(cx, |v, cx| v.reload(window, cx));
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("db-csv-{tab_id}")))
                        .secondary()
                        .xsmall()
                        .icon(IconName::FileSpreadsheet)
                        .label("Export CSV")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            match v_exp.read(cx).export_csv() {
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
                ),
        );

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .when(!zen_mode, |this| this.child(header))
        .child(div().flex_1().min_h_0().w_full().child(view.clone()))
        .into_any_element()
}
