use std::sync::Arc;

use gpui::{
    Image, InteractiveElement, MouseButton, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window, div, hsla, prelude::FluentBuilder as _, px,
};

use crate::model::Playlist;
use crate::ui::components::track_row::thumbnail;
use crate::ui::icons::{Icon, icon};
use crate::ui::theme;

pub fn playlist_card(
    ix: usize,
    width: f32,
    playlist: &Playlist,
    thumb: Option<Arc<Image>>,
    favorite: bool,
    on_open: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    on_toggle: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Stateful<gpui::Div> {
    let inner = width - 12.0;
    let group = SharedString::from(format!("playlist-card-group-{ix}"));
    let heart = div()
        .id(SharedString::from(format!("playlist-card-heart-{ix}")))
        .absolute()
        .top(px(6.0))
        .right(px(6.0))
        .flex()
        .items_center()
        .justify_center()
        .size(px(30.0))
        .rounded_full()
        .bg(hsla(0.0, 0.0, 0.0, 0.6))
        .cursor_pointer()
        .hover(|s| s.bg(hsla(0.0, 0.0, 0.0, 0.8)))
        .when(!favorite, |s| {
            s.invisible().group_hover(group.clone(), |s| s.visible())
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |ev, window, cx| {
            cx.stop_propagation();
            on_toggle(ev, window, cx);
        })
        .child(
            icon(
                if favorite {
                    Icon::Heart
                } else {
                    Icon::HeartOutline
                },
                px(18.0),
            )
            .text_color(if favorite {
                theme::accent()
            } else {
                theme::text()
            }),
        );
    div()
        .id(SharedString::from(format!("playlist-card-{ix}")))
        .group(group)
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
        .child(
            div()
                .relative()
                .child(thumbnail(thumb, inner, inner * 9.0 / 16.0))
                .child(heart),
        )
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
