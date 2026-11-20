use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render,
    ScrollWheelEvent, StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*,
    px,
};

use futures_util::StreamExt;

use super::playlist::ALL;
use super::search::message;
use crate::api::ytdlp;
use crate::model::{Track, shuffled};
use crate::route::{Navigate, Route};
use crate::runtime;
use crate::state::Stores;
use crate::ui::components::button::{ButtonVariant, icon_button};
use crate::ui::components::playlist_card::playlist_card;
use crate::ui::components::smooth_scroll::SmoothScroll;
use crate::ui::components::spinner::spinner_sized;
use crate::ui::components::video_card::{GRID_GAP, GRID_PAD, grid_metrics};
use crate::ui::icons::Icon;
use crate::ui::toast;

pub struct PlaylistsPage {
    stores: Stores,
    scroll: SmoothScroll,
    pending: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<Navigate> for PlaylistsPage {}

impl PlaylistsPage {
    pub fn new(stores: Stores, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe(&stores.playlists, |this, _, cx| {
                this.ensure_thumbs(cx);
                cx.notify();
            }),
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
            cx.observe(&stores.prefs, |_, _, cx| cx.notify()),
        ];
        let this = Self {
            stores,
            scroll: SmoothScroll::new(),
            pending: false,
            _subs: subs,
        };
        this.ensure_thumbs(cx);
        this
    }

    fn ensure_thumbs(&self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .stores
            .playlists
            .read(cx)
            .playlists
            .iter()
            .map(|p| p.thumb_id.clone())
            .collect();
        self.stores.thumbs.update(cx, |t, cx| {
            for id in ids.iter().filter(|id| !id.is_empty()) {
                t.ensure(id, cx);
            }
        });
    }

    fn shuffle_all(&mut self, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let ids: Vec<String> = self
            .stores
            .playlists
            .read(cx)
            .playlists
            .iter()
            .map(|p| p.id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        self.pending = true;
        cx.notify();
        let bin = self.stores.prefs.read(cx).prefs.ytdlp_path.clone();
        cx.spawn(async move |this, cx| {
            let results = runtime::run(async move {
                futures_util::stream::iter(ids)
                    .map(|id| {
                        let bin = bin.clone();
                        async move { ytdlp::playlist_videos(&bin, &id, 1, ALL).await }
                    })
                    .buffer_unordered(4)
                    .collect::<Vec<_>>()
                    .await
            })
            .await;
            let mut failed = 0usize;
            let mut tracks: Vec<Track> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for result in results {
                match result {
                    Ok(page) => {
                        for t in page.tracks {
                            if seen.insert(t.id.clone()) {
                                tracks.push(t);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("loading a saved playlist failed: {e}");
                        failed += 1;
                    }
                }
            }
            let error = if failed > 0 {
                Some(format!("{failed} playlists could not be loaded"))
            } else if tracks.is_empty() {
                Some("No videos found in the saved playlists".to_string())
            } else {
                None
            };
            this.update(cx, |this, cx| {
                this.pending = false;
                if !tracks.is_empty() {
                    let queue = shuffled(&tracks);
                    this.stores
                        .player
                        .update(cx, |p, cx| p.play_tracks(queue, 0, cx));
                }
                cx.notify();
            })
            .ok();
            if let Some(m) = error {
                cx.update(|cx| toast::error(cx, m));
            }
        })
        .detach();
    }

    fn on_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.scroll.on_wheel(ev) {
            cx.stop_propagation();
            cx.notify();
        }
    }
}

impl Render for PlaylistsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.scroll.step(window);
        let collapsed = self.stores.prefs.read(cx).prefs.sidebar_collapsed;
        let (_, card_w) = grid_metrics(window, collapsed);
        let playlists = self.stores.playlists.read(cx).playlists.clone();

        let header = div()
            .flex()
            .items_center()
            .gap(px(16.0))
            .child(
                div()
                    .text_size(px(22.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Playlists"),
            )
            .when(!playlists.is_empty(), |s| {
                s.child(if self.pending {
                    div()
                        .id("playlists-shuffle-busy")
                        .flex()
                        .items_center()
                        .justify_center()
                        .px(px(12.0))
                        .child(spinner_sized(18.0))
                        .into_any_element()
                } else {
                    icon_button(
                        "playlists-shuffle",
                        Icon::Shuffle,
                        "Shuffle",
                        ButtonVariant::Secondary,
                        cx.listener(|this, _, _, cx| this.shuffle_all(cx)),
                    )
                    .into_any_element()
                })
            });

        let body = if playlists.is_empty() {
            message("No favorite playlists yet. Open a playlist and favorite it to see it here.")
                .into_any_element()
        } else {
            let cards: Vec<_> = playlists
                .iter()
                .enumerate()
                .map(|(ix, playlist)| {
                    let thumb = self.stores.thumbs.read(cx).get(&playlist.thumb_id);
                    let (open, toggled) = (playlist.clone(), playlist.clone());
                    playlist_card(
                        ix,
                        card_w,
                        playlist,
                        thumb,
                        true,
                        cx.listener(move |_, _, _, cx| {
                            cx.emit(Navigate(Route::Playlist(open.clone())))
                        }),
                        cx.listener(move |this, _, _, cx| {
                            this.stores
                                .playlists
                                .update(cx, |p, cx| p.toggle(&toggled, cx));
                        }),
                    )
                })
                .collect();
            div()
                .id("favorite-playlists")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.scroll.handle)
                .on_scroll_wheel(cx.listener(|this, ev, _, cx| this.on_wheel(ev, cx)))
                .child(div().flex().flex_wrap().gap(px(GRID_GAP)).children(cards))
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .px(px(GRID_PAD))
            .py(px(16.0))
            .gap(px(16.0))
            .child(header)
            .child(body)
    }
}
