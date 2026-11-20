use super::{PaneSide, TabKind, VKarmaApp};
use crate::live_preview::{
    LIVE_TAIL_SPACER_HEIGHT, LiveBlock, LiveBlockKind, LiveHeightIndex, line_has_link,
    list_indent_px,
};
use crate::markdown::{preprocess_markdown_with_options, resolve_inline_image_source};
use crate::theme::{hex_color, kd};
use gpui_kit::base::input::{
    Backspace, Delete, Enter, Escape, MoveDown, MoveLeft, MoveRight, MoveUp, Paste,
    Textarea as BaseTextarea,
};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarHandle};
use gpui_kit::component::text::TextView;
use gpui_kit::{
    AnyElement, Bounds, ClickEvent, Context, EntityId, FontWeight, InteractiveElement, IntoElement,
    ListOffset, ListState, MouseButton, MouseDownEvent, ParentElement, Pixels, Point, ScrollEvent,
    SharedString, Size, StatefulInteractiveElement as _, Styled, canvas, div, list, point,
    prelude::FluentBuilder as _, px, relative, size,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

struct LiveScrollbarInner {
    index: LiveHeightIndex,
    blocks: Arc<Vec<LiveBlock>>,
    centered: bool,
    horiz_pad: f32,
    drag_total_height: Option<f32>,
}

/// Custom `ScrollbarHandle` wrapping `ListState` with width-aware height estimation for
/// off-screen blocks so the Live Preview scrollbar thumb is accurately sized from frame 1
/// and drags smoothly all the way to the bottom of any document.
#[derive(Clone)]
pub struct LiveScrollbarHandle {
    list_state: ListState,
    inner: Rc<RefCell<LiveScrollbarInner>>,
}

impl LiveScrollbarHandle {
    pub fn new(list_state: ListState, blocks: Arc<Vec<LiveBlock>>) -> Self {
        let index = LiveHeightIndex::new(&blocks, 760.0);
        Self {
            list_state,
            inner: Rc::new(RefCell::new(LiveScrollbarInner {
                index,
                blocks,
                centered: true,
                horiz_pad: 32.0,
                drag_total_height: None,
            })),
        }
    }

    pub fn sync_render_context(
        &self,
        blocks: &Arc<Vec<LiveBlock>>,
        centered: bool,
        horiz_pad: f32,
    ) {
        let mut inner = self.inner.borrow_mut();
        inner.centered = centered;
        inner.horiz_pad = horiz_pad;
        if !Arc::ptr_eq(&inner.blocks, blocks) {
            inner.blocks = Arc::clone(blocks);
            if inner.index.item_count() != blocks.len() + 1 {
                let w = inner.index.content_width();
                inner.index.rebuild(blocks, w);
            }
        }
    }

    pub fn splice_blocks(
        &self,
        old_range: std::ops::Range<usize>,
        new_blocks: Arc<Vec<LiveBlock>>,
        new_range: std::ops::Range<usize>,
    ) {
        let mut inner = self.inner.borrow_mut();
        inner.blocks = Arc::clone(&new_blocks);
        let old_blocks_len = inner.index.item_count().saturating_sub(1);
        let expected_new_len = old_blocks_len
            .saturating_sub(old_range.len())
            .saturating_add(new_range.len());
        if old_range.end <= old_blocks_len
            && new_range.end <= new_blocks.len()
            && expected_new_len == new_blocks.len()
        {
            inner
                .index
                .splice_blocks(old_range, &new_blocks[new_range]);
        } else {
            let w = inner.index.content_width();
            inner.index.rebuild(&new_blocks, w);
        }
    }

    fn sync_visible_measurements(&self) {
        let vp = self.list_state.viewport_bounds();
        let scroll_top = self.list_state.logical_scroll_top();

        let mut inner = self.inner.borrow_mut();
        let item_count = inner.index.item_count();
        if item_count == 0 {
            return;
        }
        if vp.size.width > px(10.) && inner.drag_total_height.is_none() {
            let avail = (f32::from(vp.size.width) - 2.0 * inner.horiz_pad).max(120.0);
            let content_w = if inner.centered { avail.min(820.0) } else { avail };
            let text_w = (content_w - 10.0).max(120.0);
            if (text_w - inner.index.content_width()).abs() > 2.0 {
                let blocks = Arc::clone(&inner.blocks);
                inner.index.rebuild(&blocks, text_w);
            }
        }

        let start_ix = scroll_top.item_ix.min(item_count);
        let end_ix = (start_ix + 256).min(item_count);
        let limit_bottom = vp.bottom() + px(1300.);
        let mut updates = Vec::with_capacity(64);
        for ix in start_ix..end_ix {
            let Some(b) = self.list_state.bounds_for_item(ix) else {
                break;
            };
            updates.push((ix, f32::from(b.size.height)));
            if b.top() > limit_bottom {
                break;
            }
        }
        if !updates.is_empty() {
            inner.index.update_measured_batch(&updates);
        }
    }

    pub fn scroll_by_wheel(&self, delta_y: Pixels) -> bool {
        self.sync_visible_measurements();
        let vp_h = f32::from(self.list_state.viewport_bounds().size.height);
        if vp_h <= 0.0 {
            return false;
        }
        let scroll_top = self.list_state.logical_scroll_top();
        let dy = f32::from(delta_y);

        enum WheelAction {
            None,
            Start,
            End,
            At(ListOffset),
        }

        let action = {
            let inner = self.inner.borrow();
            let live_max = (inner.index.total_height() - vp_h).max(0.0);
            if live_max <= 0.0 {
                return false;
            }
            let current_y = inner.index.scroll_top_to_y(
                scroll_top.item_ix,
                f32::from(scroll_top.offset_in_item),
                vp_h,
            );
            let new_y = (current_y - dy).clamp(0.0, live_max);
            if (new_y - current_y).abs() < 0.01 {
                WheelAction::None
            } else if new_y >= live_max - 0.5 && dy < 0.0 {
                WheelAction::End
            } else if new_y <= 0.5 {
                WheelAction::Start
            } else {
                let (item_ix, off) = inner.index.y_to_item_offset(new_y);
                WheelAction::At(ListOffset {
                    item_ix,
                    offset_in_item: px(off),
                })
            }
        };

        match action {
            WheelAction::None => false,
            WheelAction::Start => {
                self.list_state.scroll_to(ListOffset {
                    item_ix: 0,
                    offset_in_item: px(0.),
                });
                true
            }
            WheelAction::End => {
                self.list_state.scroll_to_end();
                true
            }
            WheelAction::At(off) => {
                self.list_state.scroll_to(off);
                true
            }
        }
    }

    pub fn reveal_item(&self, ix: usize) {
        self.sync_visible_measurements();
        let vp_h = f32::from(self.list_state.viewport_bounds().size.height);
        if vp_h <= 0.0 {
            return;
        }
        let scroll_top = self.list_state.logical_scroll_top();
        let target = {
            let inner = self.inner.borrow();
            let Some((item_top, item_bottom)) = inner.index.item_range(ix) else {
                return;
            };
            let cur_top = inner.index.scroll_top_to_y(
                scroll_top.item_ix,
                f32::from(scroll_top.offset_in_item),
                vp_h,
            );
            let cur_bottom = cur_top + vp_h;
            if (item_top >= cur_top - 1.0 && item_bottom <= cur_bottom + 1.0)
                || (item_top <= cur_top + 1.0 && item_bottom >= cur_bottom - 1.0)
            {
                None
            } else if item_top < cur_top || item_top >= cur_bottom + vp_h * 0.5 {
                Some(ListOffset {
                    item_ix: ix,
                    offset_in_item: px(0.),
                })
            } else {
                let target_y = (item_bottom - vp_h).max(0.0).min(item_top);
                let (top_ix, off) = inner.index.y_to_item_offset(target_y);
                Some(ListOffset {
                    item_ix: top_ix,
                    offset_in_item: px(off),
                })
            }
        };
        if let Some(off) = target {
            self.list_state.scroll_to(off);
        }
    }
}

impl ScrollbarHandle for LiveScrollbarHandle {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.list_state.viewport_bounds()
    }

    fn offset(&self) -> Point<Pixels> {
        self.sync_visible_measurements();
        let vp_h = f32::from(self.list_state.viewport_bounds().size.height);
        let scroll_top = self.list_state.logical_scroll_top();
        let inner = self.inner.borrow();
        let live_max = (inner.index.total_height() - vp_h).max(0.0);
        if live_max <= 0.0 {
            return point(px(0.), px(0.));
        }
        let live_y = inner.index.scroll_top_to_y(
            scroll_top.item_ix,
            f32::from(scroll_top.offset_in_item),
            vp_h,
        );
        let reported_y = if let Some(frozen_total) = inner.drag_total_height {
            let frozen_max = (frozen_total - vp_h).max(0.0);
            if frozen_max <= 0.0 {
                0.0
            } else {
                (live_y / live_max).clamp(0.0, 1.0) * frozen_max
            }
        } else {
            live_y
        };
        point(px(0.), -px(reported_y))
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        self.sync_visible_measurements();
        let vp_h = f32::from(self.list_state.viewport_bounds().size.height);
        let req_y = (-f32::from(offset.y)).max(0.0);
        enum DragAction {
            Start,
            End,
            At(ListOffset),
        }
        let action = {
            let inner = self.inner.borrow();
            let live_max = (inner.index.total_height() - vp_h).max(0.0);
            let active_total = inner
                .drag_total_height
                .unwrap_or_else(|| inner.index.total_height());
            let active_max = (active_total - vp_h).max(0.0);
            if active_max <= 0.0 || live_max <= 0.0 {
                return;
            }
            let frac = (req_y.clamp(0.0, active_max) / active_max).clamp(0.0, 1.0);
            if frac >= 0.9995 || req_y >= active_max - 0.5 {
                DragAction::End
            } else if frac <= 0.0001 || req_y <= 0.5 {
                DragAction::Start
            } else {
                let (item_ix, off) = inner.index.y_to_item_offset(frac * live_max);
                DragAction::At(ListOffset {
                    item_ix,
                    offset_in_item: px(off),
                })
            }
        };
        match action {
            DragAction::Start => self.list_state.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            }),
            DragAction::End => self.list_state.scroll_to_end(),
            DragAction::At(off) => self.list_state.scroll_to(off),
        }
    }

    fn content_size(&self) -> Size<Pixels> {
        self.sync_visible_measurements();
        let vp = self.list_state.viewport_bounds();
        let inner = self.inner.borrow();
        let total_h = inner
            .drag_total_height
            .unwrap_or_else(|| inner.index.total_height());
        size(vp.size.width, px(total_h))
    }

    fn start_drag(&self) {
        self.sync_visible_measurements();
        let mut inner = self.inner.borrow_mut();
        inner.drag_total_height = Some(inner.index.total_height());
    }

    fn end_drag(&self) {
        self.inner.borrow_mut().drag_total_height = None;
    }
}

fn with_live_wheel_scroll<E: InteractiveElement>(
    el: E,
    scrollbar: LiveScrollbarHandle,
    view_id: EntityId,
) -> E {
    el.on_scroll(move |ev: &ScrollEvent, _window, cx| {
        let delta_y = ev.delta.pixel_delta(px(20.)).y;
        if delta_y != px(0.) {
            let scrolled = scrollbar.scroll_by_wheel(delta_y);
            cx.stop_propagation();
            if scrolled {
                cx.notify(view_id);
            }
        }
    })
}

pub fn render_md_live_preview_pane(
    app: &VKarmaApp,
    side: PaneSide,
    tab: &super::DocumentTab,
    centered: bool,
    cx: &mut Context<VKarmaApp>,
) -> AnyElement {
    let TabKind::Markdown {
        is_mdx,
        live_blocks,
        live_active_block,
        live_editor,
        live_list_state,
        live_scrollbar,
        live_row_bounds,
        ..
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let tab_id = tab.id;
    let tab_path = tab.path.clone();
    let is_mdx = *is_mdx;
    let blocks = Arc::clone(live_blocks);
    let active_ix = *live_active_block;
    let list_state = live_list_state.clone();
    let live_editor = live_editor.clone();
    let row_bounds = live_row_bounds.clone();
    let vault = Arc::clone(&app.vault);
    let extensions = app.markdown_extensions.clone();
    let app_entity = cx.entity();
    let view_id = app_entity.entity_id();
    let font_family = cx.theme().font_family.clone();

    let horiz_pad = px(if centered { 32. } else { 20. });
    live_scrollbar.sync_render_context(&blocks, centered, f32::from(horiz_pad));
    let sb_list = live_scrollbar.clone();
    let total_blocks = blocks.len();

    let list_el = list(list_state.clone(), move |ix, _window, cx| {
        if ix >= total_blocks {
            let app_tail = app_entity.clone();
            let tail = div()
                .id(SharedString::from(format!("lp-tail-{side:?}-{tab_id}")))
                .w_full()
                .px(horiz_pad)
                .h(px(LIVE_TAIL_SPACER_HEIGHT))
                .cursor_text()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    app_tail.update(cx, |app, cx| {
                        app.activate_or_append_bottom_line(side, tab_id, window, cx);
                    });
                })
                .child(div().w_full().when(centered, |d| d.max_w(px(820.)).mx_auto()));
            return with_live_wheel_scroll(tail, sb_list.clone(), view_id).into_any_element();
        }

        let block = &blocks[ix];
        let kind = block.kind;
        let is_active = active_ix == Some(ix);
        let vert_pad = row_vertical_padding(kind);
        let indent = match kind {
            LiveBlockKind::ListItem | LiveBlockKind::TaskItem => list_indent_px(&block.raw),
            _ => 0.0,
        };

        if is_active {
            let app_up = app_entity.clone();
            let app_dn = app_entity.clone();
            let app_lt = app_entity.clone();
            let app_rt = app_entity.clone();
            let app_ent = app_entity.clone();
            let app_bs = app_entity.clone();
            let app_del = app_entity.clone();
            let app_esc = app_entity.clone();
            let app_pst = app_entity.clone();
            let bounds_map = row_bounds.clone();

            let mut editor_row = h_flex().w_full().h_auto().child(BaseTextarea::new(&live_editor));
            editor_row = match kind {
                LiveBlockKind::Heading(1) => editor_row
                    .font_family(font_family.clone())
                    .text_size(px(27.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(relative(1.25))
                    .text_color(hex_color(kd::FG)),
                LiveBlockKind::Heading(2) => editor_row
                    .font_family(font_family.clone())
                    .text_size(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(relative(1.35))
                    .text_color(hex_color(kd::FG)),
                LiveBlockKind::Heading(3) => editor_row
                    .font_family(font_family.clone())
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(relative(1.35))
                    .text_color(hex_color(kd::FG)),
                LiveBlockKind::Heading(_) => editor_row
                    .font_family(font_family.clone())
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(relative(1.35))
                    .text_color(hex_color(kd::FG)),
                LiveBlockKind::CodeFence
                | LiveBlockKind::Frontmatter
                | LiveBlockKind::Table
                | LiveBlockKind::MdxBlock => editor_row
                    .text_size(px(13.5))
                    .line_height(relative(1.5))
                    .text_color(hex_color(kd::FG)),
                _ => editor_row
                    .font_family(font_family.clone())
                    .text_size(px(15.))
                    .line_height(relative(1.65))
                    .text_color(hex_color(kd::FG)),
            };

            let mut active_box = div()
                .id(SharedString::from(format!("lp-active-{side:?}-{tab_id}-{ix}")))
                .w_full()
                .when(indent > 0.0, |d| d.pl(px(indent)))
                .when(kind.is_multiline(), |d| {
                    d.rounded(px(6.))
                        .bg(hex_color(kd::BG_SIDEBAR))
                        .px_3()
                        .py_2()
                })
                .capture_action(move |_: &MoveUp, window, cx| {
                    app_up.update(cx, |app, cx| {
                        app.handle_live_move_up(side, tab_id, ix, window, cx);
                    });
                })
                .capture_action(move |_: &MoveDown, window, cx| {
                    app_dn.update(cx, |app, cx| {
                        app.handle_live_move_down(side, tab_id, ix, window, cx);
                    });
                })
                .capture_action(move |_: &MoveLeft, window, cx| {
                    app_lt.update(cx, |app, cx| {
                        app.handle_live_move_left(side, tab_id, ix, window, cx);
                    });
                })
                .capture_action(move |_: &MoveRight, window, cx| {
                    app_rt.update(cx, |app, cx| {
                        app.handle_live_move_right(side, tab_id, ix, window, cx);
                    });
                })
                .capture_action(move |ev: &Enter, window, cx| {
                    let secondary = ev.secondary;
                    let shift = ev.shift;
                    app_ent.update(cx, |app, cx| {
                        app.handle_live_enter(side, tab_id, ix, secondary, shift, window, cx);
                    });
                })
                .capture_action(move |_: &Backspace, window, cx| {
                    app_bs.update(cx, |app, cx| {
                        app.handle_live_backspace(side, tab_id, ix, window, cx);
                    });
                })
                .capture_action(move |_: &Delete, window, cx| {
                    app_del.update(cx, |app, cx| {
                        app.handle_live_delete(side, tab_id, ix, window, cx);
                    });
                })
                .capture_action(move |_: &Escape, window, cx| {
                    app_esc.update(cx, |app, cx| {
                        app.handle_live_escape(side, tab_id, window, cx);
                    });
                })
                .capture_action(move |_: &Paste, window, cx| {
                    app_pst.update(cx, |app, cx| {
                        app.handle_live_paste_action(side, tab_id, window, cx);
                    });
                })
                .child(
                    canvas(
                        move |bounds, _, _| {
                            bounds_map.borrow_mut().insert(ix, bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .w_full()
                    .h(px(0.)),
                )
                .child(editor_row);

            if kind == LiveBlockKind::ImageEmbed {
                let raw_img = live_editor.read(cx).value();
                let img_pre: SharedString = preprocess_markdown_with_options(
                    raw_img.as_ref(),
                    &tab_path,
                    &vault,
                    0,
                    is_mdx,
                )
                .into();
                let app_links = app_entity.clone();
                active_box = active_box.child(
                    div().pt_1().child(
                        TextView::markdown(
                            SharedString::from(format!("lp-active-img-{side:?}-{tab_id}-{ix}")),
                            img_pre,
                        )
                        .markdown_extensions(extensions.clone())
                        .image_source(resolve_inline_image_source)
                        .selectable(false)
                        .on_link_click(move |url, _, window, cx| {
                            app_links.update(cx, |app, cx| {
                                app.handle_markdown_link(tab_id, side, url, window, cx);
                            });
                        })
                        .w_full()
                        .text_size(px(15.))
                        .line_height(relative(1.65))
                        .text_color(hex_color(kd::FG)),
                    ),
                );
            }

            let active_row = div()
                .id(SharedString::from(format!("lp-row-wrap-{side:?}-{tab_id}-{ix}")))
                .w_full()
                .px(horiz_pad)
                .pt(vert_pad.0)
                .pb(vert_pad.1)
                .child(
                    div()
                        .w_full()
                        .when(centered, |d| d.max_w(px(820.)).mx_auto())
                        .child(active_box),
                );
            return with_live_wheel_scroll(active_row, sb_list.clone(), view_id).into_any_element();
        }

        // Inactive rendered preview block
        let app_click = app_entity.clone();
        let bounds_map = row_bounds.clone();
        if kind == LiveBlockKind::Blank {
            let blank_row = div()
                .id(SharedString::from(format!("lp-row-{side:?}-{tab_id}-{ix}")))
                .w_full()
                .px(horiz_pad)
                .cursor_text()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    app_click.update(cx, |app, cx| {
                        app.activate_live_block_no_scroll(side, tab_id, ix, 0, window, cx);
                    });
                })
                .child(
                    div()
                        .w_full()
                        .when(centered, |d| d.max_w(px(820.)).mx_auto())
                        .child(
                            div()
                                .w_full()
                                .h(px(24.75))
                                .child(
                                    canvas(
                                        move |bounds, _, _| {
                                            bounds_map.borrow_mut().insert(ix, bounds);
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .w_full()
                                    .h(px(0.)),
                                ),
                        ),
                );
            return with_live_wheel_scroll(blank_row, sb_list.clone(), view_id).into_any_element();
        }

        let src_for_preview = match kind {
            LiveBlockKind::ListItem | LiveBlockKind::TaskItem => block.raw.trim_start(),
            _ => block.raw.as_str(),
        };
        let pre_md: SharedString =
            preprocess_markdown_with_options(src_for_preview, &tab_path, &vault, 0, is_mdx).into();

        let app_links = app_entity.clone();
        let app_tasks = app_entity.clone();
        let tv = TextView::markdown(
            SharedString::from(format!("lp-tv-{side:?}-{tab_id}-{ix}")),
            pre_md,
        )
        .markdown_extensions(extensions.clone())
        .image_source(resolve_inline_image_source)
        .selectable(false)
        .on_link_click(move |url, _, window, cx| {
            app_links.update(cx, |app, cx| {
                app.handle_markdown_link(tab_id, side, url, window, cx);
            });
        })
        .on_task_toggle(move |ev, window, cx| {
            app_tasks.update(cx, |app, cx| {
                app.handle_live_task_toggle(side, tab_id, ix, ev.checked(), window, cx);
            });
        })
        .w_full()
        .text_size(px(15.))
        .line_height(relative(1.65))
        .text_color(hex_color(kd::FG));

        let has_link = line_has_link(&block.raw);
        let bounds_for_task = row_bounds.clone();
        let mut row = div()
            .id(SharedString::from(format!("lp-row-{side:?}-{tab_id}-{ix}")))
            .w_full()
            .px(horiz_pad)
            .pt(vert_pad.0)
            .pb(vert_pad.1)
            .cursor_text();

        if has_link {
            row = row.on_click(move |ev: &ClickEvent, window, cx| {
                let pos = ev.position();
                app_click.update(cx, |app, cx| {
                    app.activate_live_block_from_click(side, tab_id, ix, pos, window, cx);
                });
            });
        } else if kind == LiveBlockKind::TaskItem {
            row = row.on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, window, cx| {
                let pos = ev.position;
                let past_checkbox = bounds_for_task
                    .borrow()
                    .get(&ix)
                    .is_none_or(|b| f32::from(pos.x) >= f32::from(b.origin.x) + 24.0);
                if past_checkbox {
                    cx.stop_propagation();
                    app_click.update(cx, |app, cx| {
                        app.activate_live_block_from_click(side, tab_id, ix, pos, window, cx);
                    });
                }
            });
        } else {
            row = row.on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                let pos = ev.position;
                app_click.update(cx, |app, cx| {
                    app.activate_live_block_from_click(side, tab_id, ix, pos, window, cx);
                });
            });
        }

        let row = row.child(
            div()
                .w_full()
                .when(centered, |d| d.max_w(px(820.)).mx_auto())
                .child(
                    div()
                        .w_full()
                        .pr(px(10.))
                        .when(indent > 0.0, |d| d.pl(px(indent)))
                        .child(
                            canvas(
                                move |bounds, _, _| {
                                    bounds_map.borrow_mut().insert(ix, bounds);
                                },
                                |_, _, _, _| {},
                            )
                            .w_full()
                            .h(px(0.)),
                        )
                        .child(tv),
                ),
        );
        with_live_wheel_scroll(row, sb_list.clone(), view_id).into_any_element()
    })
    .size_full();

    div()
        .relative()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .pt_4()
        .child(list_el)
        .child(Scrollbar::vertical(live_scrollbar))
        .into_any_element()
}

fn row_vertical_padding(kind: LiveBlockKind) -> (gpui_kit::Pixels, gpui_kit::Pixels) {
    match kind {
        LiveBlockKind::Heading(1) => (px(12.), px(4.)),
        LiveBlockKind::Heading(2) => (px(10.), px(3.)),
        LiveBlockKind::Heading(_) => (px(8.), px(2.)),
        LiveBlockKind::CodeFence
        | LiveBlockKind::Table
        | LiveBlockKind::Callout
        | LiveBlockKind::Blockquote
        | LiveBlockKind::SceneBreak
        | LiveBlockKind::ImageEmbed
        | LiveBlockKind::Frontmatter
        | LiveBlockKind::MdxBlock => (px(6.), px(6.)),
        LiveBlockKind::ParagraphLine | LiveBlockKind::ListItem | LiveBlockKind::TaskItem => {
            (px(1.5), px(1.5))
        }
        LiveBlockKind::Blank => (px(0.), px(0.)),
    }
}
