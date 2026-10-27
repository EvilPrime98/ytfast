use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, ScrollWheelEvent,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*, px,
};

use super::search::message;
use crate::api::{feeds, ytdlp};
use crate::model::{Channel, Track};
use crate::runtime;
use crate::state::Stores;
use crate::ui::components::button::{ButtonVariant, button};
use crate::ui::components::smooth_scroll::SmoothScroll;
use crate::ui::components::spinner::spinner_fill;
use crate::ui::components::track_row::RowAction;
use crate::ui::components::video_card::{GRID_GAP, GRID_PAD, avatar, grid_metrics, video_card};
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
    _subs: Vec<Subscription>,
}

impl ChannelPage {
    pub fn new(stores: Stores, channel: Channel, cx: &mut Context<Self>) -> Self {
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
            _subs: subs,
        };
        this.ensure_avatar(cx);
        this.load_more(cx);
        this
    }

    fn on_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.scroll.on_wheel(ev) {
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
}

impl Render for ChannelPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.scroll.step(window);
        let collapsed = self.stores.prefs.read(cx).prefs.sidebar_collapsed;
        let (_, card_w) = grid_metrics(window, collapsed);
        let playing_id = self.stores.player.read(cx).current().map(|t| t.id.clone());
        let (favorite, notify) = {
            let channels = self.stores.channels.read(cx);
            let fav = channels.get(&self.channel.id);
            (fav.is_some(), fav.is_some_and(|f| f.notify))
        };
        let avatar_image = self.stores.thumbs.read(cx).avatar(&self.channel.id);

        let cards: Vec<_> = self
            .tracks
            .iter()
            .enumerate()
            .map(|(ix, track)| {
                let thumb = self.stores.thumbs.read(cx).get(&track.id);
                let saved = self.stores.library.read(cx).contains(&track.id);
                let (stores, all) = (self.stores.clone(), self.tracks.clone());
                let (queue_track, save_track) = (track.clone(), track.clone());
                let (queue_stores, save_stores) = (self.stores.clone(), self.stores.clone());
                video_card(
                    "channel",
                    ix,
                    card_w,
                    track,
                    thumb,
                    playing_id.as_deref() == Some(track.id.as_str()),
                    move |_, cx| {
                        stores
                            .player
                            .update(cx, |p, cx| p.play_tracks(all.clone(), ix, cx))
                    },
                    vec![
                        RowAction::new(Icon::Plus, move |_, cx| {
                            queue_stores
                                .player
                                .update(cx, |p, cx| p.enqueue(queue_track.clone(), cx));
                            toast::success(cx, "Added to queue");
                        }),
                        RowAction::new(
                            if saved {
                                Icon::Heart
                            } else {
                                Icon::HeartOutline
                            },
                            move |_, cx| {
                                save_stores
                                    .library
                                    .update(cx, |l, cx| l.toggle(&save_track, cx));
                            },
                        )
                        .active(saved),
                    ],
                )
            })
            .collect();

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
            .when(!self.tracks.is_empty(), |s| {
                s.child(button(
                    "channel-play-all",
                    "Play all",
                    ButtonVariant::Classic,
                    play_all,
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
            div()
                .id("channel-videos")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.scroll.handle)
                .on_scroll_wheel(cx.listener(|this, ev, _, cx| this.on_wheel(ev, cx)))
                .gap(px(16.0))
                .child(div().flex().flex_wrap().gap(px(GRID_GAP)).children(cards))
                .when(self.has_more, |s| {
                    s.child(div().flex().justify_center().pb(px(8.0)).child(button(
                        "channel-more",
                        if self.loading {
                            "Loading…"
                        } else {
                            "Load more"
                        },
                        ButtonVariant::Secondary,
                        cx.listener(|this, _, _, cx| this.load_more(cx)),
                    )))
                })
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
