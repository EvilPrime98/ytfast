//! Design tokens. Dark theme only.

use gpui::{Pixels, Rgba, px, rgb, rgba};

pub fn accent() -> Rgba {
    rgb(0xff4e45)
}
/// Text/icon colour on top of a solid accent fill.
pub fn on_accent() -> Rgba {
    rgb(0x1a0605)
}
/// rgba(255,78,69,0.30)
pub fn accent_soft() -> Rgba {
    rgba(0xff4e454d)
}
pub fn bg_app() -> Rgba {
    rgb(0x121212)
}
pub fn bg_chrome() -> Rgba {
    rgb(0x0b0b0b)
}
pub fn bg_panel() -> Rgba {
    rgb(0x1c1c1c)
}
pub fn bg_modal() -> Rgba {
    rgb(0x232323)
}
/// Track-row hover wash.
pub fn bg_row_hover() -> Rgba {
    rgba(0xffffff0d)
}
/// Unfilled part of the seek / volume bars.
pub fn bar_track() -> Rgba {
    rgba(0xffffff26)
}
/// rgba(255,255,255,0.08)
pub fn border_subtle() -> Rgba {
    rgba(0xffffff14)
}
pub fn error() -> Rgba {
    rgb(0xff5c5c)
}
pub fn text() -> Rgba {
    rgb(0xffffff)
}
pub fn text_muted() -> Rgba {
    rgb(0xaaaaaa)
}
/// Hover wash used on header/window buttons.
pub fn hover() -> Rgba {
    rgba(0xffffff14)
}

pub fn header_height() -> Pixels {
    px(56.0)
}
pub fn sidebar_width() -> Pixels {
    px(220.0)
}
pub fn player_bar_height() -> Pixels {
    px(84.0)
}

/// Geist, embedded in `assets/fonts` and registered by [`register_fonts`] at startup.
pub const FONT_SANS: &str = "Geist";

const FONT_FILES: [&str; 6] = [
    "fonts/Geist-Regular.ttf",
    "fonts/Geist-Medium.ttf",
    "fonts/Geist-SemiBold.ttf",
    "fonts/Geist-Bold.ttf",
    "fonts/GeistMono-Medium.ttf",
    "fonts/GeistMono-SemiBold.ttf",
];

/// Register the embedded fonts with GPUI's text system. A missing or unreadable file only costs
/// that weight (GPUI falls back to a system font), so it is logged, not fatal.
pub fn register_fonts(cx: &mut gpui::App) {
    use gpui::AssetSource as _;
    let fonts = FONT_FILES
        .iter()
        .filter_map(|path| match crate::assets::Assets.load(path) {
            Ok(Some(data)) => Some(data),
            _ => {
                tracing::warn!("embedded font {path} is missing");
                None
            }
        })
        .collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        tracing::warn!("could not register the embedded fonts: {e}");
    }
}
