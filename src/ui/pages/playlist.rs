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
use crate::ui::components::spinner::spinner_fill;
use crate::ui::components::video_card::{GRID_PAD, grid_metrics};
use crate::ui::components::video_grid::video_grid;
use crate::ui::icons::Icon;
use crate::ui::{theme, toast};

const PAGE: u32 = 30;

pub struct PlaylistPage {
    stores: Stores,
    playlist: Playlist,
    owner: String,
    tracks: Vec<Track>,
    loading: bool,
    has_more: bool,
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
                            let fresh: Vec<Track> = page
                                .tracks
                                .into_iter()
                                .filter(|t| !this.tracks.iter().any(|k| k.id == t.id))
                                .collect();
                            this.stores.thumbs.update(cx, |t, cx| {
                                for track in &fresh {
                                    t.ensure(&track.id, cx);
                                }
                            });
                            this.tracks.extend(fresh);
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
}

impl Render for PlaylistPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.scroll.step(window);
        let collapsed = self.stores.prefs.read(cx).prefs.sidebar_collapsed;
        let (_, card_w) = grid_metrics(window, collapsed);

        let play_all = {
            let (stores, all) = (self.stores.clone(), self.tracks.clone());
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                stores
                    .player
                    .update(cx, |p, cx| p.play_tracks(all.clone(), 0, cx))
            }
        };

        let shuffle = {
            let (stores, all) = (self.stores.clone(), self.tracks.clone());
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                stores
                    .player
                    .update(cx, |p, cx| p.play_tracks(shuffled(&all), 0, cx))
            }
        };

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
                    play_all,
                ))
                .child(icon_button(
                    "playlist-shuffle",
                    Icon::Shuffle,
                    "Shuffle",
                    ButtonVariant::Secondary,
                    shuffle,
                ))
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
