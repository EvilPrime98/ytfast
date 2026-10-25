//! The player bar: current track, transport, seek bar, save and volume.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Bounds, Context, Entity, EventEmitter, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render,
    StatefulInteractiveElement, Styled, Window, canvas, div, px, relative,
};

use crate::model::format_duration;
use crate::route::{Navigate, Route};
use crate::state::Stores;
use crate::state::player::{PlayState, PlayerStore};
use crate::ui::components::track_row::thumbnail;
use crate::ui::icons::{Icon, icon};
use crate::ui::{theme, toast};

pub struct PlayerBar {
    stores: Stores,
    /// Where the seek / volume bars were laid out last frame, to turn a click into a fraction.
    seek_bounds: Rc<Cell<Bounds<Pixels>>>,
    volume_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// True from mouse-down on the volume bar until the button is released.
    volume_dragging: Rc<Cell<bool>>,
}

impl EventEmitter<Navigate> for PlayerBar {}

impl PlayerBar {
    pub fn new(stores: Stores, cx: &mut Context<Self>) -> Self {
        cx.observe(&stores.player, |this, player, cx| {
            // Fetch the cover of whatever just became current (never from `render`).
            if let Some(track) = player.read(cx).current() {
                let id = track.id.clone();
                this.stores.thumbs.update(cx, |t, cx| t.ensure(&id, cx));
            }
            cx.notify();
        })
        .detach();
        cx.observe(&stores.thumbs, |_, _, cx| cx.notify()).detach();
        cx.observe(&stores.library, |_, _, cx| cx.notify()).detach();
        Self {
            stores,
            seek_bounds: Rc::default(),
            volume_bounds: Rc::default(),
            volume_dragging: Rc::default(),
        }
    }
}

fn fraction(bounds: Bounds<Pixels>, position: Point<Pixels>) -> f32 {
    let width = f32::from(bounds.size.width);
    if width <= 0.0 {
        return 0.0;
    }
    (f32::from(position.x - bounds.origin.x) / width).clamp(0.0, 1.0)
}

/// While `dragging` is set, follows the mouse anywhere in the window (not just over the bar)
/// and writes the position under it to the volume; releasing the button ends the drag.
fn volume_drag(
    bounds: Rc<Cell<Bounds<Pixels>>>,
    dragging: Rc<Cell<bool>>,
    player: Entity<PlayerStore>,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            window.on_mouse_event({
                let dragging = dragging.clone();
                move |ev: &MouseMoveEvent, phase, _, cx| {
                    if !phase.bubble() || !dragging.get() {
                        return;
                    }
                    let f = fraction(bounds.get(), ev.position);
                    player.update(cx, |p, cx| p.set_volume(f, cx));
                }
            });
            window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, _| {
                if phase.bubble() && ev.button == MouseButton::Left {
                    dragging.set(false);
                }
            });
        },
    )
    .absolute()
    .size_full()
}

/// A clickable horizontal bar filled to `filled` (0..=1). `bounds` is refreshed every frame.
fn bar(
    id: &'static str,
    filled: f32,
    bounds: Rc<Cell<Bounds<Pixels>>>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .h(px(16.0))
        .flex_1()
        .cursor_pointer()
        .child(
            canvas(move |b, _, _| bounds.set(b), |_, _, _, _| ())
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .w_full()
                .h(px(4.0))
                .rounded_full()
                .bg(theme::bar_track())
                .child(
                    div()
                        .h_full()
                        .w(relative(filled.clamp(0.0, 1.0)))
                        .rounded_full()
                        .bg(theme::accent()),
                ),
        )
}

fn transport_button(id: &'static str, glyph: Icon, size: f32) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded_full()
        .cursor_pointer()
        .text_color(theme::text())
        .hover(|s| s.bg(theme::hover()))
        .child(icon(glyph, px(size * 0.6)).text_color(theme::text()))
}

impl Render for PlayerBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let player = self.stores.player.read(cx);
        let current = player.current().cloned();
        let state = player.state;
        let position = player.position;
        let duration = player.duration;
        let volume = player.volume;
        let has_prev = player.index.is_some();
        let has_next = player.index.is_some_and(|ix| ix + 1 < player.queue.len());
        let thumb = current
            .as_ref()
            .and_then(|t| self.stores.thumbs.read(cx).get(&t.id));
        let saved = current
            .as_ref()
            .is_some_and(|t| self.stores.library.read(cx).contains(&t.id));

        let progress = match duration {
            Some(d) if !d.is_zero() => position.as_secs_f32() / d.as_secs_f32(),
            _ => 0.0,
        };
        let subtitle = match (&current, state) {
            (_, PlayState::Loading) => "Loading…".to_string(),
            (Some(t), _) => t.channel.clone(),
            (None, _) => "Search for something to play".to_string(),
        };

        let play_glyph = if state == PlayState::Playing {
            Icon::Pause
        } else {
            Icon::Play
        };
        let dim = |on: bool| if on { 1.0 } else { 0.35 };

        let stores = self.stores.clone();
        let seek_bounds = self.seek_bounds.clone();
        let volume_bounds = self.volume_bounds.clone();
        let volume_dragging = self.volume_dragging.clone();

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(16.0))
            .h(theme::player_bar_height())
            .w_full()
            .px(px(16.0))
            .bg(theme::bg_chrome())
            .border_t_1()
            .border_color(theme::border_subtle())
            // Left: cover + title. Opens the queue.
            .child(
                div()
                    .id("bar-track")
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .w(px(300.0))
                    .flex_none()
                    .min_w_0()
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Navigate(Route::NowPlaying))))
                    .child(thumbnail(thumb, 80.0, 45.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme::text())
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
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(theme::text_muted())
                                    .child(subtitle),
                            ),
                    ),
            )
            // Centre: transport + seek bar.
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(
                                transport_button("bar-prev", Icon::Prev, 34.0)
                                    .opacity(dim(has_prev))
                                    .on_click({
                                        let stores = stores.clone();
                                        move |_, _, cx| {
                                            stores.player.update(cx, |p, cx| p.previous(cx))
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .id("bar-play")
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .size(px(42.0))
                                    .rounded_full()
                                    .bg(theme::text())
                                    .cursor_pointer()
                                    .opacity(if state == PlayState::Loading {
                                        0.6
                                    } else {
                                        1.0
                                    })
                                    .on_click({
                                        let stores = stores.clone();
                                        move |_, _, cx| {
                                            stores.player.update(cx, |p, cx| p.toggle(cx))
                                        }
                                    })
                                    .child(
                                        icon(play_glyph, px(26.0)).text_color(theme::bg_chrome()),
                                    ),
                            )
                            .child(
                                transport_button("bar-next", Icon::Next, 34.0)
                                    .opacity(dim(has_next))
                                    .on_click({
                                        let stores = stores.clone();
                                        move |_, _, cx| stores.player.update(cx, |p, cx| p.next(cx))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .w_full()
                            .max_w(px(640.0))
                            .text_size(px(11.0))
                            .text_color(theme::text_muted())
                            .child(
                                div()
                                    .w(px(40.0))
                                    .flex_none()
                                    .child(format_duration(Some(position.as_secs()))),
                            )
                            .child(
                                bar("bar-seek", progress, seek_bounds.clone()).on_mouse_down(
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
                                div().w(px(40.0)).flex_none().child(format_duration(
                                    duration.map(|d: Duration| d.as_secs()),
                                )),
                            ),
                    ),
            )
            // Right: save + volume.
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .gap(px(8.0))
                    .w(px(240.0))
                    .child(
                        div()
                            .id("bar-save")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(34.0))
                            .rounded_full()
                            .cursor_pointer()
                            .opacity(dim(current.is_some()))
                            .hover(|s| s.bg(theme::hover()))
                            .on_click({
                                let stores = stores.clone();
                                let current = current.clone();
                                move |_, _, cx| {
                                    let Some(track) = current.clone() else { return };
                                    let saved =
                                        stores.library.update(cx, |l, cx| l.toggle(&track, cx));
                                    toast::success(
                                        cx,
                                        if saved {
                                            "Saved to library"
                                        } else {
                                            "Removed from library"
                                        },
                                    );
                                }
                            })
                            .child(
                                icon(
                                    if saved {
                                        Icon::Heart
                                    } else {
                                        Icon::HeartOutline
                                    },
                                    px(20.0),
                                )
                                .text_color(if saved {
                                    theme::accent()
                                } else {
                                    theme::text_muted()
                                }),
                            ),
                    )
                    .child(icon(Icon::Volume, px(18.0)).text_color(theme::text_muted()))
                    .child(
                        div()
                            .relative()
                            .w(px(96.0))
                            .flex()
                            .child(volume_drag(
                                volume_bounds.clone(),
                                volume_dragging.clone(),
                                stores.player.clone(),
                            ))
                            .child(
                                bar("bar-volume", volume, volume_bounds.clone()).on_mouse_down(
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
