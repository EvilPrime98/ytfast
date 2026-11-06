use gpui::{
    Context, IntoElement, ParentElement, Render, ScrollWheelEvent, Styled, Subscription, Window,
    div, prelude::*, px,
};

use super::search::message;
use crate::api::ytdlp;
use crate::model::{Playlist, Track, shuffled};
use crate::runtime;
use crate::state::Stores;
use crate::ui::components::button::{ButtonVariant, button, icon_button};
use crate::ui::components::smooth_scroll::SmoothScroll;
use crate::ui::components::spinner::{spinner_fill, spinner_sized};
use crate::ui::components::video_card::{GRID_PAD, grid_metrics};
use crate::ui::components::video_grid::video_grid;
use crate::ui::icons::Icon;
use crate::ui::{theme, toast};

const PAGE: u32 = 30;
const ALL: u32 = 100_000;

pub struct PlaylistPage {
    stores: Stores,
    playlist: Playlist,
    owner: String,
    tracks: Vec<Track>,
    loading: bool,
    has_more: bool,
    pending: bool,
    error: Option<String>,
    next_start: u32,
    scroll: SmoothScroll,
    _subs: Vec<Subscription>,
}

impl PlaylistPage {
    pub fn new(stores: Stores, playlist: Playlist, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
            cx.observe(&stores.library, |_, _, cx| cx.notify()),
            cx.observe(&stores.playlists, |_, _, cx| cx.notify()),
            cx.observe(&stores.player, |_, _, cx| cx.notify()),
            cx.observe(&stores.prefs, |_, _, cx| cx.notify()),
        ];
        let mut this = Self {
            stores,
            playlist,
            owner: String::new(),
            tracks: Vec::new(),
            loading: false,
            has_more: false,
            pending: false,
            error: None,
            next_start: 1,
            scroll: SmoothScroll::new(),
            _subs: subs,
        };
        this.load_more(cx);
        this
    }

    fn on_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.scroll.on_wheel(ev) {
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn snapshot(&self) -> Playlist {
        Playlist {
            owner: self.owner.clone(),
            ..self.playlist.clone()
        }
    }

    fn merge(&mut self, tracks: Vec<Track>, cx: &mut Context<Self>) {
        let fresh: Vec<Track> = tracks
            .into_iter()
            .filter(|t| !self.tracks.iter().any(|k| k.id == t.id))
            .collect();
        self.stores.thumbs.update(cx, |t, cx| {
            for track in &fresh {
                t.ensure(&track.id, cx);
            }
        });
        self.tracks.extend(fresh);
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        cx.notify();
        let bin = self.stores.prefs.read(cx).prefs.ytdlp_path.clone();
        let id = self.playlist.id.clone();
        let (start, end) = (self.next_start, self.next_start + PAGE - 1);
        cx.spawn(async move |this, cx| {
            let result =
                runtime::run(async move { ytdlp::playlist_videos(&bin, &id, start, end).await })
                    .await;
            let failure = this
                .update(cx, |this, cx| {
                    this.loading = false;
                    let failure = match result {
                        Ok(page) => {
                            this.next_start = end + 1;
                            this.has_more = page.tracks.len() as u32 >= PAGE;
                            if !page.title.is_empty() {
                                this.playlist.title = page.title;
                            }
                            if !page.owner.is_empty() {
                                this.owner = page.owner;
                            }
                            let snapshot = this.snapshot();
                            this.stores
                                .playlists
                                .update(cx, |p, cx| p.refresh(&snapshot, cx));
                            this.merge(page.tracks, cx);
                            None
                        }
                        Err(e) => {
                            this.has_more = false;
                            this.error = Some(e.to_string());
                            Some(e.to_string())
                        }
                    };
                    cx.notify();
                    failure
                })
                .ok()
                .flatten();
            if let Some(e) = failure {
                tracing::warn!("loading the playlist failed: {e}");
                cx.update(|cx| toast::error(cx, e));
            }
        })
        .detach();
    }

    fn play_all(&mut self, shuffle: bool, cx: &mut Context<Self>) {
        if self.pending || self.tracks.is_empty() {
            return;
        }
        if !self.has_more {
            self.start(shuffle, cx);
            return;
        }
        if self.loading {
            return;
        }
        self.pending = true;
        self.loading = true;
        cx.notify();
        let bin = self.stores.prefs.read(cx).prefs.ytdlp_path.clone();
        let id = self.playlist.id.clone();
        let start = self.next_start;
        cx.spawn(async move |this, cx| {
            let result =
                runtime::run(async move { ytdlp::playlist_videos(&bin, &id, start, ALL).await })
                    .await;
            let failure = this
                .update(cx, |this, cx| {
                    this.loading = false;
                    this.pending = false;
                    let failure = match result {
                        Ok(page) => {
                            this.merge(page.tracks, cx);
                            this.has_more = false;
                            this.start(shuffle, cx);
                            None
                        }
                        Err(e) => Some(e.to_string()),
                    };
                    cx.notify();
                    failure
                })
                .ok()
                .flatten();
            if let Some(e) = failure {
                tracing::warn!("loading the full playlist failed: {e}");
                cx.update(|cx| toast::error(cx, e));
            }
        })
        .detach();
    }

    fn start(&self, shuffle: bool, cx: &mut Context<Self>) {
        let queue = if shuffle {
            shuffled(&self.tracks)
        } else {
            self.tracks.clone()
        };
        self.stores
            .player
            .update(cx, |p, cx| p.play_tracks(queue, 0, cx));
    }
}

impl Render for PlaylistPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.scroll.step(window);
        let collapsed = self.stores.prefs.read(cx).prefs.sidebar_collapsed;
        let (_, card_w) = grid_metrics(window, collapsed);

        let favorite = self
            .stores
            .playlists
            .read(cx)
            .is_favorite(&self.playlist.id);
        let title = if self.playlist.title.is_empty() {
            "Playlist".to_string()
        } else {
            self.playlist.title.clone()
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(16.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.0))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(22.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(title),
                    )
                    .when(!self.owner.is_empty(), |s| {
                        s.child(
                            div()
                                .truncate()
                                .text_size(px(13.0))
                                .text_color(theme::text_muted())
                                .child(self.owner.clone()),
                        )
                    }),
            )
            .child(button(
                "playlist-favorite",
                if favorite { "Favorited" } else { "Favorite" },
                if favorite {
                    ButtonVariant::Secondary
                } else {
                    ButtonVariant::Classic
                },
                cx.listener(|this, _, _, cx| {
                    let playlist = this.snapshot();
                    this.stores
                        .playlists
                        .update(cx, |p, cx| p.toggle(&playlist, cx));
                }),
            ))
            .when(!self.tracks.is_empty(), |s| {
                s.child(button(
                    "playlist-play-all",
                    "Play all",
                    ButtonVariant::Classic,
                    cx.listener(|this, _, _, cx| this.play_all(false, cx)),
                ))
                .child(if self.pending {
                    div()
                        .id("playlist-shuffle-busy")
                        .flex()
                        .items_center()
                        .justify_center()
                        .px(px(12.0))
                        .child(spinner_sized(18.0))
                        .into_any_element()
                } else {
                    icon_button(
                        "playlist-shuffle",
                        Icon::Shuffle,
                        "Shuffle",
                        ButtonVariant::Secondary,
                        cx.listener(|this, _, _, cx| this.play_all(true, cx)),
                    )
                    .into_any_element()
                })
            });

        let body = if self.tracks.is_empty() && self.loading {
            spinner_fill().into_any_element()
        } else if self.tracks.is_empty() {
            message(
                self.error
                    .clone()
                    .unwrap_or_else(|| "No videos found".to_string()),
            )
            .into_any_element()
        } else {
            video_grid(
                "playlist",
                &self.stores,
                &self.tracks,
                card_w,
                &self.scroll,
                self.has_more.then_some(self.loading),
                cx.listener(|this, ev, _, cx| this.on_wheel(ev, cx)),
                cx.listener(|this, _, _, cx| this.load_more(cx)),
                cx,
            )
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
