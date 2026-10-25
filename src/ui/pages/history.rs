use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement,
    Styled, Subscription, Window, div, prelude::*, px,
};

use crate::state::Stores;
use crate::state::history::{now_secs, relative_time};
use crate::ui::components::button::{ButtonVariant, button};
use crate::ui::components::track_row::{RowAction, track_row};
use crate::ui::icons::{Icon, icon};
use crate::ui::{theme, toast};

pub struct HistoryPage {
    stores: Stores,
    _subs: Vec<Subscription>,
}

impl HistoryPage {
    pub fn new(stores: Stores, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe(&stores.history, |this, _, cx| {
                this.ensure_thumbs(cx);
                cx.notify();
            }),
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
            cx.observe(&stores.player, |_, _, cx| cx.notify()),
        ];
        let page = Self {
            stores,
            _subs: subs,
        };
        page.ensure_thumbs(cx);
        page
    }

    fn ensure_thumbs(&self, cx: &mut Context<Self>) {
        let ids: Vec<String> = self
            .stores
            .history
            .read(cx)
            .entries
            .iter()
            .map(|e| e.track.id.clone())
            .collect();
        self.stores.thumbs.update(cx, |t, cx| {
            for id in &ids {
                t.ensure(id, cx);
            }
        });
    }
}

impl Render for HistoryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = self.stores.history.read(cx).entries.clone();
        let tracks: Vec<_> = entries.iter().map(|e| e.track.clone()).collect();
        let playing_id = self.stores.player.read(cx).current().map(|t| t.id.clone());
        let now = now_secs();

        let rows: Vec<_> = entries
            .iter()
            .enumerate()
            .map(|(ix, entry)| {
                let track = &entry.track;
                let thumb = self.stores.thumbs.read(cx).get(&track.id);
                let play_stores = self.stores.clone();
                let all = tracks.clone();
                let queue_stores = self.stores.clone();
                let queue_track = track.clone();
                let remove_stores = self.stores.clone();
                let row = track_row(
                    "history",
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
                            queue_stores
                                .player
                                .update(cx, |p, cx| p.enqueue(queue_track.clone(), cx));
                            toast::success(cx, "Added to queue");
                        }),
                        RowAction::new(Icon::Trash, move |_, cx| {
                            remove_stores.history.update(cx, |h, cx| h.remove(ix, cx));
                        }),
                    ],
                );
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(row.flex_1().min_w_0())
                    .child(
                        div()
                            .flex_none()
                            .w(px(72.0))
                            .text_size(px(12.0))
                            .text_color(theme::text_muted())
                            .child(relative_time(entry.played_at, now)),
                    )
            })
            .collect();

        let clear_stores = self.stores.clone();
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
                            .child("History"),
                    )
                    .when(!entries.is_empty(), |s| {
                        s.child(button(
                            "history-clear",
                            "Clear all",
                            ButtonVariant::Classic,
                            move |_, _, cx| clear_stores.history.update(cx, |h, cx| h.clear(cx)),
                        ))
                    }),
            )
            .child(if entries.is_empty() {
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(10.0))
                    .child(icon(Icon::History, px(56.0)).text_color(theme::bar_track()))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .text_color(theme::text_muted())
                            .child("Tracks you play will show up here."),
                    )
                    .into_any_element()
            } else {
                div()
                    .id("history-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap(px(2.0))
                    .children(rows)
                    .into_any_element()
            })
    }
}
