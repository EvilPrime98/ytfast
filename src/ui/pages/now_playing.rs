//! Now playing: the current track large, and the queue.

use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};

use crate::route::{Navigate, Route};
use crate::state::Stores;
use crate::ui::components::button::{ButtonVariant, button};
use crate::ui::components::track_row::{RowAction, thumbnail, track_row};
use crate::ui::confirm::{self, ConfirmOptions};
use crate::ui::icons::{Icon, icon};
use crate::ui::theme;

pub struct NowPlayingPage {
    stores: Stores,
    _subs: Vec<Subscription>,
}

impl EventEmitter<Navigate> for NowPlayingPage {}

impl NowPlayingPage {
    pub fn new(stores: Stores, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe(&stores.player, |this, player, cx| {
                let ids: Vec<String> = player.read(cx).queue.iter().map(|t| t.id.clone()).collect();
                this.stores.thumbs.update(cx, |t, cx| {
                    for id in &ids {
                        t.ensure(id, cx);
                    }
                });
                cx.notify();
            }),
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
        ];
        // Covers for a queue that already exists when the page opens.
        let ids: Vec<String> = stores
            .player
            .read(cx)
            .queue
            .iter()
            .map(|t| t.id.clone())
            .collect();
        stores.thumbs.update(cx, |t, cx| {
            for id in &ids {
                t.ensure(id, cx);
            }
        });
        Self {
            stores,
            _subs: subs,
        }
    }
}

impl Render for NowPlayingPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let player = self.stores.player.read(cx);
        let queue = player.queue.clone();
        let current_ix = player.index;
        let current = player.current().cloned();

        if queue.is_empty() {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(12.0))
                .child(icon(Icon::Queue, px(56.0)).text_color(theme::bar_track()))
                .child(
                    div()
                        .text_size(px(18.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("The queue is empty"),
                )
                .child(button(
                    "np-find",
                    "Find music",
                    ButtonVariant::Classic,
                    cx.listener(|_, _, _, cx| cx.emit(Navigate(Route::Search))),
                ))
                .into_any_element();
        }

        let rows: Vec<_> = queue
            .iter()
            .enumerate()
            .map(|(ix, track)| {
                let thumb = self.stores.thumbs.read(cx).get(&track.id);
                let play_stores = self.stores.clone();
                let remove_stores = self.stores.clone();
                track_row(
                    "queue",
                    ix,
                    track,
                    thumb,
                    current_ix == Some(ix),
                    move |_, cx| play_stores.player.update(cx, |p, cx| p.play_index(ix, cx)),
                    vec![RowAction::new(Icon::Close, move |_, cx| {
                        remove_stores.player.update(cx, |p, cx| p.remove(ix, cx))
                    })],
                )
            })
            .collect();

        let art = current
            .as_ref()
            .and_then(|t| self.stores.thumbs.read(cx).get(&t.id));
        let stores = self.stores.clone();

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
                    .flex_none()
                    .w(px(360.0))
                    .gap(px(12.0))
                    .child(thumbnail(art, 360.0, 202.0))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(
                                current
                                    .as_ref()
                                    .map_or("Nothing playing".to_string(), |t| t.title.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(theme::text_muted())
                            .child(current.map(|t| t.channel).unwrap_or_default()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
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
                                "np-clear",
                                "Clear queue",
                                ButtonVariant::Secondary,
                                move |_, _, cx| {
                                    let stores = stores.clone();
                                    confirm::ask(
                                        cx,
                                        ConfirmOptions::new(
                                            "Clear the queue?",
                                            "Playback stops and every queued track is removed.",
                                        )
                                        .labels("Clear", "Cancel"),
                                        move |answer, _, cx| {
                                            if answer == Some(true) {
                                                stores.player.update(cx, |p, cx| p.clear(cx));
                                            }
                                        },
                                    );
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("queue-list")
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
