//! Search page: a search box (or a pasted YouTube link) and the results.

use gpui::{
    Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render,
    ScrollWheelEvent, StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*,
    px,
};

use crate::route::{ChannelTab, Navigate, Route};
use crate::state::Stores;
use crate::state::search::SearchMode;
use crate::ui::components::button::{ButtonVariant, button};
use crate::ui::components::smooth_scroll::SmoothScroll;
use crate::ui::components::spinner::spinner_fill;
use crate::ui::components::text_input::{TextInput, TextInputEvent};
use crate::ui::components::track_row::{RowAction, track_row};
use crate::ui::components::video_card::avatar;
use crate::ui::icons::{Icon, icon};
use crate::ui::{theme, toast};

pub struct SearchPage {
    stores: Stores,
    input: Entity<TextInput>,
    video_scroll: SmoothScroll,
    channel_scroll: SmoothScroll,
    _subs: Vec<Subscription>,
}

impl EventEmitter<Navigate> for SearchPage {}

impl SearchPage {
    pub fn new(stores: Stores, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let last_query = stores.search.read(cx).query.clone();
        let input = cx.new(|cx| {
            TextInput::new("Search songs, artists, or paste a YouTube link", cx)
                .with_value(last_query)
        });
        let subs = vec![
            cx.subscribe(&input, |this, _, ev: &TextInputEvent, cx| {
                if matches!(ev, TextInputEvent::Submit) {
                    this.submit(cx);
                }
            }),
            // Covers for new results are requested when they arrive, never from `render`.
            cx.observe(&stores.search, |this, search, cx| {
                let ids: Vec<String> = search
                    .read(cx)
                    .results
                    .iter()
                    .map(|t| t.id.clone())
                    .collect();
                let avatars: Vec<_> = search
                    .read(cx)
                    .channels
                    .iter()
                    .map(|c| (c.id.clone(), c.avatar_url.clone()))
                    .collect();
                this.stores.thumbs.update(cx, |t, cx| {
                    for id in &ids {
                        t.ensure(id, cx);
                    }
                    for (id, url) in &avatars {
                        t.ensure_avatar(id, url, cx);
                    }
                });
                cx.notify();
            }),
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
            cx.observe(&stores.library, |_, _, cx| cx.notify()),
            cx.observe(&stores.channels, |_, _, cx| cx.notify()),
            cx.observe(&stores.player, |_, _, cx| cx.notify()),
        ];
        // Typing starts right away.
        let focus = gpui::Focusable::focus_handle(input.read(cx), cx);
        window.focus(&focus, cx);
        Self {
            stores,
            input,
            video_scroll: SmoothScroll::new(),
            channel_scroll: SmoothScroll::new(),
            _subs: subs,
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        self.video_scroll.reset();
        self.channel_scroll.reset();
        let query = self.input.read(cx).value().to_string();
        let (bin, limit) = {
            let p = &self.stores.prefs.read(cx).prefs;
            (p.ytdlp_path.clone(), p.search_limit)
        };
        self.stores
            .search
            .update(cx, |s, cx| s.run(query, bin, limit, cx).detach());
    }

    fn on_video_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.video_scroll.on_wheel(ev) {
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn on_channel_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        if self.channel_scroll.on_wheel(ev) {
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn channel_rows(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let channels = self.stores.search.read(cx).channels.clone();
        channels
            .into_iter()
            .enumerate()
            .map(|(ix, channel)| {
                let image = self.stores.thumbs.read(cx).avatar(&channel.id);
                let favorite = self.stores.channels.read(cx).is_favorite(&channel.id);
                let (open, toggle) = (channel.clone(), channel.clone());
                div()
                    .id(("channel-row", ix))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .h(px(64.0))
                    .px(px(10.0))
                    .rounded(px(8.0))
                    .hover(|s| s.bg(theme::bg_row_hover()))
                    .child(
                        div()
                            .id(("channel-open", ix))
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap(px(12.0))
                            .cursor_pointer()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(Navigate(Route::Channel(
                                    open.clone(),
                                    ChannelTab::default(),
                                )))
                            }))
                            .child(avatar(image, 44.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(14.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(channel.name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .id(("channel-fav", ix))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(32.0))
                            .rounded_full()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.stores
                                    .channels
                                    .update(cx, |c, cx| c.toggle(&toggle, cx));
                            }))
                            .child(
                                icon(
                                    if favorite {
                                        Icon::Heart
                                    } else {
                                        Icon::HeartOutline
                                    },
                                    px(18.0),
                                )
                                .text_color(if favorite {
                                    theme::accent()
                                } else {
                                    theme::text_muted()
                                }),
                            ),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn mode_chip(
        &self,
        id: &'static str,
        label: &'static str,
        mode: SearchMode,
        current: SearchMode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = mode == current;
        div()
            .id(id)
            .flex()
            .items_center()
            .h(px(30.0))
            .px(px(14.0))
            .rounded_full()
            .cursor_pointer()
            .text_size(px(13.0))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(if active {
                theme::on_accent()
            } else {
                theme::text_muted()
            })
            .bg(if active {
                theme::accent()
            } else {
                theme::bg_panel()
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.stores.search.update(cx, |s, cx| s.set_mode(mode, cx))
            }))
            .child(label)
    }
}

impl Render for SearchPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.video_scroll.step(window);
        self.channel_scroll.step(window);
        let search = self.stores.search.read(cx);
        let mode = search.mode;
        let results = search.results.clone();
        let loading = search.loading;
        let channel_mode = mode == SearchMode::Channels;
        let searched = search.searched;
        let channel_count = search.channels.len();
        let playing_id = self.stores.player.read(cx).current().map(|t| t.id.clone());

        let channel_rows = if channel_mode {
            self.channel_rows(cx)
        } else {
            Vec::new()
        };
        let rows: Vec<_> = results
            .iter()
            .enumerate()
            .map(|(ix, track)| {
                let thumb = self.stores.thumbs.read(cx).get(&track.id);
                let saved = self.stores.library.read(cx).contains(&track.id);
                let stores = self.stores.clone();
                let all = results.clone();
                let queue_track = track.clone();
                let save_track = track.clone();
                let queue_stores = self.stores.clone();
                let save_stores = self.stores.clone();
                track_row(
                    "search",
                    ix,
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

        let body = if channel_mode && !loading && searched && channel_count > 0 {
            div()
                .id("channel-results")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.channel_scroll.handle)
                .on_scroll_wheel(cx.listener(|this, ev, _, cx| this.on_channel_wheel(ev, cx)))
                .gap(px(2.0))
                .children(channel_rows)
                .into_any_element()
        } else if loading {
            spinner_fill().into_any_element()
        } else if !searched {
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .flex_1()
                .gap(px(10.0))
                .child(icon(Icon::Music, px(56.0)).text_color(theme::bar_track()))
                .child(
                    div()
                        .text_size(px(18.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("Listen to anything on YouTube"),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(theme::text_muted())
                        .child("Type a song or artist and press Enter, or paste a video or playlist link."),
                )
                .into_any_element()
        } else if channel_mode {
            message("No channels found").into_any_element()
        } else if rows.is_empty() {
            message("No results").into_any_element()
        } else {
            div()
                .id("search-results")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.video_scroll.handle)
                .on_scroll_wheel(cx.listener(|this, ev, _, cx| this.on_video_wheel(ev, cx)))
                .gap(px(2.0))
                .children(rows)
                .into_any_element()
        };

        let play_all = {
            let stores = self.stores.clone();
            let all = results.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
                stores
                    .player
                    .update(cx, |p, cx| p.play_tracks(all.clone(), 0, cx))
            }
        };

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
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .items_center()
                            .gap(px(10.0))
                            .h(px(44.0))
                            .px(px(16.0))
                            .rounded_full()
                            .bg(theme::bg_panel())
                            .border_1()
                            .border_color(theme::border_subtle())
                            .child(icon(Icon::Search, px(18.0)).text_color(theme::text_muted()))
                            .child(self.input.clone()),
                    )
                    .when(!channel_mode && !results.is_empty(), |s| {
                        s.child(button(
                            "search-play-all",
                            "Play all",
                            ButtonVariant::Classic,
                            play_all,
                        ))
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(self.mode_chip("mode-videos", "Videos", SearchMode::Videos, mode, cx))
                    .child(self.mode_chip(
                        "mode-channels",
                        "Channels",
                        SearchMode::Channels,
                        mode,
                        cx,
                    )),
            )
            .child(body)
    }
}

pub(super) fn message(text: impl Into<gpui::SharedString>) -> impl IntoElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .text_size(px(14.0))
        .text_color(theme::text_muted())
        .child(text.into())
}
