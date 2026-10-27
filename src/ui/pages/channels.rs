use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*, px,
};

use crate::route::{ChannelTab, Navigate, Route};
use crate::state::Stores;
use crate::ui::components::button::{ButtonVariant, button};
use crate::ui::components::track_row::RowAction;
use crate::ui::components::video_card::{GRID_GAP, GRID_PAD, avatar, grid_metrics, video_card};
use crate::ui::icons::{Icon, icon};
use crate::ui::{theme, toast};

const NEW_LIMIT: usize = 24;
const TILE_W: f32 = 180.0;

pub struct ChannelsPage {
    stores: Stores,
    _subs: Vec<Subscription>,
}

impl EventEmitter<Navigate> for ChannelsPage {}

impl ChannelsPage {
    pub fn new(stores: Stores, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe(&stores.channels, |this, _, cx| {
                this.ensure_assets(cx);
                cx.notify();
            }),
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
            cx.observe(&stores.library, |_, _, cx| cx.notify()),
            cx.observe(&stores.player, |_, _, cx| cx.notify()),
            cx.observe(&stores.prefs, |_, _, cx| cx.notify()),
        ];
        let this = Self {
            stores,
            _subs: subs,
        };
        this.ensure_assets(cx);
        this
    }

    fn ensure_assets(&self, cx: &mut Context<Self>) {
        let channels = self.stores.channels.read(cx);
        let avatars: Vec<_> = channels
            .favorites
            .iter()
            .map(|f| (f.channel.id.clone(), f.channel.avatar_url.clone()))
            .collect();
        let ids: Vec<String> = channels
            .unseen_videos()
            .into_iter()
            .take(NEW_LIMIT)
            .map(|t| t.id)
            .collect();
        self.stores.thumbs.update(cx, |t, cx| {
            for (id, url) in &avatars {
                t.ensure_avatar(id, url, cx);
            }
            for id in &ids {
                t.ensure(id, cx);
            }
        });
    }
}

impl Render for ChannelsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.stores.prefs.read(cx).prefs.sidebar_collapsed;
        let (_, card_w) = grid_metrics(window, collapsed);
        let playing_id = self.stores.player.read(cx).current().map(|t| t.id.clone());
        let (favorites, new_videos, checking) = {
            let c = self.stores.channels.read(cx);
            (
                c.favorites.clone(),
                c.unseen_videos()
                    .into_iter()
                    .take(NEW_LIMIT)
                    .collect::<Vec<_>>(),
                c.checking,
            )
        };

        let cards: Vec<_> = new_videos
            .iter()
            .enumerate()
            .map(|(ix, track)| {
                let thumb = self.stores.thumbs.read(cx).get(&track.id);
                let saved = self.stores.library.read(cx).contains(&track.id);
                let (stores, all) = (self.stores.clone(), new_videos.clone());
                let (queue_track, save_track) = (track.clone(), track.clone());
                let (queue_stores, save_stores) = (self.stores.clone(), self.stores.clone());
                video_card(
                    "new",
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

        let tiles: Vec<_> = favorites
            .iter()
            .enumerate()
            .map(|(ix, fav)| {
                let image = self.stores.thumbs.read(cx).avatar(&fav.channel.id);
                let open = fav.channel.clone();
                let id = fav.channel.id.clone();
                let notify = fav.notify;
                let unseen = fav.unseen.len();
                let status = if fav.unavailable() {
                    Some("Unavailable".to_string())
                } else if unseen > 0 {
                    Some(format!("{unseen} new"))
                } else {
                    None
                };
                let unavailable = fav.unavailable();
                div()
                    .id(SharedString::from(format!("fav-tile-{ix}")))
                    .flex()
                    .flex_col()
                    .flex_none()
                    .items_center()
                    .w(px(TILE_W))
                    .gap(px(8.0))
                    .p(px(10.0))
                    .rounded(px(10.0))
                    .hover(|s| s.bg(theme::bg_row_hover()))
                    .child(
                        div()
                            .id(SharedString::from(format!("fav-open-{ix}")))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.0))
                            .w_full()
                            .cursor_pointer()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(Navigate(Route::Channel(
                                    open.clone(),
                                    ChannelTab::default(),
                                )))
                            }))
                            .child(avatar(image, 88.0))
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_center()
                                    .text_size(px(14.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(fav.channel.name.clone()),
                            )
                            .child(
                                div()
                                    .h(px(16.0))
                                    .text_size(px(12.0))
                                    .text_color(if unavailable {
                                        theme::error()
                                    } else {
                                        theme::accent()
                                    })
                                    .children(status),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("fav-bell-{ix}")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(30.0))
                            .rounded_full()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.stores
                                    .channels
                                    .update(cx, |c, cx| c.set_notify(&id, !notify, cx));
                            }))
                            .child(icon(Icon::Bell, px(18.0)).text_color(if notify {
                                theme::accent()
                            } else {
                                theme::text_muted()
                            })),
                    )
            })
            .collect();

        let section_title = |text: &'static str| {
            div()
                .text_size(px(18.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(text)
        };

        let new_section = (!cards.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(section_title("New videos"))
                        .child(button(
                            "channels-mark-seen",
                            "Mark all as seen",
                            ButtonVariant::Secondary,
                            cx.listener(|this, _, _, cx| {
                                this.stores.channels.update(cx, |c, cx| c.mark_all_seen(cx))
                            }),
                        )),
                )
                .child(div().flex().flex_wrap().gap(px(GRID_GAP)).children(cards))
        });

        let favorites_section = if tiles.is_empty() {
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .py(px(64.0))
                .gap(px(10.0))
                .child(icon(Icon::Users, px(56.0)).text_color(theme::bar_track()))
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(theme::text_muted())
                        .child("No favorite channels yet."),
                )
                .child(button(
                    "channels-find",
                    "Find channels",
                    ButtonVariant::Classic,
                    cx.listener(|this, _, _, cx| {
                        this.stores.search.update(cx, |s, cx| {
                            s.set_mode(crate::state::search::SearchMode::Channels, cx)
                        });
                        cx.emit(Navigate(Route::Search));
                    }),
                ))
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(section_title("Favorites"))
                .child(div().flex().flex_wrap().gap(px(GRID_GAP)).children(tiles))
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(22.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Channels"),
                    )
                    .when(!favorites.is_empty(), |s| {
                        s.child(button(
                            "channels-check",
                            if checking { "Checking…" } else { "Check now" },
                            ButtonVariant::Secondary,
                            cx.listener(|this, _, _, cx| {
                                this.stores.channels.update(cx, |c, cx| {
                                    if !c.checking {
                                        c.check_all(true, cx).detach();
                                    }
                                });
                            }),
                        ))
                    }),
            )
            .child(
                div()
                    .id("channels-scroll")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap(px(24.0))
                    .children(new_section)
                    .child(favorites_section),
            )
    }
}
