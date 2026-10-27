use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render,
    ScrollWheelEvent, StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*,
    px,
};

use crate::route::{Navigate, Route};
use crate::state::Stores;
use crate::ui::components::button::{ButtonVariant, button};
use crate::ui::components::smooth_scroll::SmoothScroll;
use crate::ui::components::track_row::RowAction;
use crate::ui::components::video_card::{GRID_GAP, GRID_PAD, grid_metrics, video_card};
use crate::ui::icons::{Icon, icon};
use crate::ui::{theme, toast};

const PAGE: usize = 30;
const PREFIX: &str = "subs";

pub struct SubscriptionsPage {
    stores: Stores,
    scroll: SmoothScroll,
    shown: usize,
    _subs: Vec<Subscription>,
}

impl EventEmitter<Navigate> for SubscriptionsPage {}

impl SubscriptionsPage {
    pub fn new(stores: Stores, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe(&stores.subscriptions, |this, _, cx| {
                this.ensure_thumbs(cx);
                cx.notify();
            }),
            cx.observe(&stores.channels, |this, _, cx| {
                this.refresh(false, cx);
                cx.notify();
            }),
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
            cx.observe(&stores.library, |_, _, cx| cx.notify()),
            cx.observe(&stores.player, |_, _, cx| cx.notify()),
            cx.observe(&stores.prefs, |_, _, cx| cx.notify()),
        ];
        let page = Self {
            stores,
            scroll: SmoothScroll::new(),
            shown: PAGE,
            _subs: subs,
        };
        page.ensure_thumbs(cx);
        page.refresh(false, cx);
        page
    }

    fn on_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.scroll.on_wheel(ev) {
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn refresh(&self, force: bool, cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .stores
            .channels
            .read(cx)
            .favorites
            .iter()
            .map(|f| f.channel.id.clone())
            .collect();
        self.stores
            .subscriptions
            .update(cx, |s, cx| s.refresh(ids, force, cx).detach());
    }

    fn ensure_thumbs(&self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .stores
            .subscriptions
            .read(cx)
            .videos
            .iter()
            .take(self.shown)
            .map(|t| t.id.clone())
            .collect();
        self.stores.thumbs.update(cx, |t, cx| {
            for id in &ids {
                t.ensure(id, cx);
            }
        });
    }
}

impl Render for SubscriptionsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.scroll.step(window);
        let collapsed = self.stores.prefs.read(cx).prefs.sidebar_collapsed;
        let (_, card_w) = grid_metrics(window, collapsed);
        let playing_id = self.stores.player.read(cx).current().map(|t| t.id.clone());
        let has_favorites = !self.stores.channels.read(cx).favorites.is_empty();
        let (videos, loading) = {
            let s = self.stores.subscriptions.read(cx);
            (s.videos.clone(), s.loading)
        };
        let total = videos.len();
        let shown: Vec<_> = videos.into_iter().take(self.shown).collect();

        let cards: Vec<_> = shown
            .iter()
            .enumerate()
            .map(|(ix, track)| {
                let thumb = self.stores.thumbs.read(cx).get(&track.id);
                let saved = self.stores.library.read(cx).contains(&track.id);
                let (stores, all) = (self.stores.clone(), shown.clone());
                let (queue_track, save_track) = (track.clone(), track.clone());
                let (queue_stores, save_stores) = (self.stores.clone(), self.stores.clone());
                video_card(
                    PREFIX,
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

        let message = |text: &'static str| {
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .py(px(64.0))
                .gap(px(10.0))
                .child(icon(Icon::Play, px(56.0)).text_color(theme::bar_track()))
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(theme::text_muted())
                        .child(text),
                )
        };

        let body = if !has_favorites {
            message(
                "No favorite channels yet. Open a channel and favorite it to see its videos here.",
            )
            .child(button(
                "subs-find",
                "Go to channels",
                ButtonVariant::Classic,
                cx.listener(|_, _, _, cx| cx.emit(Navigate(Route::Channels))),
            ))
        } else if shown.is_empty() {
            message(if loading {
                "Loading videos…"
            } else {
                "No videos found."
            })
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(16.0))
                .child(div().flex().flex_wrap().gap(px(GRID_GAP)).children(cards))
                .when(self.shown < total, |s| {
                    s.child(div().flex().justify_center().child(button(
                        "subs-more",
                        "Load more",
                        ButtonVariant::Secondary,
                        cx.listener(|this, _, _, cx| {
                            this.shown += PAGE;
                            this.ensure_thumbs(cx);
                            cx.notify();
                        }),
                    )))
                })
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
                            .child("My Feed"),
                    )
                    .when(has_favorites, |s| {
                        s.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .when(loading, |s| {
                                    s.child(
                                        div()
                                            .text_size(px(12.0))
                                            .text_color(theme::text_muted())
                                            .child("Updating…"),
                                    )
                                })
                                .child(button(
                                    "subs-refresh",
                                    "Refresh",
                                    ButtonVariant::Secondary,
                                    cx.listener(|this, _, _, cx| this.refresh(true, cx)),
                                )),
                        )
                    }),
            )
            .child(
                div()
                    .id("subs-scroll")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll.handle)
                    .on_scroll_wheel(cx.listener(|this, ev, _, cx| this.on_wheel(ev, cx)))
                    .child(body),
            )
    }
}
