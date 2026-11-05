use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::ColorExt as _;
use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Global, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, Pixels, Render, StatefulInteractiveElement, Styled,
    Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, px,
    relative, size,
};

use crate::model::format_duration;
use crate::platform;
use crate::state::Stores;
use crate::state::player::PlayState;
use crate::ui::components::track_row::thumbnail;
use crate::ui::icons::{Icon, icon};
use crate::ui::shell::header::logo;
use crate::ui::shell::player_bar::{bar, fraction, volume_drag};
use crate::ui::theme;

const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 272.0;

const SETTLE: Duration = Duration::from_millis(8);

const REOPEN_GUARD: Duration = Duration::from_millis(400);

#[derive(Default)]
struct Popup {
    handle: Option<AnyWindowHandle>,
    native: Option<(isize, f32)>,
    visible: bool,
    closed_at: Option<Instant>,
}

impl Global for Popup {}

pub fn is_popup(cx: &App, handle: AnyWindowHandle) -> bool {
    cx.try_global::<Popup>()
        .is_some_and(|p| p.handle == Some(handle))
}

pub fn close(cx: &mut App) {
    let Some(popup) = cx.try_global::<Popup>() else {
        return;
    };
    let (true, Some((raw, _))) = (popup.visible, popup.native) else {
        return;
    };
    let popup = cx.global_mut::<Popup>();
    popup.visible = false;
    popup.closed_at = Some(Instant::now());
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SETTLE).await;
        platform::window::hide_popup(raw);
    })
    .detach();
}

pub fn toggle(cx: &mut App, x: i32, y: i32) {
    if !cx.has_global::<Popup>() {
        cx.set_global(Popup::default());
    }
    if cx.global::<Popup>().visible {
        close(cx);
        return;
    }
    if cx
        .global::<Popup>()
        .closed_at
        .is_some_and(|t| t.elapsed() < REOPEN_GUARD)
    {
        return;
    }
    if cx.global::<Popup>().handle.is_none() {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(WIDTH), px(HEIGHT)),
            ))),
            titlebar: None,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Opaque,
            app_id: Some("ytfast".into()),
            show: false,
            focus: false,
            ..Default::default()
        };
        let stores = cx.global::<Stores>().clone();
        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| {
                cx.observe_window_activation(window, |_, window, cx| {
                    if !window.is_window_active() {
                        cx.defer(close);
                    }
                })
                .detach();
                MiniPlayer::new(stores, cx)
            })
        });
        let Ok(handle) = opened else {
            return;
        };
        let native = handle
            .update(cx, |_, window, _| platform::window::native(window))
            .ok()
            .flatten();
        let popup = cx.global_mut::<Popup>();
        popup.handle = Some(handle.into());
        popup.native = native;
    }
    let Some((raw, scale)) = cx.global::<Popup>().native else {
        return;
    };
    cx.global_mut::<Popup>().visible = true;
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SETTLE).await;
        platform::window::present_popup(raw, scale, x, y, WIDTH, HEIGHT);
    })
    .detach();
}

struct MiniPlayer {
    stores: Stores,
    volume_bounds: Rc<Cell<Bounds<Pixels>>>,
    volume_dragging: Rc<Cell<bool>>,
}

impl MiniPlayer {
    fn new(stores: Stores, cx: &mut Context<Self>) -> Self {
        cx.observe(&stores.player, |this, player, cx| {
            if let Some(track) = player.read(cx).current() {
                let id = track.id.clone();
                this.stores.thumbs.update(cx, |t, cx| t.ensure(&id, cx));
            }
            cx.notify();
        })
        .detach();
        cx.observe(&stores.thumbs, |_, _, cx| cx.notify()).detach();
        if let Some(track) = stores.player.read(cx).current() {
            let id = track.id.clone();
            stores.thumbs.update(cx, |t, cx| t.ensure(&id, cx));
        }
        Self {
            stores,
            volume_bounds: Rc::default(),
            volume_dragging: Rc::default(),
        }
    }
}

fn side_button(id: &'static str, glyph: Icon, enabled: bool) -> gpui::Stateful<gpui::Div> {
    let b = div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(34.0))
        .rounded_full()
        .opacity(if enabled { 1.0 } else { 0.35 })
        .child(icon(glyph, px(20.0)).text_color(theme::text()));
    if enabled {
        b.cursor_pointer().hover(|s| s.bg(theme::hover()))
    } else {
        b
    }
}

impl Render for MiniPlayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let player = self.stores.player.read(cx);
        let current = player.current().cloned();
        let state = player.state;
        let has_prev = player.index.is_some();
        let has_next = player.index.is_some_and(|ix| ix + 1 < player.queue.len());
        let thumb = current
            .as_ref()
            .and_then(|t| self.stores.thumbs.read(cx).get(&t.id));
        let has_track = current.is_some();
        let subtitle = match (&current, state) {
            (_, PlayState::Loading) => "Loading…".to_string(),
            (Some(t), _) => t.channel.clone(),
            (None, _) => String::new(),
        };
        let play_glyph = if state == PlayState::Playing {
            Icon::Pause
        } else {
            Icon::Play
        };
        let play_enabled = has_track && state != PlayState::Loading;
        let stores = self.stores.clone();
        let volume = player.volume;
        let volume_bounds = self.volume_bounds.clone();
        let volume_dragging = self.volume_dragging.clone();

        let position = player.position;
        let duration = player.duration;
        let progress = match duration {
            Some(d) if !d.is_zero() => (position.as_secs_f32() / d.as_secs_f32()).clamp(0.0, 1.0),
            _ => 0.0,
        };
        let mut play = div()
            .id("mini-play")
            .flex()
            .items_center()
            .justify_center()
            .size(px(46.0))
            .rounded_full()
            .bg(theme::accent())
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0.008, 1.0, 0.635, 0.45),
                offset: point(px(0.0), px(2.0)),
                blur_radius: px(12.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .opacity(if play_enabled { 1.0 } else { 0.35 })
            .child(icon(play_glyph, px(26.0)).text_color(theme::on_accent()));
        if play_enabled {
            play = play
                .cursor_pointer()
                .hover(|s| s.opacity(0.9))
                .active(|s| s.opacity(0.75));
        }

        let title = current
            .as_ref()
            .map_or("Nothing playing".to_string(), |t| t.title.clone());

        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .size_full()
            .pt(px(16.0))
            .px(px(16.0))
            .pb(px(22.0))
            .bg(theme::bg_chrome())
            .border_1()
            .border_color(theme::border_subtle())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(logo(16.0))
                    .child(
                        div()
                            .flex()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(div().text_color(theme::text()).child("YT"))
                            .child(div().text_color(theme::accent()).child("Fast")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(10.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(if state == PlayState::Playing {
                                theme::accent()
                            } else {
                                theme::text_muted()
                            })
                            .child(match state {
                                PlayState::Playing => "NOW PLAYING",
                                PlayState::Loading => "LOADING",
                                _ => "PAUSED",
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.0))
                    .min_w_0()
                    .child(
                        div()
                            .flex_none()
                            .rounded(px(10.0))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::black().opacity(0.45),
                                offset: point(px(0.0), px(4.0)),
                                blur_radius: px(14.0),
                                spread_radius: px(0.0),
                                inset: false,
                            }])
                            .child(thumbnail(thumb, 88.0, 88.0)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .line_clamp(2)
                                    .text_size(px(15.0))
                                    .line_height(px(19.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme::text())
                                    .child(title),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(theme::text_muted())
                                    .child(subtitle),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_size(px(10.0))
                    .text_color(theme::text_muted())
                    .child(
                        div()
                            .w(px(30.0))
                            .child(format_duration(Some(position.as_secs()))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(4.0))
                            .rounded_full()
                            .bg(theme::bar_track())
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(progress))
                                    .rounded_full()
                                    .bg(theme::accent()),
                            ),
                    )
                    .child(
                        div()
                            .w(px(30.0))
                            .flex()
                            .justify_end()
                            .child(format_duration(duration.map(|d| d.as_secs()))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(18.0))
                    .child(side_button("mini-prev", Icon::Prev, has_prev).on_click({
                        let stores = stores.clone();
                        move |_, _, cx| {
                            if has_prev {
                                stores.player.update(cx, |p, cx| p.previous(cx));
                            }
                        }
                    }))
                    .child(play.on_click({
                        let stores = stores.clone();
                        move |_, _, cx| {
                            if play_enabled {
                                stores.player.update(cx, |p, cx| p.toggle(cx));
                            }
                        }
                    }))
                    .child(side_button("mini-next", Icon::Next, has_next).on_click({
                        let stores = stores.clone();
                        move |_, _, cx| {
                            if has_next {
                                stores.player.update(cx, |p, cx| p.next(cx));
                            }
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(icon(Icon::Volume, px(18.0)).text_color(theme::text_muted()))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .flex()
                            .child(volume_drag(
                                volume_bounds.clone(),
                                volume_dragging.clone(),
                                stores.player.clone(),
                            ))
                            .child(
                                bar("mini-volume", volume, volume_bounds.clone()).on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                                        volume_dragging.set(true);
                                        let f = fraction(volume_bounds.get(), ev.position);
                                        this.stores.player.update(cx, |p, cx| p.set_volume(f, cx));
                                    }),
                                ),
                            ),
                    ),
            )
    }
}
