//! Monochrome SVG icons (`assets/icons/*.svg`). Colour comes from the element's `text_color`,
//! like `currentColor`.

use gpui::{Pixels, Styled, Svg, svg};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Bell,
    Burger,
    ChevronDown,
    Close,
    Gear,
    Heart,
    HeartOutline,
    History,
    Music,
    Next,
    Pause,
    Play,
    Plus,
    Prev,
    Queue,
    Search,
    Spinner,
    Trash,
    Users,
    Volume,
    WinMinimize,
    WinMaximize,
    WinRestore,
}

impl Icon {
    #[cfg(test)]
    pub const ALL: [Icon; 23] = [
        Self::Bell,
        Self::Burger,
        Self::ChevronDown,
        Self::Close,
        Self::Gear,
        Self::Heart,
        Self::HeartOutline,
        Self::History,
        Self::Music,
        Self::Next,
        Self::Pause,
        Self::Play,
        Self::Plus,
        Self::Prev,
        Self::Queue,
        Self::Search,
        Self::Spinner,
        Self::Trash,
        Self::Users,
        Self::Volume,
        Self::WinMinimize,
        Self::WinMaximize,
        Self::WinRestore,
    ];

    pub fn path(self) -> &'static str {
        match self {
            Self::Bell => "icons/bell.svg",
            Self::Burger => "icons/burger.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::Close => "icons/close.svg",
            Self::Gear => "icons/gear.svg",
            Self::Heart => "icons/heart.svg",
            Self::HeartOutline => "icons/heart-outline.svg",
            Self::History => "icons/history.svg",
            Self::Music => "icons/music.svg",
            Self::Next => "icons/next.svg",
            Self::Pause => "icons/pause.svg",
            Self::Play => "icons/play.svg",
            Self::Plus => "icons/plus.svg",
            Self::Prev => "icons/prev.svg",
            Self::Queue => "icons/queue.svg",
            Self::Search => "icons/search.svg",
            Self::Spinner => "icons/spinner.svg",
            Self::Trash => "icons/trash.svg",
            Self::Users => "icons/users.svg",
            Self::Volume => "icons/volume.svg",
            Self::WinMinimize => "icons/win-minimize.svg",
            Self::WinMaximize => "icons/win-maximize.svg",
            Self::WinRestore => "icons/win-restore.svg",
        }
    }
}

/// A square icon of `size` px.
pub fn icon(icon: Icon, size: impl Into<Pixels>) -> Svg {
    let size = size.into();
    svg().path(icon.path()).size(size).flex_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Assets;
    use gpui::AssetSource;

    #[test]
    fn every_icon_asset_is_embedded() {
        for i in Icon::ALL {
            assert!(
                Assets.load(i.path()).unwrap().is_some(),
                "missing {}",
                i.path()
            );
        }
    }
}
