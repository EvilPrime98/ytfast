//! One track in a grid: large thumbnail, title, channel and length, plus a few icon actions.
//! Clicking the thumbnail or the title plays the track.

use std::sync::Arc;

use gpui::{
    App, Image, InteractiveElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::model::{Track, format_duration};
use crate::ui::components::track_row::{RowAction, thumbnail};
use crate::ui::icons::icon;
use crate::ui::theme;

/// A card `width` pixels wide with a 16:9 thumbnail. `prefix` + `ix` keep element ids unique.
#[allow(clippy::too_many_arguments)]
pub fn video_card(
    prefix: &'static str,
    ix: usize,
    width: f32,
    track: &Track,
    thumb: Option<Arc<Image>>,
    current: bool,
    on_play: impl Fn(&mut Window, &mut App) + 'static,
    actions: Vec<RowAction>,
) -> Stateful<gpui::Div> {
    let title_color = if current {
        theme::accent()
    } else {
        theme::text()
    };
    let meta = match (track.channel.is_empty(), track.duration_secs) {
        (false, Some(_)) => format!(
            "{} · {}",
            track.channel,
            format_duration(track.duration_secs)
        ),
        (false, None) => track.channel.clone(),
        (true, _) => format_duration(track.duration_secs),
    };
    let inner = width - 12.0;
    div()
        .id(SharedString::from(format!("{prefix}-card-{ix}")))
        .flex()
        .flex_col()
        .flex_none()
        .w(px(width))
        .gap(px(8.0))
        .p(px(6.0))
        .rounded(px(10.0))
        .hover(|s| s.bg(theme::bg_row_hover()))
        .child(
            div()
                .id(SharedString::from(format!("{prefix}-card-play-{ix}")))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .cursor_pointer()
                .on_click(move |_, window, cx| on_play(window, cx))
                .child(thumbnail(thumb, inner, inner * 9.0 / 16.0))
                .child(
                    div()
                        .line_clamp(2)
                        .h(px(36.0))
                        .text_size(px(14.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(title_color)
                        .child(track.title.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.0))
                        .text_color(theme::text_muted())
                        .child(meta),
                )
                .children(actions.into_iter().enumerate().map(|(n, action)| {
                    let on_click = action.on_click;
                    div()
                        .id(SharedString::from(format!("{prefix}-card-act-{ix}-{n}")))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(28.0))
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::hover()))
                        .on_click(move |_, window, cx| on_click(window, cx))
                        .child(icon(action.icon, px(16.0)).text_color(if action.active {
                            theme::accent()
                        } else {
                            theme::text_muted()
                        }))
                })),
        )
}
