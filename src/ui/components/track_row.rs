//! One track in a list: thumbnail, title / channel, length and a few icon actions.
//! Clicking the thumbnail + title area plays the track.

use std::sync::Arc;

use gpui::{
    App, Image, InteractiveElement, IntoElement, ObjectFit, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, StyledImage as _, Window, div, img, px,
};

use crate::model::{Track, format_duration};
use crate::ui::icons::{Icon, icon};
use crate::ui::theme;

type ClickHandler = Box<dyn Fn(&mut Window, &mut App)>;

pub struct RowAction {
    pub icon: Icon,
    /// Drawn in the accent colour (e.g. a saved heart).
    pub active: bool,
    pub on_click: ClickHandler,
}

impl RowAction {
    pub fn new(icon: Icon, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Self {
            icon,
            active: false,
            on_click: Box::new(on_click),
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

/// A rounded thumbnail, or a placeholder tile while it loads.
pub fn thumbnail(image: Option<Arc<Image>>, width: f32, height: f32) -> impl IntoElement {
    let frame = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(width))
        .h(px(height))
        .rounded(px(6.0))
        .overflow_hidden()
        .bg(theme::bg_panel());
    match image {
        Some(image) => frame.child(
            img(image)
                .w(px(width))
                .h(px(height))
                .object_fit(ObjectFit::Cover),
        ),
        None => frame.child(icon(Icon::Music, px(height * 0.4)).text_color(theme::text_muted())),
    }
}

/// `prefix` + `ix` keep element ids unique within a list.
pub fn track_row(
    prefix: &'static str,
    ix: usize,
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
    div()
        .id(SharedString::from(format!("{prefix}-row-{ix}")))
        .flex()
        .items_center()
        .gap(px(12.0))
        .h(px(60.0))
        .px(px(10.0))
        .rounded(px(8.0))
        .hover(|s| s.bg(theme::bg_row_hover()))
        .child(
            div()
                .id(SharedString::from(format!("{prefix}-play-{ix}")))
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(12.0))
                .cursor_pointer()
                .on_click(move |_, window, cx| on_play(window, cx))
                .child(thumbnail(thumb, 80.0, 45.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(14.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(title_color)
                                .child(track.title.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.0))
                                .text_color(theme::text_muted())
                                .child(track.channel.clone()),
                        ),
                ),
        )
        .child(
            div()
                .flex_none()
                .w(px(52.0))
                .text_size(px(12.0))
                .text_color(theme::text_muted())
                .child(format_duration(track.duration_secs)),
        )
        .children(actions.into_iter().enumerate().map(|(n, action)| {
            let on_click = action.on_click;
            div()
                .id(SharedString::from(format!("{prefix}-act-{ix}-{n}")))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(32.0))
                .rounded_full()
                .cursor_pointer()
                .hover(|s| s.bg(theme::hover()))
                .on_click(move |_, window, cx| on_click(window, cx))
                .child(icon(action.icon, px(18.0)).text_color(if action.active {
                    theme::accent()
                } else {
                    theme::text_muted()
                }))
        }))
}
