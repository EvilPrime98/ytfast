use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render,
    ScrollWheelEvent, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
    prelude::*, px,
};

use super::search::message;
use crate::api::{feeds, ytdlp};
use crate::model::{Channel, Playlist, Track};
use crate::route::{ChannelTab, Navigate, ReplaceRoute, Route};
use crate::runtime;
use crate::state::Stores;
use crate::ui::components::button::{ButtonVariant, button};
use crate::ui::components::playlist_card::playlist_card;
use crate::ui::components::smooth_scroll::SmoothScroll;
use crate::ui::components::spinner::spinner_fill;
use crate::ui::components::video_card::{GRID_GAP, GRID_PAD, avatar, grid_metrics};
use crate::ui::components::video_grid::video_grid;
use crate::ui::icons::{Icon, icon};
use crate::ui::{theme, toast};

const PAGE: u32 = 30;

pub struct ChannelPage {
    stores: Stores,
    channel: Channel,
    tracks: Vec<Track>,
    loading: bool,
    has_more: bool,
    error: Option<String>,
    next_start: u32,
    scroll: SmoothScroll,
    tab: ChannelTab,
    playlists: Vec<Playlist>,
    playlists_started: bool,
    playlists_loading: bool,
    playlists_has_more: bool,
    playlists_error: Option<String>,
    playlists_next_start: u32,
    playlists_scroll: SmoothScroll,
    _subs: Vec<Subscription>,
}

impl EventEmitter<Navigate> for ChannelPage {}
impl EventEmitter<ReplaceRoute> for ChannelPage {}

impl ChannelPage {
    pub fn new(stores: Stores, channel: Channel, tab: ChannelTab, cx: &mut Context<Self>) -> Self {
        stores
            .channels
            .update(cx, |c, cx| c.mark_channel_seen(&channel.id, cx));
        let subs = vec![
            cx.observe(&stores.channels, |_, _, cx| cx.notify()),
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
            cx.observe(&stores.library, |_, _, cx| cx.notify()),
            cx.observe(&stores.player, |_, _, cx| cx.notify()),
            cx.observe(&stores.prefs, |_, _, cx| cx.notify()),
        ];
        let mut this = Self {
            stores,
            channel,
            tracks: Vec::new(),
            loading: false,
            has_more: false,
            error: None,
            next_start: 1,
            scroll: SmoothScroll::new(),
            tab,
            playlists: Vec::new(),
            playlists_started: false,
            playlists_loading: false,
            playlists_has_more: false,
            playlists_error: None,
            playlists_next_start: 1,
            playlists_scroll: SmoothScroll::new(),
            _subs: subs,
        };
        this.ensure_avatar(cx);
        this.load_more(cx);
        if tab == ChannelTab::Playlists {
            this.open_playlists(cx);
        }
        this
    }

    fn set_tab(&mut self, tab: ChannelTab, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        if tab == ChannelTab::Playlists {
            self.open_playlists(cx);
        }
        cx.emit(ReplaceRoute(Route::Channel(self.channel.clone(), tab)));
        cx.notify();
    }

    fn open_playlists(&mut self, cx: &mut Context<Self>) {
        if !self.playlists_started {
            self.playlists_started = true;
            self.load_more_playlists(cx);
        }
    }

    fn on_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let scroll = match self.tab {
            ChannelTab::Videos => &mut self.scroll,
            ChannelTab::Playlists => &mut self.playlists_scroll,
        };
        if scroll.on_wheel(ev) {
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn ensure_avatar(&self, cx: &mut Context<Self>) {
        let c = self.channel.clone();
        self.stores
            .thumbs
            .update(cx, |t, cx| t.ensure_avatar(&c.id, &c.avatar_url, cx));
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        cx.notify();
        let bin = self.stores.prefs.read(cx).prefs.ytdlp_path.clone();
        let id = self.channel.id.clone();
        let (start, end) = (self.next_start, self.next_start + PAGE - 1);
        let first_page = start == 1;
        cx.spawn(async move |this, cx| {
            let (result, feed) = runtime::run(async move {
                let feed = async {
                    if !first_page {
                        return Vec::new();
                    }
                    feeds::latest(&id).await.unwrap_or_else(|e| {
                        tracing::warn!("loading the channel feed failed: {e}");
                        Vec::new()
                    })
                };
                tokio::join!(ytdlp::channel_videos(&bin, &id, start, end), feed)
            })
            .await;
            let failure = this
                .update(cx, |this, cx| {
                    this.loading = false;
                    let failure = match result {
                        Ok(mut page) => {
                            for t in &mut page.tracks {
                                if t.upload_day.is_none() {
                                    t.upload_day = feed
                                        .iter()
                                        .find(|f| f.id == t.id)
                                        .and_then(|f| f.upload_day);
                                }
                            }
                            this.next_start = end + 1;
                            this.has_more = page.tracks.len() as u32 >= PAGE;
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
                            let known = Channel {
                                id: this.channel.id.clone(),
                                name: page.name,
                                avatar_url: page.avatar_url,
                            };
                            if this.channel.name.is_empty() && !known.name.is_empty() {
                                this.channel.name = known.name.clone();
                            }
                            if this.channel.avatar_url.is_empty() {
                                this.channel.avatar_url = known.avatar_url.clone();
                                this.ensure_avatar(cx);
                            }
                            this.stores
                                .channels
                                .update(cx, |c, cx| c.update_channel(&known, cx));
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
                tracing::warn!("loading the channel failed: {e}");
                cx.update(|cx| toast::error(cx, e));
            }
        })
        .detach();
    }

    fn load_more_playlists(&mut self, cx: &mut Context<Self>) {
        if self.playlists_loading {
            return;
        }
        self.playlists_loading = true;
        self.playlists_error = None;
        cx.notify();
        let bin = self.stores.prefs.read(cx).prefs.ytdlp_path.clone();
        let id = self.channel.id.clone();
        let (start, end) = (
            self.playlists_next_start,
            self.playlists_next_start + PAGE - 1,
        );
        cx.spawn(async move |this, cx| {
            let result =
                runtime::run(async move { ytdlp::channel_playlists(&bin, &id, start, end).await })
                    .await;
            let failure = this
                .update(cx, |this, cx| {
                    this.playlists_loading = false;
                    let failure = match result {
                        Ok(page) => {
                            this.playlists_next_start = end + 1;
                            this.playlists_has_more = page.len() as u32 >= PAGE;
                            let fresh: Vec<Playlist> = page
                                .into_iter()
                                .filter(|p| !this.playlists.iter().any(|k| k.id == p.id))
                                .collect();
                            this.stores.thumbs.update(cx, |t, cx| {
                                for playlist in fresh.iter().filter(|p| !p.thumb_id.is_empty()) {
                                    t.ensure(&playlist.thumb_id, cx);
                                }
                            });
                            this.playlists.extend(fresh);
                            None
                        }
                        Err(e) => {
                            this.playlists_has_more = false;
                            this.playlists_error = Some(e.to_string());
                            Some(e.to_string())
                        }
                    };
                    cx.notify();
                    failure
                })
                .ok()
                .flatten();
            if let Some(e) = failure {
                tracing::warn!("loading the channel playlists failed: {e}");
                cx.update(|cx| toast::error(cx, e));
            }
        })
        .detach();
    }

    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |id: &'static str, label: &'static str, which: ChannelTab| {
            let active = self.tab == which;
            div()
                .id(SharedString::from(id))
                .px(px(16.0))
                .py(px(8.0))
                .cursor_pointer()
                .text_size(px(14.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .border_b_2()
                .border_color(gpui::transparent_black())
                .when(active, |s| s.border_color(theme::accent()))
                .text_color(if active {
                    theme::text()
                } else {
                    theme::text_muted()
                })
                .hover(|s| s.text_color(theme::text()))
                .on_click(cx.listener(move |this, _, _, cx| this.set_tab(which, cx)))
                .child(label)
        };
        div()
            .flex()
            .gap(px(4.0))
            .border_b_1()
            .border_color(theme::border_subtle())
            .child(tab("channel-tab-videos", "Videos", ChannelTab::Videos))
            .child(tab(
                "channel-tab-playlists",
                "Playlists",
                ChannelTab::Playlists,
            ))
    }

    fn videos_body(&self, card_w: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.tracks.is_empty() && self.loading {
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
                "channel",
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
        }
    }

    fn playlists_body(&self, card_w: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.playlists.is_empty() && self.playlists_loading {
            return spinner_fill().into_any_element();
        }
        if self.playlists.is_empty() {
            return message(
                self.playlists_error
                    .clone()
                    .unwrap_or_else(|| "No playlists".to_string()),
            )
            .into_any_element();
        }
        let cards: Vec<_> = self
            .playlists
            .iter()
            .enumerate()
            .map(|(ix, playlist)| {
                let thumb = self.stores.thumbs.read(cx).get(&playlist.thumb_id);
                let open = playlist.clone();
                playlist_card(
                    ix,
                    card_w,
                    playlist,
                    thumb,
                    cx.listener(move |_, _, _, cx| {
                        cx.emit(Navigate(Route::Playlist(open.clone())))
                    }),
                )
            })
            .collect();
        div()
            .id("channel-playlists")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.playlists_scroll.handle)
            .on_scroll_wheel(cx.listener(|this, ev, _, cx| this.on_wheel(ev, cx)))
            .gap(px(16.0))
            .child(div().flex().flex_wrap().gap(px(GRID_GAP)).children(cards))
            .when(self.playlists_has_more, |s| {
                s.child(div().flex().justify_center().pb(px(8.0)).child(button(
                    "channel-playlists-more",
                    if self.playlists_loading {
                        "Loading…"
                    } else {
                        "Load more"
                    },
                    ButtonVariant::Secondary,
                    cx.listener(|this, _, _, cx| this.load_more_playlists(cx)),
                )))
            })
            .into_any_element()
    }
}

impl Render for ChannelPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.scroll.step(window);
        self.playlists_scroll.step(window);
        let collapsed = self.stores.prefs.read(cx).prefs.sidebar_collapsed;
        let (_, card_w) = grid_metrics(window, collapsed);
        let (favorite, notify) = {
            let channels = self.stores.channels.read(cx);
            let fav = channels.get(&self.channel.id);
            (fav.is_some(), fav.is_some_and(|f| f.notify))
        };
        let avatar_image = self.stores.thumbs.read(cx).avatar(&self.channel.id);

        let play_all = {
            let (stores, all) = (self.stores.clone(), self.tracks.clone());
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                stores
                    .player
                    .update(cx, |p, cx| p.play_tracks(all.clone(), 0, cx))
            }
        };

        let name = if self.channel.name.is_empty() {
            "Channel".to_string()
        } else {
            self.channel.name.clone()
        };

        let header = div()
            .flex()
            .items_center()
            .gap(px(16.0))
            .child(avatar(avatar_image, 72.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(22.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(name),
            )
            .when(favorite, |s| {
                s.child(
                    div()
                        .id("channel-bell")
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(34.0))
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = this.channel.id.clone();
                            this.stores
                                .channels
                                .update(cx, |c, cx| c.set_notify(&id, !notify, cx));
                            toast::success(
                                cx,
                                if notify {
                                    "Notifications off for this channel"
                                } else {
                                    "Notifications on for this channel"
                                },
                            );
                        }))
                        .child(icon(Icon::Bell, px(20.0)).text_color(if notify {
                            theme::accent()
                        } else {
                            theme::text_muted()
                        })),
                )
            })
            .child(button(
                "channel-favorite",
                if favorite { "Favorited" } else { "Favorite" },
                if favorite {
                    ButtonVariant::Secondary
                } else {
                    ButtonVariant::Classic
                },
                cx.listener(|this, _, _, cx| {
                    let channel = this.channel.clone();
                    this.stores
                        .channels
                        .update(cx, |c, cx| c.toggle(&channel, cx));
                }),
            ))
            .when(
                self.tab == ChannelTab::Videos && !self.tracks.is_empty(),
                |s| {
                    s.child(button(
                        "channel-play-all",
                        "Play all",
                        ButtonVariant::Classic,
                        play_all,
                    ))
                },
            );

        let body = match self.tab {
            ChannelTab::Videos => self.videos_body(card_w, cx),
            ChannelTab::Playlists => self.playlists_body(card_w, cx),
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
            .child(self.tab_bar(cx))
            .child(body)
    }
}
