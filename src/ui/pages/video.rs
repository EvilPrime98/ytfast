use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    Bounds, Context, Corners, EventEmitter, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, Render, RenderImage,
    StatefulInteractiveElement, Styled, Subscription, Window, canvas, div, prelude::*, px, rgba,
};

use crate::api::ytdlp;
use crate::model::format_duration;
use crate::route::Navigate;
use crate::runtime;
use crate::state::Stores;
use crate::state::player::{PlayState, VideoState};
use crate::ui::components::button::{ButtonVariant, button};
use crate::ui::components::dropdown::dropdown;
use crate::ui::components::track_row::{thumbnail, track_row};
use crate::ui::icons::{Icon, icon};
use crate::ui::shell::player_bar::{bar, fraction};
use crate::ui::theme;
use crate::video::Playback;

pub const HEIGHTS: [u32; 4] = [360, 480, 720, 1080];

const CONTROLS_HIDE_AFTER: Duration = Duration::from_secs(3);

#[derive(Default)]
struct FrameSlot {
    current: Option<Arc<RenderImage>>,
    retired: Vec<Arc<RenderImage>>,
}

impl FrameSlot {
    fn show(&mut self, image: Option<Arc<RenderImage>>) {
        self.retired
            .extend(std::mem::replace(&mut self.current, image));
    }
}

pub struct VideoPage {
    stores: Stores,
    playback: Option<Playback>,
    started: Option<(PathBuf, u64)>,
    play_error: Option<String>,
    description: Option<(String, String)>,
    last_activity: Instant,
    clock_ref: (Duration, Instant),
    controls_shown: bool,
    screen_bounds: Rc<Cell<Bounds<Pixels>>>,
    frame: Rc<RefCell<FrameSlot>>,
    seek_bounds: Rc<Cell<Bounds<Pixels>>>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<Navigate> for VideoPage {}

impl VideoPage {
    pub fn new(stores: Stores, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe(&stores.player, |this, _, cx| {
                this.ensure_queue_thumbs(cx);
                this.ensure_description(cx);
                cx.notify();
            }),
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
            cx.observe(&stores.prefs, |_, _, cx| cx.notify()),
        ];
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| this.pump(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
                if this.update(cx, |this, cx| this.pull_frame(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut this = Self {
            stores,
            playback: None,
            started: None,
            play_error: None,
            description: None,
            last_activity: Instant::now(),
            clock_ref: (Duration::ZERO, Instant::now()),
            controls_shown: true,
            screen_bounds: Rc::default(),
            frame: Rc::default(),
            seek_bounds: Rc::default(),
            _subs: subs,
        };
        this.ensure_queue_thumbs(cx);
        this.ensure_description(cx);
        this
    }

    fn ensure_queue_thumbs(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .stores
            .player
            .read(cx)
            .queue
            .iter()
            .map(|t| t.id.clone())
            .collect();
        self.stores.thumbs.update(cx, |t, cx| {
            for id in &ids {
                t.ensure(id, cx);
            }
        });
    }

    fn ensure_description(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.stores.player.read(cx).current().map(|t| t.id.clone()) else {
            return;
        };
        if self.description.as_ref().is_some_and(|(d, _)| *d == id) {
            return;
        }
        self.description = Some((id.clone(), String::new()));
        let bin = self.stores.prefs.read(cx).prefs.ytdlp_path.clone();
        cx.spawn(async move |this, cx| {
            let wanted = id.clone();
            let text = runtime::run(async move { ytdlp::description(&bin, &id).await })
                .await
                .unwrap_or_default();
            this.update(cx, |this, cx| {
                if this.description.as_ref().is_some_and(|(d, _)| *d == wanted) {
                    this.description = Some((wanted, text));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn screen_size(&self, window: &Window) -> Option<(u32, u32)> {
        let b = self.screen_bounds.get();
        let s = window.scale_factor();
        let (w, h) = (f32::from(b.size.width) * s, f32::from(b.size.height) * s);
        (w >= 1.0 && h >= 1.0).then(|| (w.round() as u32, h.round() as u32))
    }

    fn pull_frame(&mut self, cx: &mut Context<Self>) {
        let Some(image) = self.playback.as_mut().and_then(|pb| pb.take_frame()) else {
            return;
        };
        self.frame.borrow_mut().show(Some(image));
        cx.notify();
    }

    fn pump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (video, epoch, state, reported, speed, lib) = {
            let p = self.stores.player.read(cx);
            (
                p.video.clone(),
                p.seek_epoch,
                p.state,
                p.clock(),
                p.speed,
                self.stores.prefs.read(cx).prefs.libmpv_path.clone(),
            )
        };
        let mut dirty = false;
        if reported != self.clock_ref.0 {
            self.clock_ref = (reported, Instant::now());
        }
        let clock = if state == PlayState::Playing {
            let since = self.clock_ref.1.elapsed().min(Duration::from_millis(250));
            reported + since.mul_f32(speed)
        } else {
            reported
        };
        let rect = self.screen_size(window);

        match video {
            VideoState::Ready(path) => {
                let active = matches!(state, PlayState::Playing | PlayState::Paused);
                let current = self
                    .started
                    .as_ref()
                    .is_some_and(|(p, e)| *p == path && *e == epoch);
                if active && !current && rect.is_some() {
                    let same_file = self.started.as_ref().is_some_and(|(p, _)| *p == path);
                    let paused = state == PlayState::Paused;
                    let result = match self.playback.as_mut() {
                        Some(pb) if same_file => {
                            pb.seek(clock);
                            Ok(())
                        }
                        Some(pb) => pb.load(&path, clock, paused),
                        None => Playback::new(&lib).and_then(|mut pb| {
                            if let Some(r) = rect {
                                pb.set_size(r);
                            }
                            pb.load(&path, clock, paused)?;
                            self.playback = Some(pb);
                            Ok(())
                        }),
                    };
                    if let Err(e) = &result {
                        tracing::warn!("video: {e}");
                    }
                    self.play_error = result.err();
                    self.started = Some((path, epoch));
                    dirty = true;
                }
            }
            _ => {
                if self.playback.is_some() || self.started.is_some() {
                    self.playback = None;
                    self.frame.borrow_mut().show(None);
                    self.started = None;
                    self.play_error = None;
                    dirty = true;
                }
            }
        }

        if let Some(pb) = self.playback.as_mut() {
            if let Some(r) = rect {
                pb.set_size(r);
            }
            pb.drive(state == PlayState::Playing, clock, speed);
        }
        if !window.is_fullscreen() {
            self.last_activity = Instant::now();
        }

        let shown = !window.is_fullscreen()
            || state != PlayState::Playing
            || self.last_activity.elapsed() < CONTROLS_HIDE_AFTER;
        if shown != self.controls_shown {
            self.controls_shown = shown;
            dirty = true;
        }
        if dirty {
            cx.notify();
        }
    }

    fn controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let player = self.stores.player.read(cx);
        let state = player.state;
        let position = player.position;
        let duration = player.duration;
        let has_prev = player.index.is_some();
        let has_next = player.index.is_some_and(|ix| ix + 1 < player.queue.len());
        let progress = match duration {
            Some(d) if !d.is_zero() => position.as_secs_f32() / d.as_secs_f32(),
            _ => 0.0,
        };
        let play_glyph = if state == PlayState::Playing {
            Icon::Pause
        } else {
            Icon::Play
        };
        let seek_bounds = self.seek_bounds.clone();
        let flat = |id: &'static str, glyph: Icon, on: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .size(px(36.0))
                .rounded_full()
                .cursor_pointer()
                .opacity(if on { 1.0 } else { 0.4 })
                .hover(|s| s.bg(rgba(0xffffff26)))
                .child(icon(glyph, px(22.0)).text_color(gpui::white()))
        };
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .w_full()
            .child(flat("vid-prev", Icon::Prev, has_prev).on_click(
                cx.listener(|this, _, _, cx| this.stores.player.update(cx, |p, cx| p.previous(cx))),
            ))
            .child(flat("vid-play", play_glyph, true).on_click(
                cx.listener(|this, _, _, cx| this.stores.player.update(cx, |p, cx| p.toggle(cx))),
            ))
            .child(flat("vid-next", Icon::Next, has_next).on_click(
                cx.listener(|this, _, _, cx| this.stores.player.update(cx, |p, cx| p.next(cx))),
            ))
            .child(
                div()
                    .w(px(44.0))
                    .flex_none()
                    .text_size(px(11.0))
                    .text_color(gpui::white())
                    .child(format_duration(Some(position.as_secs()))),
            )
            .child(
                bar("vid-seek", progress, seek_bounds.clone()).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        let f = fraction(seek_bounds.get(), ev.position);
                        this.stores
                            .player
                            .update(cx, |p, cx| p.seek_fraction(f, cx));
                    }),
                ),
            )
            .child(
                div()
                    .w(px(44.0))
                    .flex_none()
                    .text_size(px(11.0))
                    .text_color(gpui::white())
                    .child(format_duration(duration.map(|d| d.as_secs()))),
            )
            .child(
                div()
                    .id("vid-fullscreen")
                    .flex_none()
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(gpui::white())
                    .hover(|s| s.bg(rgba(0xffffff26)))
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.toggle_fullscreen();
                        cx.notify();
                    }))
                    .child("Exit fullscreen"),
            )
    }
}

impl Render for VideoPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fullscreen = window.is_fullscreen();
        let player = self.stores.player.read(cx);
        let current = player.current().cloned();
        let queue = player.queue.clone();
        let current_ix = player.index;
        let video = player.video.clone();
        let state = player.state;
        let art = current
            .as_ref()
            .and_then(|t| self.stores.thumbs.read(cx).get(&t.id));

        let message = match (&self.play_error, &video) {
            (Some(e), _) => Some(format!("Could not play the video: {e}")),
            (None, VideoState::Unavailable) => {
                Some("Video unavailable, playing audio only.".to_string())
            }
            (None, VideoState::Fetching) => Some("Downloading the video…".to_string()),
            (None, VideoState::Off) => Some("Video mode is off.".to_string()),
            (None, VideoState::Ready(_)) if self.playback.is_none() && state != PlayState::Idle => {
                Some("Starting the video…".to_string())
            }
            _ => None,
        };

        let bounds = self.screen_bounds.clone();
        let frame = self.frame.clone();
        let screen = div()
            .id("video-screen")
            .relative()
            .flex()
            .flex_1()
            .min_h(px(180.0))
            .items_center()
            .justify_center()
            .bg(gpui::black())
            .child(
                canvas(
                    move |b, _, _| bounds.set(b),
                    move |b, _, window, _| {
                        let mut slot = frame.borrow_mut();
                        for old in slot.retired.drain(..) {
                            let _ = window.drop_image(old);
                        }
                        if let Some(img) = slot.current.clone() {
                            let _ = window.paint_image(b, b, Corners::default(), img, 0, false);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .when(self.playback.is_none(), |s| {
                s.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(12.0))
                        .child(thumbnail(art.clone(), 320.0, 180.0))
                        .when_some(message, |s, m| {
                            s.child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(theme::text_muted())
                                    .child(m),
                            )
                        }),
                )
            })
            .on_click(
                cx.listener(|this, _, _, cx| this.stores.player.update(cx, |p, cx| p.toggle(cx))),
            )
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| {
                this.last_activity = Instant::now();
                if !this.controls_shown {
                    this.controls_shown = true;
                    cx.notify();
                }
            }));

        if fullscreen {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .bg(gpui::black())
                .child(screen)
                .when(self.controls_shown, |s| {
                    s.child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_none()
                            .gap(px(6.0))
                            .px(px(16.0))
                            .py(px(10.0))
                            .bg(rgba(0x000000f2))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(gpui::white())
                                    .child(
                                        current
                                            .as_ref()
                                            .map(|t| t.title.clone())
                                            .unwrap_or_default(),
                                    ),
                            )
                            .child(self.controls(cx)),
                    )
                })
                .into_any_element();
        }

        let quality_menu = {
            let prefs = self.stores.prefs.read(cx);
            let height = prefs.prefs.video_max_height;
            let open = prefs.open_menu == Some("video-quality");
            let (open_s, select_s, player) = (
                self.stores.prefs.clone(),
                self.stores.prefs.clone(),
                self.stores.player.clone(),
            );
            let options = HEIGHTS
                .into_iter()
                .map(|h| (h, format!("{h}p").into()))
                .collect();
            dropdown(
                "video-quality",
                options,
                height,
                open,
                true,
                move |o, cx| open_s.update(cx, |p, cx| p.set_menu_open("video-quality", o, cx)),
                move |v, cx| {
                    select_s.update(cx, |p, cx| p.update(cx, |p| p.video_max_height = v));
                    player.update(cx, |p, cx| p.reload_video(cx));
                },
            )
        };

        let description = self
            .description
            .as_ref()
            .filter(|(id, _)| current.as_ref().is_some_and(|t| t.id == *id))
            .map(|(_, d)| d.clone())
            .unwrap_or_default();

        let rows: Vec<_> = queue
            .iter()
            .enumerate()
            .map(|(ix, track)| {
                let thumb = self.stores.thumbs.read(cx).get(&track.id);
                let stores = self.stores.clone();
                track_row(
                    "video-queue",
                    ix,
                    track,
                    thumb,
                    current_ix == Some(ix),
                    move |_, cx| stores.player.update(cx, |p, cx| p.play_index(ix, cx)),
                    Vec::new(),
                )
            })
            .collect();

        div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap(px(24.0))
            .p(px(24.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .gap(px(12.0))
                    .child(screen)
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(18.0))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child(
                                                current
                                                    .as_ref()
                                                    .map_or("Nothing playing".to_string(), |t| {
                                                        t.title.clone()
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.0))
                                            .text_color(theme::text_muted())
                                            .child(
                                                current
                                                    .as_ref()
                                                    .map(|t| t.channel.clone())
                                                    .unwrap_or_default(),
                                            ),
                                    ),
                            )
                            .child(quality_menu),
                    )
                    .child(
                        div()
                            .id("video-description")
                            .flex_none()
                            .max_h(px(110.0))
                            .overflow_y_scroll()
                            .text_size(px(13.0))
                            .text_color(theme::text_muted())
                            .child(description),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .w(px(340.0))
                    .min_h_0()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(16.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(format!("Up next · {} tracks", queue.len())),
                            )
                            .child(button(
                                "video-fullscreen",
                                "Fullscreen",
                                ButtonVariant::Secondary,
                                cx.listener(|_, _, window, cx| {
                                    window.toggle_fullscreen();
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        div()
                            .id("video-queue-list")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .gap(px(2.0))
                            .children(rows),
                    ),
            )
            .into_any_element()
    }
}
