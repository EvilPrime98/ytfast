// No console window behind the GUI in release builds on Windows.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod api;
mod app;
mod assets;
mod audio;
mod model;
mod platform;
mod route;
mod runtime;
mod state;
mod ui;

use gpui::{
    App, Bounds, QuitMode, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions, point,
    prelude::*, px, size,
};

use crate::app::AppRoot;
use crate::assets::Assets;
use crate::state::Stores;
use crate::ui::theme;

/// Whether closing the last window should leave the app running in the tray.
fn hides_to_tray(cx: &App) -> bool {
    #[cfg(windows)]
    {
        platform::tray::shown()
            && cx.global::<Stores>().prefs.read(cx).prefs.keep_running_in_background
    }
    #[cfg(not(windows))]
    {
        let _ = cx;
        false
    }
}

fn main() {
    // Held until `main` returns so buffered log lines are flushed.
    let _log_guard = platform::logging::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--quit") {
        platform::single_instance::send(platform::single_instance::Command::Quit);
        return;
    }
    let Some(commands) = platform::single_instance::claim() else {
        // Already running: bring its window forward instead of starting a second app.
        platform::single_instance::send(platform::single_instance::Command::Show);
        return;
    };
    let start_hidden = args.iter().any(|a| a == "--start-hidden");

    gpui_ce_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            theme::register_fonts(cx);
            app::bind_keys(cx);

            // Created once and kept for the whole process: the window is only a view onto them.
            let stores = Stores::new(cx);
            cx.set_global(stores);

            #[cfg(windows)]
            platform::tray::init(cx, show_main_window);

            // GPUI's default quits on the last window closing everywhere but macOS, which would
            // pre-empt the check below; quitting is decided here instead.
            cx.set_quit_mode(QuitMode::Explicit);

            // Closing the last window quits (the audio thread dies with the process), unless a
            // visible tray icon and the setting keep the app resident. Checked at close time so
            // toggling the setting takes effect immediately.
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() && !hides_to_tray(cx) {
                    cx.quit();
                }
            })
            .detach();

            // Commands from a second launch (`show`) or `--quit`.
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(150))
                        .await;
                    while let Ok(command) = commands.try_recv() {
                        cx.update(|cx| match command {
                            platform::single_instance::Command::Quit => cx.quit(),
                            platform::single_instance::Command::Show => show_main_window(cx),
                        });
                    }
                }
            })
            .detach();

            // A hidden start (login autostart) only makes sense when the tray can bring the
            // window back; otherwise show it.
            if !(start_hidden && hides_to_tray(cx)) {
                open_main_window(cx);
            }
        });
}

/// Focuses the existing window, or opens a new one if it was closed to the tray.
fn show_main_window(cx: &mut App) {
    if let Some(window) = cx.windows().first().copied() {
        window
            .update(cx, |_, window, _| window.activate_window())
            .ok();
        cx.activate(true);
    } else {
        open_main_window(cx);
    }
}

fn open_main_window(cx: &mut App) {
    // 1100x720, min 900x600, frameless on win/linux with custom controls (macOS: inset
    // titlebar with native traffic lights).
    let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(900.0), px(600.0))),
        titlebar: Some(TitlebarOptions {
            title: Some("YTFast".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.0), px(20.0))),
        }),
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some("ytfast".into()),
        ..Default::default()
    };
    let stores = cx.global::<Stores>().clone();
    cx.open_window(options, |window, cx| cx.new(|cx| AppRoot::new(stores, window, cx)))
        .unwrap();
    cx.activate(true);
}
