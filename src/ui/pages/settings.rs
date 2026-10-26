//! Settings: the yt-dlp location, playback and cache options.

use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Window, div, prelude::*, px,
};

use crate::api::ytdlp;
use crate::runtime;
use crate::state::Stores;
use crate::ui::components::button::{ButtonVariant, button};
use crate::ui::components::checkbox::checkbox;
use crate::ui::components::dropdown::dropdown;
use crate::ui::components::text_input::{TextInput, TextInputEvent};
use crate::ui::confirm::{self, ConfirmOptions};
use crate::ui::{theme, toast};

pub struct SettingsPage {
    stores: Stores,
    path_input: Entity<TextInput>,
    /// Size of the audio cache, once measured.
    cache_bytes: Option<u64>,
    _subs: Vec<Subscription>,
}

impl SettingsPage {
    pub fn new(stores: Stores, cx: &mut Context<Self>) -> Self {
        let path = stores.prefs.read(cx).prefs.ytdlp_path.clone();
        let path_input = cx.new(|cx| TextInput::new("yt-dlp (found on PATH)", cx).with_value(path));
        let subs = vec![
            cx.observe(&stores.prefs, |_, _, cx| cx.notify()),
            cx.subscribe(&path_input, |this, _, ev: &TextInputEvent, cx| {
                if matches!(ev, TextInputEvent::Submit) {
                    this.save_path(cx);
                }
            }),
        ];
        let mut this = Self {
            stores,
            path_input,
            cache_bytes: None,
            _subs: subs,
        };
        this.measure_cache(cx);
        this
    }

    fn measure_cache(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let bytes = runtime::run(async { ytdlp::cache_size(&ytdlp::cache_dir()) }).await;
            this.update(cx, |this, cx| {
                this.cache_bytes = Some(bytes);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn save_path(&mut self, cx: &mut Context<Self>) {
        let path = self.path_input.read(cx).value().trim().to_string();
        self.stores
            .prefs
            .update(cx, |p, cx| p.update(cx, |p| p.ytdlp_path = path.clone()));
        cx.spawn(async move |_, cx| {
            let result = runtime::run(async move { ytdlp::version(&path).await }).await;
            cx.update(|cx| match result {
                Ok(v) => toast::success(cx, format!("yt-dlp {v} is ready")),
                Err(e) => toast::error(cx, e.to_string()),
            });
        })
        .detach();
    }

    fn clear_cache(&mut self, cx: &mut Context<Self>) {
        let me = cx.entity();
        confirm::ask(
            cx,
            ConfirmOptions::new(
                "Clear the audio cache?",
                "Downloaded audio is deleted. Tracks are downloaded again when played.",
            )
            .labels("Clear", "Cancel"),
            move |answer, _, cx| {
                if answer != Some(true) {
                    return;
                }
                me.update(cx, |this, cx| {
                    cx.spawn(async move |this, cx| {
                        runtime::run(async { ytdlp::prune_cache(&ytdlp::cache_dir(), 0) }).await;
                        this.update(cx, |this, cx| this.measure_cache(cx)).ok();
                    })
                    .detach();
                    this.cache_bytes = None;
                    cx.notify();
                });
            },
        );
    }
}

fn section(title: &'static str, body: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .text_size(px(14.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(body)
}

fn format_mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.stores.prefs.read(cx);
        let prefs = store.prefs.clone();
        let open = store.open_menu;
        let prefs_store = self.stores.prefs.clone();

        let limit_options: Vec<(u32, SharedString)> = [10u32, 15, 25, 50]
            .into_iter()
            .map(|n| (n, format!("{n} results").into()))
            .collect();
        let cache_options: Vec<(u64, SharedString)> = [256u64, 1024, 4096]
            .into_iter()
            .map(|mb| (mb, format!("{} MB", mb).into()))
            .collect();

        let autoplay = {
            let s = prefs_store.clone();
            checkbox(
                "set-autoplay",
                "Play the next track automatically",
                prefs.autoplay_next,
                move |on, _, cx| s.update(cx, |p, cx| p.update(cx, |p| p.autoplay_next = on)),
            )
        };

        let interval_options: Vec<(u64, SharedString)> = [
            (15u64, "15 minutes"),
            (30, "30 minutes"),
            (60, "1 hour"),
            (180, "3 hours"),
        ]
        .into_iter()
        .map(|(m, l)| (m, l.into()))
        .collect();

        let channel_notify = {
            let s = prefs_store.clone();
            checkbox(
                "set-channel-notify",
                "Notify about new videos from favorite channels",
                prefs.channel_notifications,
                move |on, _, cx| {
                    s.update(cx, |p, cx| p.update(cx, |p| p.channel_notifications = on))
                },
            )
        };

        let interval_menu = {
            let (open_s, select_s) = (prefs_store.clone(), prefs_store.clone());
            dropdown(
                "set-interval",
                interval_options,
                prefs.channel_poll_minutes,
                open == Some("set-interval"),
                false,
                move |o, cx| open_s.update(cx, |p, cx| p.set_menu_open("set-interval", o, cx)),
                move |v, cx| {
                    select_s.update(cx, |p, cx| p.update(cx, |p| p.channel_poll_minutes = v))
                },
            )
        };

        let background = {
            let s = prefs_store.clone();
            checkbox(
                "set-background",
                "Keep playing in the tray when the window is closed",
                prefs.keep_running_in_background,
                move |on, _, cx| {
                    s.update(cx, |p, cx| {
                        p.update(cx, |p| p.keep_running_in_background = on)
                    })
                },
            )
        };

        let limit_menu = {
            let (open_s, select_s) = (prefs_store.clone(), prefs_store.clone());
            dropdown(
                "set-limit",
                limit_options,
                prefs.search_limit,
                open == Some("set-limit"),
                false,
                move |o, cx| open_s.update(cx, |p, cx| p.set_menu_open("set-limit", o, cx)),
                move |v, cx| select_s.update(cx, |p, cx| p.update(cx, |p| p.search_limit = v)),
            )
        };

        let cache_menu = {
            let (open_s, select_s) = (prefs_store.clone(), prefs_store.clone());
            dropdown(
                "set-cache",
                cache_options,
                prefs.cache_limit_mb,
                open == Some("set-cache"),
                false,
                move |o, cx| open_s.update(cx, |p, cx| p.set_menu_open("set-cache", o, cx)),
                move |v, cx| select_s.update(cx, |p, cx| p.update(cx, |p| p.cache_limit_mb = v)),
            )
        };

        div()
            .id("settings")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(24.0))
            .py(px(16.0))
            .gap(px(24.0))
            .child(
                div()
                    .text_size(px(22.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Settings"),
            )
            .child(section(
                "yt-dlp",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme::text_muted())
                            .child("Full path to the yt-dlp program. Leave empty to use the one on PATH."),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_1()
                                    .h(px(36.0))
                                    .px(px(12.0))
                                    .rounded(px(6.0))
                                    .bg(theme::bg_panel())
                                    .border_1()
                                    .border_color(theme::border_subtle())
                                    .child(self.path_input.clone()),
                            )
                            .child(button(
                                "set-save-path",
                                "Save & check",
                                ButtonVariant::Classic,
                                cx.listener(|this, _, _, cx| this.save_path(cx)),
                            )),
                    ),
            ))
            .child(section("Playback", autoplay))
            .child(section("Background", background))
            .child(section(
                "Channels",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(channel_notify)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(div().text_size(px(13.0)).child("Check every"))
                            .child(interval_menu),
                    ),
            ))
            .child(section(
                "Search",
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(div().text_size(px(13.0)).child("Show up to"))
                    .child(limit_menu),
            ))
            .child(section(
                "Audio cache",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(theme::text_muted())
                            .child(match self.cache_bytes {
                                Some(b) => format!("{} on disk", format_mb(b)),
                                None => "Measuring…".to_string(),
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(div().text_size(px(13.0)).child("Keep at most"))
                            .child(cache_menu)
                            .child(button(
                                "set-clear-cache",
                                "Clear cache",
                                ButtonVariant::Secondary,
                                cx.listener(|this, _, _, cx| this.clear_cache(cx)),
                            )),
                    ),
            ))
    }
}
