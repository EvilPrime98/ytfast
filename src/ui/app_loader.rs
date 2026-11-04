//! `AppLoader`: full-window splash while the startup sequence runs. The play icon spins as the loading indicator.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, InteractiveElement, IntoElement, ParentElement, SharedString,
    Styled, Transformation, div, percentage, px, rgb,
};

use crate::ui::icons::{Icon, icon};
use crate::ui::theme;

const LOGO_SIZE: f32 = 88.0;

pub fn app_loader(message: impl Into<SharedString>) -> impl IntoElement {
    div()
        .id("app-loader")
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(14.0))
        .bg(theme::bg_chrome())
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(LOGO_SIZE))
                .rounded_full()
                .bg(theme::accent())
                .child(
                    icon(Icon::Play, px(LOGO_SIZE * 0.62))
                        .text_color(theme::on_accent())
                        .with_animation(
                            "loader-spin",
                            Animation::new(Duration::from_millis(1200)).repeat(),
                            |el, t| el.with_transformation(Transformation::rotate(percentage(t))),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .text_size(px(22.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child("YT")
                .child(div().text_color(theme::accent()).child("Fast")),
        )
        .child(
            div()
                .mt(px(8.0))
                .text_size(px(13.0))
                .text_color(rgb(0x8f8f8f))
                .child(message.into()),
        )
}
