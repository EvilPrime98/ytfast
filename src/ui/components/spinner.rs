use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, IntoElement, ParentElement, Styled, Transformation, div,
    percentage, px,
};

use crate::ui::icons::{Icon, icon};
use crate::ui::theme;

const SIZE: f32 = 32.0;

pub fn spinner() -> impl IntoElement {
    icon(Icon::Spinner, px(SIZE))
        .text_color(theme::accent())
        .with_animation(
            "spinner",
            Animation::new(Duration::from_millis(900)).repeat(),
            |el, t| el.with_transformation(Transformation::rotate(percentage(t))),
        )
}

pub fn spinner_fill() -> impl IntoElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .child(spinner())
}
