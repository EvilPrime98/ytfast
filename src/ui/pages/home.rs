//! Home page: a search box and a grid feed built from the history and the library.

use std::collections::HashSet;

use gpui::{
    Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render,
    ScrollDelta, ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement, Styled, Subscription,
    Window, div, point, prelude::*, px,
};

use crate::model::Track;
use crate::route::{Navigate, Route};
use crate::state::Stores;
use crate::ui::components::text_input::{TextInput, TextInputEvent};
use crate::ui::components::track_row::RowAction;
use crate::ui::components::video_card::video_card;
use crate::ui::icons::{Icon, icon};
use crate::ui::{theme, toast};

const GAP: f32 = 16.0;
const MIN_CARD: f32 = 220.0;
const PAD: f32 = 24.0;
/// Rows of cards shown per section.
const SECTION_ROWS: usize = 2;
const RECENT_LIMIT: usize = 12;
const WHEEL_STEP: f32 = 64.0;
const SCROLL_EASE: f32 = 0.15;
const SCROLL_SNAP: f32 = 0.5;
/// Element-id prefixes, one per feed section (the feed has a handful at most).
const FEED_PREFIXES: [&str; 5] = ["feed-0", "feed-1", "feed-2", "feed-3", "feed-4"];

pub struct HomePage {
    stores: Stores,
    input: Entity<TextInput>,
    scroll: ScrollHandle,
    scroll_target: f32,
    gliding: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<Navigate> for HomePage {}

impl HomePage {
    pub fn new(stores: Stores, cx: &mut Context<Self>) -> Self {
        let input =
            cx.new(|cx| TextInput::new("Search songs, artists, or paste a YouTube link", cx));
        let subs = vec![
            cx.subscribe(&input, |this, _, ev: &TextInputEvent, cx| {
                if matches!(ev, TextInputEvent::Submit) {
                    this.submit(cx);
                }
            }),
            // Covers for new tracks are requested when they arrive, never from `render`.
            cx.observe(&stores.feed, |this, _, cx| {
                this.ensure_thumbs(cx);
                cx.notify();
            }),
            cx.observe(&stores.history, |this, _, cx| {
                this.ensure_thumbs(cx);
                cx.notify();
            }),
            cx.observe(&stores.thumbs, |_, _, cx| cx.notify()),
            cx.observe(&stores.library, |_, _, cx| cx.notify()),
            cx.observe(&stores.player, |_, _, cx| cx.notify()),
            cx.observe(&stores.prefs, |_, _, cx| cx.notify()),
        ];
        let page = Self {
            stores,
            input,
            scroll: ScrollHandle::new(),
            scroll_target: 0.0,
            gliding: false,
            _subs: subs,
        };
        page.ensure_thumbs(cx);
        page.refresh_feed(cx);
        page
    }

    fn on_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let max = f32::from(self.scroll.max_offset().y);
        let current = -f32::from(self.scroll.offset().y);
        let ScrollDelta::Lines(lines) = ev.delta else {
            self.gliding = false;
            return;
        };
        let base = if self.gliding {
            self.scroll_target
        } else {
            current
        };
        self.scroll_target = (base - lines.y * WHEEL_STEP).clamp(0.0, max);
        self.gliding = true;
        cx.stop_propagation();
        cx.notify();
    }

    fn step_glide(&mut self, window: &mut Window) {
        if !self.gliding {
            return;
        }
        let max = f32::from(self.scroll.max_offset().y);
        let target = self.scroll_target.min(max);
        let current = -f32::from(self.scroll.offset().y);
        let next = if (target - current).abs() < SCROLL_SNAP {
            self.gliding = false;
            target
        } else {
            window.request_animation_frame();
            current + (target - current) * SCROLL_EASE
        };
        self.scroll.set_offset(point(px(0.0), px(-next)));
    }

    fn refresh_feed(&self, cx: &mut Context<Self>) {
        let bin = self.stores.prefs.read(cx).prefs.ytdlp_path.clone();
        let history = self.stores.history.read(cx).entries.clone();
        let library = self.stores.library.read(cx).tracks.clone();
        self.stores
            .feed
            .update(cx, |f, cx| f.refresh(bin, history, library, cx).detach());
    }

    fn ensure_thumbs(&self, cx: &mut Context<Self>) {
        let mut ids: Vec<String> = self
            .stores
            .history
            .read(cx)
            .entries
            .iter()
            .take(RECENT_LIMIT * 2)
            .map(|e| e.track.id.clone())
            .collect();
        ids.extend(
            self.stores
                .feed
                .read(cx)
                .sections
                .iter()
                .flat_map(|s| s.tracks.iter().map(|t| t.id.clone())),
        );
        self.stores.thumbs.update(cx, |t, cx| {
            for id in &ids {
                t.ensure(id, cx);
            }
        });
    }

    /// Run the search, then show the results on the search page.
    fn submit(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).value().to_string();
        if query.trim().is_empty() {
            return;
        }
        let (bin, limit) = {
            let p = &self.stores.prefs.read(cx).prefs;
            (p.ytdlp_path.clone(), p.search_limit)
        };
        self.stores
            .search
            .update(cx, |s, cx| s.run(query, bin, limit, cx).detach());
        cx.emit(Navigate(Route::Search));
    }

    fn section(
        &self,
        prefix: &'static str,
        title: String,
        tracks: Vec<Track>,
        cols: usize,
        card_w: f32,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let playing_id = self.stores.player.read(cx).current().map(|t| t.id.clone());
        let tracks: Vec<Track> = tracks.into_iter().take(cols * SECTION_ROWS).collect();
        let cards: Vec<_> = tracks
            .iter()
            .enumerate()
            .map(|(ix, track)| {
                let thumb = self.stores.thumbs.read(cx).get(&track.id);
                let saved = self.stores.library.read(cx).contains(&track.id);
                let stores = self.stores.clone();
                let all = tracks.clone();
                let queue_track = track.clone();
                let save_track = track.clone();
                let queue_stores = self.stores.clone();
                let save_stores = self.stores.clone();
                video_card(
                    prefix,
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
        grid(title, cards)
    }
}

impl Render for HomePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.step_glide(window);
        let sidebar = if self.stores.prefs.read(cx).prefs.sidebar_collapsed {
            0.0
        } else {
            f32::from(theme::sidebar_width())
        };
        let avail = (f32::from(window.viewport_size().width) - sidebar - PAD * 2.0).max(MIN_CARD);
        let cols = (((avail + GAP) / (MIN_CARD + GAP)) as usize).max(1);
        // A pixel of slack so rounding never wraps the last card of a row.
        let card_w = ((avail - GAP * (cols - 1) as f32) / cols as f32 - 1.0).floor();

        let recent: Vec<Track> = {
            let mut seen = HashSet::new();
            self.stores
                .history
                .read(cx)
                .entries
                .iter()
                .filter(|e| seen.insert(e.track.id.clone()))
                .take(RECENT_LIMIT)
                .map(|e| e.track.clone())
                .collect()
        };
        let (sections, loading) = {
            let feed = self.stores.feed.read(cx);
            (feed.sections.clone(), feed.loading)
        };

        let mut blocks = Vec::new();
        if !recent.is_empty() {
            blocks.push(self.section("recent", "Jump back in".into(), recent, cols, card_w, cx));
        }
        for (s, prefix) in sections.into_iter().zip(FEED_PREFIXES) {
            blocks.push(self.section(prefix, s.title, s.tracks, cols, card_w, cx));
        }
        if blocks.is_empty() {
            blocks.push(if loading {
                placeholders(cols, card_w)
            } else {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .py(px(64.0))
                    .gap(px(10.0))
                    .child(icon(Icon::Music, px(56.0)).text_color(theme::bar_track()))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .text_color(theme::text_muted())
                            .child("Nothing here yet. Search for something to get started."),
                    )
            });
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .px(px(PAD))
            .py(px(16.0))
            .gap(px(12.0))
            .child(
                div()
                    .flex()
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
            .child(
                div()
                    .id("home-feed")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .on_scroll_wheel(cx.listener(|this, ev, _, cx| this.on_wheel(ev, cx)))
                    .gap(px(24.0))
                    .children(blocks),
            )
    }
}

/// A titled, wrapping grid of already sized cards.
fn grid(title: String, cards: Vec<impl IntoElement>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .truncate()
                .text_size(px(18.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(div().flex().flex_wrap().gap(px(GAP)).children(cards))
}

/// Grey tiles shown while the first feed loads.
fn placeholders(cols: usize, card_w: f32) -> gpui::Div {
    let tiles: Vec<_> = (0..cols * SECTION_ROWS)
        .map(|_| {
            div()
                .flex_none()
                .w(px(card_w))
                .h(px((card_w - 12.0) * 9.0 / 16.0 + 64.0))
                .rounded(px(10.0))
                .bg(theme::bg_panel())
        })
        .collect();
    grid("Loading your feed…".into(), tiles)
}
