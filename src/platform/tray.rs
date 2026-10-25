//! Windows tray icon so the app keeps playing after its window is closed. Left click or "Open"
//! brings the window back, "Quit" exits. Other platforms keep the quit-on-last-window-close rule.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui::App;
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};

static SHOWN: AtomicBool = AtomicBool::new(false);

/// Whether a tray icon is actually up. The window may only hide behind it when it is, so the app
/// never vanishes with no way back.
pub fn shown() -> bool {
    SHOWN.load(Ordering::Relaxed)
}

/// Creates the icon and starts polling its events. When the tray is unavailable `shown()` stays
/// false, so closing the window still quits (otherwise the app would be unreachable).
pub fn init(cx: &mut App, show_window: fn(&mut App)) {
    let open = MenuItem::new("Open YTFast", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    if menu.append_items(&[&open, &quit]).is_err() {
        return;
    }
    // Resource 1 is the icon `build.rs` embeds from packaging/icon.ico.
    let Ok(icon) = Icon::from_resource(1, Some((32, 32))) else {
        tracing::warn!("tray: embedded icon not found");
        return;
    };
    let tray: TrayIcon = match TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("YTFast")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
    {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("tray: could not create icon: {e}");
            return;
        }
    };
    SHOWN.store(true, Ordering::Relaxed);
    let (open_id, quit_id) = (open.id().clone(), quit.id().clone());

    cx.spawn(async move |cx| {
        // Owns the icon: dropping it would remove it from the tray.
        let _tray = tray;
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                if ev.id == quit_id {
                    cx.update(|cx| cx.quit());
                } else if ev.id == open_id {
                    cx.update(show_window);
                }
            }
            while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = ev
                {
                    cx.update(show_window);
                }
            }
        }
    })
    .detach();
}
