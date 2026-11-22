use super::{TabKind, VKarmaApp};
use crate::formats::archive::{
    ArchiveIndex, ArchivePreview, extract_all_safe, format_bytes, read_entry_preview,
};
use crate::theme::{hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, ObjectFit, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, StyledImage as _, Subscription, Window, div, img,
    prelude::FluentBuilder as _, px,
};
use std::path::PathBuf;
use std::sync::Arc;

pub struct ArchiveView {
    pub archive_path: PathBuf,
    pub index: ArchiveIndex,
    filter_input: Entity<InputState>,
    _filter_sub: Subscription,
    selected_entry: Option<String>,
    preview: Option<ArchivePreview>,
    preview_error: Option<String>,
    focus_handle: FocusHandle,
}

impl Focusable for ArchiveView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ArchiveView {
    pub fn new(archive_path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (index, load_err) = match ArchiveIndex::load(&archive_path) {
            Ok(idx) => (idx, None),
            Err(err) => (
                ArchiveIndex {
                    format: crate::formats::archive::ArchiveFormat::from_path(&archive_path),
                    entries: Vec::new(),
                    total_uncompressed: 0,
                },
                Some(format!("{err}")),
            ),
        };
        let first_file = index
            .entries
            .iter()
            .find(|e| !e.is_dir)
            .map(|e| e.path.clone());

        let filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter archive entries…"));
        let filter_sub = cx.subscribe(&filter_input, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });

        let mut view = Self {
            archive_path,
            index,
            filter_input,
            _filter_sub: filter_sub,
            selected_entry: None,
            preview: None,
            preview_error: load_err,
            focus_handle: cx.focus_handle(),
        };
        if let Some(first) = first_file {
            view.select_entry(first, cx);
        }
        view
    }

    pub fn relocate_archive(&mut self, new_path: PathBuf) {
        self.archive_path = new_path;
    }

    pub fn select_entry(&mut self, entry_path: String, cx: &mut Context<Self>) {
        self.selected_entry = Some(entry_path.clone());
        match read_entry_preview(&self.archive_path, &entry_path, 512 * 1024) {
            Ok(prev) => {
                self.preview = Some(prev);
                self.preview_error = None;
            }
            Err(err) => {
                self.preview = None;
                self.preview_error = Some(format!("{err}"));
            }
        }
        cx.notify();
    }

    pub fn extract_here(&self) -> anyhow::Result<(PathBuf, usize)> {
        let stem = self
            .archive_path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| {
                n.trim_end_matches(".tar.gz")
                    .trim_end_matches(".tgz")
                    .trim_end_matches(".tar")
                    .trim_end_matches(".zip")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or("archive");
        let parent = self
            .archive_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let dest = parent.join(format!("{stem}-extracted"));
        let count = extract_all_safe(&self.archive_path, &dest)?;
        Ok((dest, count))
    }

    pub fn summary(&self) -> String {
        let files = self.index.entries.iter().filter(|e| !e.is_dir).count();
        format!(
            "{} · {} files · {}",
            self.index.format.label(),
            files,
            format_bytes(self.index.total_uncompressed)
        )
    }
}

impl Render for ArchiveView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let q = self.filter_input.read(cx).value().trim().to_ascii_lowercase();
        let selected = self.selected_entry.clone();

        let items = self
            .index
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| q.is_empty() || e.path.to_ascii_lowercase().contains(&q))
            .take(1500)
            .map(|(ix, entry)| {
                let path = entry.path.clone();
                let path_click = path.clone();
                let is_sel = selected.as_deref() == Some(path.as_str());
                let is_dir = entry.is_dir;
                let sz = entry.compressed_size.map_or_else(
                    || format_bytes(entry.size),
                    |c| format!("{} ({})", format_bytes(entry.size), format_bytes(c)),
                );

                h_flex()
                    .id(SharedString::from(format!("arc-ent-{ix}")))
                    .w_full()
                    .px_2p5()
                    .py_1()
                    .rounded(px(5.))
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .bg(if is_sel {
                        hex_alpha(0x8ba4b026)
                    } else {
                        gpui_kit::transparent_black()
                    })
                    .hover(|s| s.bg(hex_color(kd::BG_PANEL)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_entry(path_click.clone(), cx);
                    }))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1p5()
                            .items_center()
                            .child(
                                Icon::new(if is_dir {
                                    IconName::Folder
                                } else {
                                    IconName::FileText
                                })
                                .xsmall()
                                .text_color(if is_dir {
                                    hex_color(kd::AQUA)
                                } else {
                                    hex_color(kd::FG_MUTED)
                                }),
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
                                    .child(path),
                            ),
                    )
                    .when(!is_dir, |row| {
                        row.child(
                            div()
                                .text_size(px(10.))
                                .text_color(hex_color(kd::FG_MUTED))
                                .child(sz),
                        )
                    })
            });

        let preview_pane: gpui_kit::AnyElement = if let Some(ref err) = self.preview_error {
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
        } else if let Some(ref prev) = self.preview {
            match prev {
                ArchivePreview::Directory => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(hex_color(kd::FG_MUTED))
                    .child("Directory entry — select a file to preview.")
                    .into_any_element(),
                ArchivePreview::Text(txt) => div()
                    .id(SharedString::from(format!("arc-txt-{:?}", cx.entity_id())))
                    .size_full()
                    .overflow_y_scrollbar()
                    .p_4()
                    .child(
                        div()
                            .font_family("monospace")
                            .text_xs()
                            .text_color(hex_color(kd::FG))
                            .child(txt.clone()),
                    )
                    .into_any_element(),
                ArchivePreview::Image(image) => div()
                    .id(SharedString::from(format!("arc-img-{:?}", cx.entity_id())))
                    .size_full()
                    .overflow_y_scrollbar()
                    .p_6()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(Arc::clone(image))
                            .max_w(px(900.))
                            .max_h(px(700.))
                            .object_fit(ObjectFit::Contain)
                            .rounded(px(6.)),
                    )
                    .into_any_element(),
                ArchivePreview::Binary {
                    hex_dump,
                    total_bytes,
                } => v_flex()
                    .size_full()
                    .p_4()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(hex_color(kd::YELLOW))
                            .child(format!("Binary file ({} previewed)", format_bytes(*total_bytes as u64))),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("arc-hex-{:?}", cx.entity_id())))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .p_3()
                            .rounded(px(6.))
                            .bg(hex_color(kd::BG_SIDEBAR))
                            .font_family("monospace")
                            .text_xs()
                            .text_color(hex_color(kd::FG_SUBTLE))
                            .child(hex_dump.clone()),
                    )
                    .into_any_element(),
            }
        } else {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(hex_color(kd::FG_MUTED))
                .child("Select any file in the archive to preview.")
                .into_any_element()
        };

        h_flex()
            .id(SharedString::from(format!("arc-view-{:?}", cx.entity_id())))
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .track_focus(&self.focus_handle)
            .child(
                v_flex()
                    .w(px(320.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(hex_color(kd::BG_SIDEBAR))
                    .border_r_1()
                    .border_color(hex_color(kd::BORDER_SUBTLE))
                    .child(
                        div()
                            .p_2()
                            .border_b_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .child(Input::new(&self.filter_input).xsmall()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "arc-list-{:?}",
                                cx.entity_id()
                            )))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .p_1p5()
                            .child(v_flex().gap_0p5().children(items)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when_some(selected, |col, entry_name| {
                        col.child(
                            div()
                                .px_4()
                                .py_2()
                                .border_b_1()
                                .border_color(hex_color(kd::BORDER_SUBTLE))
                                .bg(hex_color(kd::BG_DEEP))
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(hex_color(kd::BLUE))
                                .child(entry_name),
                        )
                    })
                    .child(div().flex_1().min_h_0().w_full().child(preview_pane)),
            )
            .into_any_element()
    }
}

pub fn render_archive_tab(
    app: &VKarmaApp,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Archive {
        view,
        file_size_bytes,
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let rel_path = tab.rel_path.clone();
    let zen_mode = app.zen_mode;
    let summary = view.read(cx).summary();
    let arc_sz = format_bytes(*file_size_bytes);
    let v_ext = view.clone();

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
                        .text_color(hex_color(kd::YELLOW))
                        .child(format!("{summary} · compressed {arc_sz}")),
                ),
        )
        .child(
            h_flex().gap_1p5().items_center().child(
                Button::new(SharedString::from(format!("arc-ext-{tab_id}")))
                    .secondary()
                    .xsmall()
                    .icon(IconName::FolderOpen)
                    .label("Extract All Here")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        match v_ext.read(cx).extract_here() {
                            Ok((dest, count)) => {
                                this.refresh_vault_index(cx);
                                this.toast_ok(
                                    format!("Extracted {count} files to {}", dest.display()),
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
