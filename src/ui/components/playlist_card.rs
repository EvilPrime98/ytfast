use std::sync::Arc;

use gpui::{
    Image, InteractiveElement, ParentElement, SharedString, Stateful, StatefulInteractiveElement,
    Styled, Window, div, px,
};

use crate::model::Playlist;
use crate::ui::components::track_row::thumbnail;
use crate::ui::theme;

pub fn playlist_card(
    ix: usize,
    width: f32,
    playlist: &Playlist,
    thumb: Option<Arc<Image>>,
    on_open: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Stateful<gpui::Div> {
    let inner = width - 12.0;
    div()
        .id(SharedString::from(format!("playlist-card-{ix}")))
        .flex()
        .flex_col()
        .flex_none()
        .w(px(width))
        .gap(px(8.0))
        .p(px(6.0))
        .rounded(px(10.0))
        .cursor_pointer()
        .hover(|s| s.bg(theme::bg_row_hover()))
        .on_click(on_open)
        .child(thumbnail(thumb, inner, inner * 9.0 / 16.0))
        .child(
            div()
                .text_ellipsis()
                .line_clamp(2)
                .h(px(36.0))
                .text_size(px(14.0))
                .line_height(px(18.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme::text())
                .child(playlist.title.clone()),
        )
}
