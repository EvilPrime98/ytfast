//! Search page: a search box (or a pasted YouTube link) and the results.

use gpui::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*, px,
};

use crate::state::Stores;
use crate::ui::components::button::{ButtonVariant, button};
use crate::ui::components::text_input::{TextInput, TextInputEvent};
use crate::ui::components::track_row::{RowAction, track_row};
use crate::ui::icons::{Icon, icon};
use crate::ui::{theme, toast};

pub struct HomePage {
    stores: Stores,
    input: Entity<TextInput>,
    _subs: Vec<Subscription>,
}

impl HomePage {
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
                this.stores.thumbs.update(cx, |t, cx| {
                    for id in &ids {
                        t.ensure(id, cx);
                    }
                });
                cx.notify();
            }),
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
            cx.observe(&stores.library, |_, _, cx| cx.notify()),
            cx.observe(&stores.player, |_, _, cx| cx.notify()),
        ];
        // Typing starts right away.
        let focus = gpui::Focusable::focus_handle(input.read(cx), cx);
        window.focus(&focus, cx);
        Self {
            stores,
            input,
            _subs: subs,
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).value().to_string();
        let (bin, limit) = {
            let p = &self.stores.prefs.read(cx).prefs;
            (p.ytdlp_path.clone(), p.search_limit)
        };
        self.stores
            .search
            .update(cx, |s, cx| s.run(query, bin, limit, cx).detach());
    }
}

impl Render for HomePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let search = self.stores.search.read(cx);
        let results = search.results.clone();
        let loading = search.loading;
        let searched = search.searched;
        let playing_id = self.stores.player.read(cx).current().map(|t| t.id.clone());

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
                    "home",
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

        let body = if loading {
            message("Searching…").into_any_element()
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
        } else if rows.is_empty() {
            message("No results").into_any_element()
        } else {
            div()
                .id("home-results")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
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
                    .when(!results.is_empty(), |s| {
                        s.child(button(
                            "home-play-all",
                            "Play all",
                            ButtonVariant::Classic,
                            play_all,
                        ))
                    }),
            )
            .child(body)
    }
}

fn message(text: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .text_size(px(14.0))
        .text_color(theme::text_muted())
        .child(text)
}
