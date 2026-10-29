//! Sidebar: one entry per route. Emits `Navigate`.

use gpui::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, prelude::*, px,
};

use crate::route::{Navigate, Route};
use crate::state::Stores;
use crate::ui::icons::{Icon, icon};
use crate::ui::theme;

pub struct Sidebar {
    stores: Stores,
    pub current: Route,
}

impl EventEmitter<Navigate> for Sidebar {}

impl Sidebar {
    pub fn new(stores: Stores, cx: &mut Context<Self>) -> Self {
        // The library count in the entry follows the store.
        cx.observe(&stores.library, |_, _, cx| cx.notify()).detach();
        cx.observe(&stores.channels, |_, _, cx| cx.notify())
            .detach();
        Self {
            stores,
            current: Route::home(),
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = self.stores.library.read(cx).tracks.len();
        let new_videos = self.stores.channels.read(cx).unseen_total();
        let entries: [(Route, Icon, String); 9] = [
            (Route::Home, Icon::Music, "Home".into()),
            (Route::Search, Icon::Search, "Search".into()),
            (Route::Subscriptions, Icon::Play, "My Feed".into()),
            (Route::NowPlaying, Icon::Queue, "Now playing".into()),
            (
                Route::Library,
                Icon::Heart,
                if saved > 0 {
                    format!("Library ({saved})")
                } else {
                    "Library".into()
                },
            ),
            (Route::History, Icon::History, "History".into()),
            (
                Route::Channels,
                Icon::Users,
                if new_videos > 0 {
                    format!("Channels ({new_videos} new)")
                } else {
                    "Channels".into()
                },
            ),
            (Route::Playlists, Icon::Playlist, "Playlists".into()),
            (Route::Settings, Icon::Gear, "Settings".into()),
        ];
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(theme::sidebar_width())
            .h_full()
            .p(px(10.0))
            .gap(px(2.0))
            .bg(theme::bg_chrome())
            .border_r_1()
            .border_color(theme::border_subtle())
            .children(
                entries
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (route, glyph, label))| {
                        let active = route == self.current
                            || (route == Route::Channels
                                && matches!(self.current, Route::Channel(..) | Route::Playlist(_)));
                        let color = if active {
                            theme::text()
                        } else {
                            theme::text_muted()
                        };
                        div()
                            .id(("nav", ix))
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .h(px(40.0))
                            .px(px(12.0))
                            .rounded(px(8.0))
                            .cursor_pointer()
                            .text_size(px(14.0))
                            .font_weight(if active {
                                gpui::FontWeight::SEMIBOLD
                            } else {
                                gpui::FontWeight::MEDIUM
                            })
                            .text_color(color)
                            .when(active, |s| s.bg(theme::hover()))
                            .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                            .on_click(
                                cx.listener(move |_, _, _, cx| cx.emit(Navigate(route.clone()))),
                            )
                            .child(icon(glyph, px(20.0)).text_color(if active {
                                theme::accent()
                            } else {
                                color
                            }))
                            .child(label)
                    }),
            )
    }
}
