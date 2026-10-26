//! One track in a grid: large thumbnail, title, channel and length, plus a few icon actions.
//! Clicking the thumbnail or the title plays the track.

use std::sync::Arc;

use gpui::{
    App, Image, InteractiveElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::model::{Track, format_age, format_duration, today_day};
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
    let mut parts = Vec::new();
    if !track.channel.is_empty() {
        parts.push(track.channel.clone());
    }
    if track.duration_secs.is_some() || parts.is_empty() {
        parts.push(format_duration(track.duration_secs));
    }
    if let Some(day) = track.upload_day {
        parts.push(format_age(day, today_day()));
    }
    let meta = parts.join(" · ");
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
                        .text_ellipsis()
                        .line_clamp(2)
                        .h(px(36.0))
                        .text_size(px(14.0))
                        .line_height(px(18.0))
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

pub const GRID_GAP: f32 = 16.0;
pub const GRID_PAD: f32 = 24.0;
const MIN_CARD: f32 = 220.0;

pub fn grid_metrics(window: &Window, sidebar_collapsed: bool) -> (usize, f32) {
    let sidebar = if sidebar_collapsed {
        0.0
    } else {
        f32::from(theme::sidebar_width())
    };
    let avail = (f32::from(window.viewport_size().width) - sidebar - GRID_PAD * 2.0).max(MIN_CARD);
    let cols = (((avail + GRID_GAP) / (MIN_CARD + GRID_GAP)) as usize).max(1);
    let width = ((avail - GRID_GAP * (cols - 1) as f32) / cols as f32 - 1.0).floor();
    (cols, width)
}

pub fn avatar(image: Option<Arc<Image>>, size: f32) -> impl gpui::IntoElement {
    use gpui::{ObjectFit, StyledImage as _, img};
    let frame = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded_full()
        .overflow_hidden()
        .bg(theme::bg_panel());
    match image {
        Some(image) => frame.child(img(image).size(px(size)).object_fit(ObjectFit::Cover)),
        None => frame.child(
            icon(crate::ui::icons::Icon::Users, size * 0.45).text_color(theme::text_muted()),
        ),
    }
}
