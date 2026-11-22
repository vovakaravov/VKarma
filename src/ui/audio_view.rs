use super::{TabKind, VKarmaApp};
use crate::formats::archive::format_bytes;
use crate::formats::audio::{AudioAnalysis, AudioPlayer, analyze_audio, format_time_mmss};
use crate::theme::{hex_alpha, hex_color, kd};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Bounds, ClipboardItem, Context, Entity, FocusHandle, Focusable,
    FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    Window, canvas, div, fill, point, prelude::FluentBuilder as _, px, size,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct AudioNote {
    pub time_secs: f64,
    pub text: String,
}

pub struct AudioView {
    audio_path: PathBuf,
    pub analysis: Option<AudioAnalysis>,
    loading: bool,
    load_error: Option<String>,
    player: AudioPlayer,
    playing: bool,
    position_secs: f64,
    play_started_at: Option<Instant>,
    play_offset_secs: f64,
    volume: f32,
    play_gen: u64,
    wave_geo: Rc<Cell<(f32, f32)>>,
    notes: Vec<AudioNote>,
    note_input: Entity<InputState>,
    _note_sub: Subscription,
    focus_handle: FocusHandle,
}

impl Focusable for AudioView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl AudioView {
    pub fn new(audio_path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let note_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Add note at current timestamp (Enter)…")
        });
        let note_sub = cx.subscribe_in(&note_input, window, |this, inp, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let text = inp.read(cx).value().trim().to_string();
                if !text.is_empty() {
                    this.add_note_at_current(text, cx);
                    inp.update(cx, |s, cx| s.set_value("", window, cx));
                }
            }
        });

        let path_clone = audio_path.clone();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { analyze_audio(&path_clone) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.loading = false;
                match res {
                    Ok(a) => {
                        view.analysis = Some(a);
                        view.load_error = None;
                    }
                    Err(err) => {
                        view.load_error = Some(format!("{err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();

        Self {
            audio_path,
            analysis: None,
            loading: true,
            load_error: None,
            player: AudioPlayer::new(),
            playing: false,
            position_secs: 0.0,
            play_started_at: None,
            play_offset_secs: 0.0,
            volume: 0.85,
            play_gen: 0,
            wave_geo: Rc::new(Cell::new((0.0, 600.0))),
            notes: Vec::new(),
            note_input,
            _note_sub: note_sub,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn relocate_audio(&mut self, new_path: PathBuf) {
        self.stop_playback();
        self.audio_path = new_path;
    }

    fn duration_secs(&self) -> f64 {
        self.analysis
            .as_ref()
            .map(|a| a.meta.duration_secs)
            .unwrap_or(0.0)
    }

    fn current_position(&self) -> f64 {
        if self.playing
            && let Some(started) = self.play_started_at
        {
            let dur = self.duration_secs();
            let pos = self.play_offset_secs + started.elapsed().as_secs_f64();
            if dur > 0.0 { pos.min(dur) } else { pos }
        } else {
            self.position_secs
        }
    }

    fn stop_playback(&mut self) {
        self.position_secs = self.current_position();
        self.playing = false;
        self.play_started_at = None;
        self.play_gen = self.play_gen.wrapping_add(1);
        self.player.stop();
    }

    fn start_playback(&mut self, cx: &mut Context<Self>) {
        let dur = self.duration_secs();
        if dur > 0.0 && self.position_secs >= dur - 0.05 {
            self.position_secs = 0.0;
        }
        if let Err(err) = self
            .player
            .start(&self.audio_path, self.position_secs, self.volume)
        {
            self.load_error = Some(format!("{err}"));
            self.playing = false;
            cx.notify();
            return;
        }
        self.load_error = None;
        self.playing = true;
        self.play_offset_secs = self.position_secs;
        self.play_started_at = Some(Instant::now());
        self.play_gen = self.play_gen.wrapping_add(1);
        let gen_id = self.play_gen;

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
                let keep_going = this
                    .update(cx, |view, cx| {
                        if !view.playing || view.play_gen != gen_id {
                            return false;
                        }
                        if let Some(err) = view.player.poll_failure() {
                            view.load_error = Some(err);
                            view.playing = false;
                            view.play_started_at = None;
                            cx.notify();
                            return false;
                        }
                        let pos = view.current_position();
                        let dur = view.duration_secs();
                        view.position_secs = pos;
                        if dur > 0.0 && pos >= dur {
                            view.playing = false;
                            view.play_started_at = None;
                            view.position_secs = dur;
                            view.player.stop();
                            cx.notify();
                            return false;
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.playing {
            self.stop_playback();
            cx.notify();
        } else {
            self.start_playback(cx);
        }
    }

    pub fn seek_to(&mut self, secs: f64, cx: &mut Context<Self>) {
        let dur = self.duration_secs();
        let clamped = if dur > 0.0 {
            secs.clamp(0.0, dur)
        } else {
            secs.max(0.0)
        };
        let was_playing = self.playing;
        self.stop_playback();
        self.position_secs = clamped;
        if was_playing {
            self.start_playback(cx);
        } else {
            cx.notify();
        }
    }

    pub fn set_volume(&mut self, next_vol: f32, cx: &mut Context<Self>) {
        self.volume = next_vol.clamp(0.0, 1.0);
        if self.playing {
            let pos = self.current_position();
            self.stop_playback();
            self.position_secs = pos;
            self.start_playback(cx);
        } else {
            cx.notify();
        }
    }

    fn add_note_at_current(&mut self, text: String, cx: &mut Context<Self>) {
        let t = self.current_position();
        self.notes.push(AudioNote {
            time_secs: t,
            text,
        });
        self.notes
            .sort_by(|a, b| a.time_secs.total_cmp(&b.time_secs));
        cx.notify();
    }

    pub fn notes_markdown(&self) -> String {
        self.notes
            .iter()
            .map(|n| format!("- [{}] {}", format_time_mmss(n.time_secs), n.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn summary(&self) -> String {
        if self.loading {
            "Analyzing audio…".to_string()
        } else if let Some(ref a) = self.analysis {
            format!(
                "{} · {} Hz · {}ch · {}",
                a.meta.codec,
                a.meta.sample_rate,
                a.meta.channels,
                format_time_mmss(a.meta.duration_secs)
            )
        } else {
            "Audio".to_string()
        }
    }
}

impl Render for AudioView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cur_pos = self.current_position();
        let dur = self.duration_secs();
        let progress = if dur > 0.0 {
            (cur_pos / dur).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let peaks = self
            .analysis
            .as_ref()
            .map(|a| a.peaks.clone())
            .unwrap_or_else(|| vec![0.15; 120]);
        let marker_fracs: Vec<f32> = if dur > 0.0 {
            self.notes
                .iter()
                .map(|n| (n.time_secs / dur).clamp(0.0, 1.0) as f32)
                .collect()
        } else {
            Vec::new()
        };
        let wave_geo = self.wave_geo.clone();

        let waveform_canvas = canvas(
            move |bounds, _, _| {
                wave_geo.set((f32::from(bounds.origin.x), f32::from(bounds.size.width)));
                bounds
            },
            move |bounds, _, window, _| {
                let ox = f32::from(bounds.origin.x);
                let oy = f32::from(bounds.origin.y);
                let w = f32::from(bounds.size.width);
                let h = f32::from(bounds.size.height);
                if w <= 8.0 || h <= 8.0 || peaks.is_empty() {
                    return;
                }
                let mid_y = oy + h * 0.5;
                let max_bar_h = (h - 20.0).max(16.0);
                let slot_w = w / (peaks.len() as f32);
                let bar_w = (slot_w * 0.68).clamp(1.5, 6.0);

                for (ix, &pk) in peaks.iter().enumerate() {
                    let frac = (ix as f32 + 0.5) / (peaks.len() as f32);
                    let bh = (pk * max_bar_h).max(4.0);
                    let bx = ox + (ix as f32) * slot_w + (slot_w - bar_w) * 0.5;
                    let by = mid_y - bh * 0.5;
                    let col = if frac <= progress {
                        hex_color(kd::BLUE)
                    } else {
                        hex_alpha(0x8ba4b044)
                    };
                    window.paint_quad(fill(
                        Bounds {
                            origin: point(px(bx), px(by)),
                            size: size(px(bar_w), px(bh)),
                        },
                        col,
                    ));
                }

                // Timestamp note markers
                for mfrac in marker_fracs {
                    let mx = ox + mfrac * w;
                    window.paint_quad(fill(
                        Bounds {
                            origin: point(px(mx - 1.0), px(oy + 4.0)),
                            size: size(px(2.0), px(h - 8.0)),
                        },
                        hex_color(kd::YELLOW),
                    ));
                }

                // Playhead needle
                let px_x = ox + progress * w;
                window.paint_quad(fill(
                    Bounds {
                        origin: point(px(px_x - 1.0), px(oy + 2.0)),
                        size: size(px(2.5), px(h - 4.0)),
                    },
                    hex_color(kd::ORANGE),
                ));
            },
        )
        .size_full();

        let title_line = self
            .analysis
            .as_ref()
            .and_then(|a| a.meta.title.clone())
            .unwrap_or_else(|| {
                self.audio_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Audio")
                    .to_string()
            });
        let sub_meta = self.analysis.as_ref().map(|a| {
            let mut parts = Vec::new();
            if let Some(ref artist) = a.meta.artist {
                parts.push(artist.clone());
            }
            if let Some(ref album) = a.meta.album {
                parts.push(album.clone());
            }
            parts.push(format!(
                "{} · {} Hz · {} ch · {} kbps",
                a.meta.codec, a.meta.sample_rate, a.meta.channels, a.meta.bitrate_kbps
            ));
            parts.join(" · ")
        });

        div()
            .id(SharedString::from(format!("audio-view-{:?}", cx.entity_id())))
            .size_full()
            .bg(hex_color(kd::BG_MAIN))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                match ev.keystroke.key.as_str() {
                    "space" => {
                        cx.stop_propagation();
                        this.toggle_play(cx);
                    }
                    "left" => {
                        cx.stop_propagation();
                        let pos = this.current_position();
                        this.seek_to(pos - 5.0, cx);
                    }
                    "right" => {
                        cx.stop_propagation();
                        let pos = this.current_position();
                        this.seek_to(pos + 5.0, cx);
                    }
                    _ => {}
                }
            }))
            .child(
                v_flex()
                    .size_full()
                    .max_w(px(880.))
                    .mx_auto()
                    .p_6()
                    .gap_4()
                    .child(
                        v_flex()
                            .p_5()
                            .gap_3()
                            .rounded(px(10.))
                            .bg(hex_color(kd::BG_SIDEBAR))
                            .border_1()
                            .border_color(hex_color(kd::BORDER_SUBTLE))
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        v_flex()
                                            .gap_0p5()
                                            .child(
                                                div()
                                                    .text_base()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(hex_color(kd::FG))
                                                    .child(title_line),
                                            )
                                            .when_some(sub_meta, |col, m| {
                                                col.child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(hex_color(kd::FG_MUTED))
                                                        .child(m),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(hex_color(kd::AQUA))
                                            .child(format!(
                                                "{} / {}",
                                                format_time_mmss(cur_pos),
                                                format_time_mmss(dur)
                                            )),
                                    ),
                            )
                            .when_some(self.load_error.as_ref(), |col, err| {
                                col.child(
                                    div()
                                        .p_2()
                                        .rounded(px(6.))
                                        .bg(hex_color(kd::BG_DEEP))
                                        .text_xs()
                                        .text_color(hex_color(kd::RED))
                                        .child(err.clone()),
                                )
                            })
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "audio-wave-{:?}",
                                        cx.entity_id()
                                    )))
                                    .w_full()
                                    .h(px(140.))
                                    .rounded(px(8.))
                                    .bg(hex_color(kd::BG_DEEP))
                                    .border_1()
                                    .border_color(hex_color(kd::BORDER_SUBTLE))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                                            let (ox, w) = this.wave_geo.get();
                                            let d = this.duration_secs();
                                            if w > 8.0 && d > 0.0 {
                                                let frac = ((f32::from(ev.position.x) - ox) / w)
                                                    .clamp(0.0, 1.0);
                                                this.seek_to(frac as f64 * d, cx);
                                            }
                                        }),
                                    )
                                    .child(waveform_canvas),
                            )
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        h_flex()
                                            .gap_1p5()
                                            .items_center()
                                            .child(
                                                Button::new("aud-rw-btn")
                                                    .ghost()
                                                    .small()
                                                    .label("-5s")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        let pos = this.current_position();
                                                        this.seek_to(pos - 5.0, cx);
                                                    })),
                                            )
                                            .child(
                                                Button::new("aud-play-btn")
                                                    .primary()
                                                    .small()
                                                    .label(if self.playing { "Pause" } else { "Play" })
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.toggle_play(cx);
                                                    })),
                                            )
                                            .child(
                                                Button::new("aud-stop-btn")
                                                    .secondary()
                                                    .small()
                                                    .label("Stop")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.stop_playback();
                                                        this.position_secs = 0.0;
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                Button::new("aud-ff-btn")
                                                    .ghost()
                                                    .small()
                                                    .label("+5s")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        let pos = this.current_position();
                                                        this.seek_to(pos + 5.0, cx);
                                                    })),
                                            ),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_1p5()
                                            .items_center()
                                            .child(
                                                Button::new("aud-vdown")
                                                    .ghost()
                                                    .xsmall()
                                                    .label("Vol -")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.set_volume(this.volume - 0.15, cx);
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(hex_color(kd::FG_MUTED))
                                                    .child(format!(
                                                        "{}%",
                                                        (self.volume * 100.0).round() as i32
                                                    )),
                                            )
                                            .child(
                                                Button::new("aud-vup")
                                                    .ghost()
                                                    .xsmall()
                                                    .label("Vol +")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.set_volume(this.volume + 0.15, cx);
                                                    })),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_h_0()
                            .p_4()
                            .gap_2p5()
                            .rounded(px(10.))
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
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(hex_color(kd::BLUE))
                                            .child(format!(
                                                "Timestamped Notes ({})",
                                                self.notes.len()
                                            )),
                                    )
                                    .child(
                                        Button::new("aud-copy-notes")
                                            .with_variant(if self.notes.is_empty() {
                                                ButtonVariant::Ghost
                                            } else {
                                                ButtonVariant::Secondary
                                            })
                                            .xsmall()
                                            .icon(IconName::Copy)
                                            .label("Copy Notes")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let md = this.notes_markdown();
                                                if !md.is_empty() {
                                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                                        md,
                                                    ));
                                                }
                                            })),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(div().flex_1().child(Input::new(&self.note_input).xsmall()))
                                    .child(
                                        Button::new("aud-add-note")
                                            .secondary()
                                            .xsmall()
                                            .icon(IconName::Plus)
                                            .label("Marker")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                let txt = this
                                                    .note_input
                                                    .read(cx)
                                                    .value()
                                                    .trim()
                                                    .to_string();
                                                let note = if txt.is_empty() {
                                                    "Bookmark".to_string()
                                                } else {
                                                    txt
                                                };
                                                this.add_note_at_current(note, cx);
                                                this.note_input.update(cx, |s, cx| {
                                                    s.set_value("", window, cx)
                                                });
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "aud-notes-scroll-{:?}",
                                        cx.entity_id()
                                    )))
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scrollbar()
                                    .child(v_flex().gap_1().children(self.notes.iter().enumerate().map(
                                        |(ix, n)| {
                                            let ts = n.time_secs;
                                            h_flex()
                                                .id(SharedString::from(format!("aud-note-{ix}")))
                                                .px_2p5()
                                                .py_1p5()
                                                .rounded(px(5.))
                                                .bg(hex_color(kd::BG_MAIN))
                                                .justify_between()
                                                .items_center()
                                                .gap_2()
                                                .cursor_pointer()
                                                .hover(|s| s.bg(hex_color(kd::BG_PANEL)))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.seek_to(ts, cx);
                                                }))
                                                .child(
                                                    h_flex()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .gap_2()
                                                        .items_center()
                                                        .child(
                                                            div()
                                                                .px_1p5()
                                                                .py_0p5()
                                                                .rounded(px(4.))
                                                                .bg(hex_color(kd::BG_DEEP))
                                                                .text_xs()
                                                                .font_weight(FontWeight::BOLD)
                                                                .text_color(hex_color(kd::ORANGE))
                                                                .child(format_time_mmss(ts)),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .min_w_0()
                                                                .text_xs()
                                                                .text_color(hex_color(kd::FG))
                                                                .child(n.text.clone()),
                                                        ),
                                                )
                                                .child(
                                                    Button::new(SharedString::from(format!(
                                                        "aud-note-del-{ix}"
                                                    )))
                                                    .ghost()
                                                    .xsmall()
                                                    .icon(IconName::X)
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        cx.stop_propagation();
                                                        if ix < this.notes.len() {
                                                            this.notes.remove(ix);
                                                            cx.notify();
                                                        }
                                                    })),
                                                )
                                        },
                                    ))),
                            ),
                    ),
            )
            .into_any_element()
    }
}

pub fn render_audio_tab(
    app: &VKarmaApp,
    tab: &super::DocumentTab,
    cx: &mut Context<VKarmaApp>,
) -> impl IntoElement {
    let TabKind::Audio {
        view,
        file_size_bytes,
    } = &tab.kind
    else {
        return div().into_any_element();
    };

    let rel_path = tab.rel_path.clone();
    let zen_mode = app.zen_mode;
    let summary = view.read(cx).summary();
    let sz = format_bytes(*file_size_bytes);

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
                        .text_color(hex_color(kd::ORANGE))
                        .child(format!("{summary} · {sz}")),
                ),
        );

    v_flex()
        .size_full()
        .bg(hex_color(kd::BG_MAIN))
        .when(!zen_mode, |this| this.child(header))
        .child(div().flex_1().min_h_0().w_full().child(view.clone()))
        .into_any_element()
}
