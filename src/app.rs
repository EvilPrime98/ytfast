//! Root entity: routing, startup sequence, shell chrome and the global overlays
//! (toasts, confirm dialog, app loader).

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, Styled, Subscription, Window, actions, div,
    prelude::*, px,
};

use crate::api::ytdlp;
use crate::route::{History, Navigate, ReplaceRoute, Route, window_title};
use crate::runtime;
use crate::state::Stores;
use crate::ui::app_loader::app_loader;
use crate::ui::confirm::{self, ConfirmHost};
use crate::ui::pages::channel::ChannelPage;
use crate::ui::pages::channels::ChannelsPage;
use crate::ui::pages::history::HistoryPage;
use crate::ui::pages::home::HomePage;
use crate::ui::pages::library::LibraryPage;
use crate::ui::pages::now_playing::NowPlayingPage;
use crate::ui::pages::playlist::PlaylistPage;
use crate::ui::pages::playlists::PlaylistsPage;
use crate::ui::pages::search::SearchPage;
use crate::ui::pages::settings::SettingsPage;
use crate::ui::pages::subscriptions::SubscriptionsPage;
use crate::ui::shell::header::header;
use crate::ui::shell::player_bar::PlayerBar;
use crate::ui::shell::sidebar::Sidebar;
use crate::ui::theme;
use crate::ui::toast::{self, ToastHost};

actions!(
    ytfast,
    [
        Back,
        Forward,
        ToggleFullscreen,
        RequestClose,
        TogglePlay,
        NextTrack,
        PreviousTrack
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("alt-left", Back, None),
        gpui::KeyBinding::new("alt-right", Forward, None),
        // F11 anywhere; macOS also uses ctrl-cmd-f.
        gpui::KeyBinding::new("f11", ToggleFullscreen, None),
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("ctrl-cmd-f", ToggleFullscreen, None),
        // Space must keep typing a space inside the search box.
        gpui::KeyBinding::new("space", TogglePlay, Some("AppRoot && !TextInput")),
        gpui::KeyBinding::new("ctrl-right", NextTrack, None),
        gpui::KeyBinding::new("ctrl-left", PreviousTrack, None),
    ]);
}

/// What the content area shows.
enum Page {
    Home(Entity<HomePage>),
    Search(Entity<SearchPage>),
    NowPlaying(Entity<NowPlayingPage>),
    Library(Entity<LibraryPage>),
    History(Entity<HistoryPage>),
    Channels(Entity<ChannelsPage>),
    Subscriptions(Entity<SubscriptionsPage>),
    Channel(Entity<ChannelPage>),
    Playlist(Entity<PlaylistPage>),
    Playlists(Entity<PlaylistsPage>),
    Settings(Entity<SettingsPage>),
}

pub struct AppRoot {
    history: History,
    stores: Stores,
    sidebar: Entity<Sidebar>,
    player_bar: Entity<PlayerBar>,
    toasts: Entity<ToastHost>,
    confirm: Entity<ConfirmHost>,
    page: Page,
    _page_subs: Vec<Subscription>,
    _subs: Vec<Subscription>,
    /// The startup check is still running: show the AppLoader.
    loading: bool,
    /// Dev aid: `YTFAST_OPEN` (see [`dev_route`]) opens a page once startup finishes.
    pending_route: Option<Route>,
    focus: FocusHandle,
}

impl AppRoot {
    /// `stores` outlive the window (they are created once in `main`), so playback and the
    /// library survive closing the window to the tray.
    pub fn new(stores: Stores, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        let toasts = toast::init(cx);
        let confirm = confirm::init(cx);

        let sidebar = cx.new(|cx| Sidebar::new(stores.clone(), cx));
        let player_bar = cx.new(|cx| PlayerBar::new(stores.clone(), cx));
        let subs = vec![
            cx.subscribe_in(&sidebar, window, |this, _, ev: &Navigate, window, cx| {
                this.navigate(ev.0.clone(), window, cx)
            }),
            cx.subscribe_in(&player_bar, window, |this, _, ev: &Navigate, window, cx| {
                this.navigate(ev.0.clone(), window, cx)
            }),
            cx.observe(&stores.prefs, |_, _, cx| cx.notify()),
        ];

        let home = cx.new(|cx| HomePage::new(stores.clone(), cx));
        let mut this = Self {
            history: History::default(),
            stores: stores.clone(),
            sidebar,
            player_bar,
            toasts,
            confirm,
            // The history starts at Home, so this is the page for the initial route.
            page: Page::Home(home.clone()),
            _page_subs: vec![cx.subscribe_in(
                &home,
                window,
                |this, _, ev: &Navigate, window, cx| this.navigate(ev.0.clone(), window, cx),
            )],
            _subs: subs,
            loading: true,
            pending_route: dev_route(),
            focus,
        };
        window.set_window_title(&window_title(this.history.current()));
        this.begin_startup(cx);
        this
    }

    /// Check that yt-dlp works and trim the audio cache, then drop the loader. A failure is
    /// reported but never stops the app: the user can fix the path in Settings.
    fn begin_startup(&mut self, cx: &mut Context<Self>) {
        let (bin, limit_mb) = {
            let p = &self.stores.prefs.read(cx).prefs;
            (p.ytdlp_path.clone(), p.cache_limit_mb)
        };
        cx.spawn(async move |this, cx| {
            let check = runtime::run(async move {
                ytdlp::prune_cache(&ytdlp::cache_dir(), limit_mb * 1024 * 1024);
                ytdlp::version(&bin).await
            })
            .await;
            match &check {
                Ok(v) => tracing::info!("yt-dlp {v}"),
                Err(e) => tracing::warn!("yt-dlp check failed: {e}"),
            }
            this.update(cx, |this, cx| {
                this.loading = false;
                cx.notify();
            })
            .ok();
            if let Err(e) = check {
                cx.update(|cx| toast::error(cx, e.to_string()));
            }
        })
        .detach();
    }

    pub fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        self.history.push(route);
        self.route_changed(window, cx);
    }

    pub fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history.back_or_home();
        self.route_changed(window, cx);
    }

    pub fn go_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history.forward();
        self.route_changed(window, cx);
    }

    fn route_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.set_window_title(&window_title(self.history.current()));
        self.build_page(window, cx);
        cx.notify();
    }

    /// Instantiate the view for the current route (a fresh page per visit).
    fn build_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let route = self.history.current().clone();
        self.sidebar.update(cx, |s, cx| {
            s.current = route.clone();
            cx.notify();
        });
        self._page_subs.clear();
        self.page = match route {
            Route::Home => {
                let page = cx.new(|cx| HomePage::new(self.stores.clone(), cx));
                self._page_subs.push(cx.subscribe_in(
                    &page,
                    window,
                    |this, _, ev: &Navigate, window, cx| this.navigate(ev.0.clone(), window, cx),
                ));
                Page::Home(page)
            }
            Route::Search => {
                let page = cx.new(|cx| SearchPage::new(self.stores.clone(), window, cx));
                self._page_subs.push(cx.subscribe_in(
                    &page,
                    window,
                    |this, _, ev: &Navigate, window, cx| this.navigate(ev.0.clone(), window, cx),
                ));
                Page::Search(page)
            }
            Route::Channels => {
                let page = cx.new(|cx| ChannelsPage::new(self.stores.clone(), cx));
                self._page_subs.push(cx.subscribe_in(
                    &page,
                    window,
                    |this, _, ev: &Navigate, window, cx| this.navigate(ev.0.clone(), window, cx),
                ));
                Page::Channels(page)
            }
            Route::Subscriptions => {
                let page = cx.new(|cx| SubscriptionsPage::new(self.stores.clone(), cx));
                self._page_subs.push(cx.subscribe_in(
                    &page,
                    window,
                    |this, _, ev: &Navigate, window, cx| this.navigate(ev.0.clone(), window, cx),
                ));
                Page::Subscriptions(page)
            }
            Route::Channel(channel, tab) => {
                let page = cx.new(|cx| ChannelPage::new(self.stores.clone(), channel, tab, cx));
                self._page_subs.push(cx.subscribe_in(
                    &page,
                    window,
                    |this, _, ev: &Navigate, window, cx| this.navigate(ev.0.clone(), window, cx),
                ));
                self._page_subs
                    .push(cx.subscribe(&page, |this, _, ev: &ReplaceRoute, cx| {
                        this.history.replace(ev.0.clone());
                        cx.notify();
                    }));
                Page::Channel(page)
            }
            Route::Playlists => {
                let page = cx.new(|cx| PlaylistsPage::new(self.stores.clone(), cx));
                self._page_subs.push(cx.subscribe_in(
                    &page,
                    window,
                    |this, _, ev: &Navigate, window, cx| this.navigate(ev.0.clone(), window, cx),
                ));
                Page::Playlists(page)
            }
            Route::Playlist(playlist) => {
                Page::Playlist(cx.new(|cx| PlaylistPage::new(self.stores.clone(), playlist, cx)))
            }
            Route::NowPlaying => {
                let page = cx.new(|cx| NowPlayingPage::new(self.stores.clone(), cx));
                self._page_subs.push(cx.subscribe_in(
                    &page,
                    window,
                    |this, _, ev: &Navigate, window, cx| this.navigate(ev.0.clone(), window, cx),
                ));
                Page::NowPlaying(page)
            }
            Route::Library => Page::Library(cx.new(|cx| LibraryPage::new(self.stores.clone(), cx))),
            Route::History => Page::History(cx.new(|cx| HistoryPage::new(self.stores.clone(), cx))),
            Route::Settings => {
                Page::Settings(cx.new(|cx| SettingsPage::new(self.stores.clone(), cx)))
            }
        };
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.stores.prefs.update(cx, |p, cx| {
            p.update(cx, |p| p.sidebar_collapsed = !p.sidebar_collapsed)
        });
    }
}

impl Focusable for AppRoot {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AppRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.loading {
            if let Some(route) = self.pending_route.take() {
                self.navigate(route, window, cx);
            }
        }
        let sidebar_collapsed = self.stores.prefs.read(cx).prefs.sidebar_collapsed;

        let content = match &self.page {
            Page::Home(page) => AnyView::from(page.clone()),
            Page::Search(page) => AnyView::from(page.clone()),
            Page::NowPlaying(page) => AnyView::from(page.clone()),
            Page::Library(page) => AnyView::from(page.clone()),
            Page::History(page) => AnyView::from(page.clone()),
            Page::Channels(page) => AnyView::from(page.clone()),
            Page::Subscriptions(page) => AnyView::from(page.clone()),
            Page::Channel(page) => AnyView::from(page.clone()),
            Page::Playlist(page) => AnyView::from(page.clone()),
            Page::Playlists(page) => AnyView::from(page.clone()),
            Page::Settings(page) => AnyView::from(page.clone()),
        };

        div()
            .id("app-root")
            .key_context("AppRoot")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Back, window, cx| this.go_back(window, cx)))
            .on_action(cx.listener(|this, _: &Forward, window, cx| this.go_forward(window, cx)))
            .on_action(cx.listener(|_, _: &ToggleFullscreen, window, _| window.toggle_fullscreen()))
            .on_action(cx.listener(|this, _: &TogglePlay, _, cx| {
                this.stores.player.update(cx, |p, cx| p.toggle(cx))
            }))
            .on_action(cx.listener(|this, _: &NextTrack, _, cx| {
                this.stores.player.update(cx, |p, cx| p.next(cx))
            }))
            .on_action(cx.listener(|this, _: &PreviousTrack, _, cx| {
                this.stores.player.update(cx, |p, cx| p.previous(cx))
            }))
            // The Linux close button goes through here; nothing needs saving first (library and
            // prefs are written on every change), so it just closes.
            .on_action(cx.listener(|_, _: &RequestClose, window, _| window.remove_window()))
            // Clicking anywhere that is not a text field returns the keyboard to the root, so
            // Space plays / pauses again after the search box was used.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
            )
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .font_family(theme::FONT_SANS)
            .text_size(px(14.0))
            .text_color(theme::text())
            .bg(theme::bg_app())
            .child(header(window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(!sidebar_collapsed, |s| s.child(self.sidebar.clone()))
                    .child(content),
            )
            .child(self.player_bar.clone())
            .child(self.toasts.clone())
            .child(self.confirm.clone())
            .when(self.loading, |s| s.child(app_loader("Checking yt-dlp…")))
    }
}

/// Dev aid: `YTFAST_OPEN=search|now-playing|library|settings` opens that page after startup.
fn dev_route() -> Option<Route> {
    Some(match std::env::var("YTFAST_OPEN").ok()?.as_str() {
        "home" => Route::Home,
        "search" => Route::Search,
        "now-playing" => Route::NowPlaying,
        "library" => Route::Library,
        "history" => Route::History,
        "channels" => Route::Channels,
        "playlists" => Route::Playlists,
        "subscriptions" => Route::Subscriptions,
        "settings" => Route::Settings,
        _ => return None,
    })
}
