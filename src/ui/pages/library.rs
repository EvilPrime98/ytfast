//! Library: the tracks the user saved with the heart.

use std::ops::Range;

use gpui::{
    Context, IntoElement, ParentElement, Render, Styled, Subscription, Window, div, prelude::*, px,
    uniform_list,
};

use crate::model::shuffled;
use crate::state::Stores;
use crate::ui::components::button::{ButtonVariant, button, icon_button};
use crate::ui::components::track_row::{RowAction, track_row};
use crate::ui::icons::{Icon, icon};
use crate::ui::{theme, toast};

pub struct LibraryPage {
    stores: Stores,
    _subs: Vec<Subscription>,
}

impl LibraryPage {
    pub fn new(stores: Stores, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe(&stores.library, |this, library, cx| {
                let ids: Vec<String> = library
                    .read(cx)
                    .tracks
                    .iter()
                    .map(|t| t.id.clone())
                    .collect();
                this.stores.thumbs.update(cx, |t, cx| {
                    for id in &ids {
                        t.ensure(id, cx);
                    }
                });
                cx.notify();
            }),
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
            cx.observe(&stores.player, |_, _, cx| cx.notify()),
        ];
        let ids: Vec<String> = stores
            .library
            .read(cx)
            .tracks
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

impl Render for LibraryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tracks = self.stores.library.read(cx).tracks.clone();
        let stores = self.stores.clone();
        let all = tracks.clone();
        let shuffle_stores = stores.clone();
        let shuffle_all = tracks.clone();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .px(px(24.0))
            .py(px(16.0))
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(22.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Library"),
                    )
                    .when(!tracks.is_empty(), |s| {
                        s.child(
                            div()
                                .flex()
                                .gap(px(8.0))
                                .child(icon_button(
                                    "lib-shuffle",
                                    Icon::Shuffle,
                                    "Shuffle",
                                    ButtonVariant::Secondary,
                                    move |_, _, cx| {
                                        shuffle_stores.player.update(cx, |p, cx| {
                                            p.play_tracks(shuffled(&shuffle_all), 0, cx)
                                        })
                                    },
                                ))
                                .child(button(
                                    "lib-play-all",
                                    "Play all",
                                    ButtonVariant::Classic,
                                    move |_, _, cx| {
                                        stores
                                            .player
                                            .update(cx, |p, cx| p.play_tracks(all.clone(), 0, cx))
                                    },
                                )),
                        )
                    }),
            )
            .child(if tracks.is_empty() {
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(10.0))
                    .child(icon(Icon::HeartOutline, px(56.0)).text_color(theme::bar_track()))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .text_color(theme::text_muted())
                            .child("Tap the heart on a track to keep it here."),
                    )
                    .into_any_element()
            } else {
                uniform_list(
                    "library-list",
                    tracks.len(),
                    cx.processor(|this, range: Range<usize>, _window, cx| {
                        let tracks = this.stores.library.read(cx).tracks.clone();
                        let playing_id =
                            this.stores.player.read(cx).current().map(|t| t.id.clone());
                        range
                            .filter_map(|ix| {
                                let track = tracks.get(ix)?;
                                let thumb = this.stores.thumbs.read(cx).get(&track.id);
                                let play_stores = this.stores.clone();
                                let all = tracks.clone();
                                let queue_stores = this.stores.clone();
                                let queue_track = track.clone();
                                let remove_stores = this.stores.clone();
                                let remove_track = track.clone();
                                let row = track_row(
                                    "library",
                                    ix,
                                    track,
                                    thumb,
                                    playing_id.as_deref() == Some(track.id.as_str()),
                                    move |_, cx| {
                                        play_stores
                                            .player
                                            .update(cx, |p, cx| p.play_tracks(all.clone(), ix, cx))
                                    },
                                    vec![
                                        RowAction::new(Icon::Plus, move |_, cx| {
                                            queue_stores.player.update(cx, |p, cx| {
                                                p.enqueue(queue_track.clone(), cx)
                                            });
                                            toast::success(cx, "Added to queue");
                                        }),
                                        RowAction::new(Icon::Trash, move |_, cx| {
                                            remove_stores
                                                .library
                                                .update(cx, |l, cx| l.toggle(&remove_track, cx));
                                        }),
                                    ],
                                );
                                Some(div().pb(px(2.0)).child(row))
                            })
                            .collect()
                    }),
                )
                .flex_1()
                .min_h_0()
                .into_any_element()
            })
    }
}
