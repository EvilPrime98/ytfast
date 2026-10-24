//! Stateless dropdown. The caller owns the open flag (`on_open_change` is told the new value),
//! so it works from plain render functions.

use std::rc::Rc;

use gpui::{
    App, InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, Window, deferred, div, prelude::*, px, rgb, rgba,
};

use crate::ui::icons::{Icon, icon};
use crate::ui::theme;

pub fn dropdown<V: Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(V, SharedString)>,
    current: V,
    open: bool,
    align_right: bool,
    on_open_change: impl Fn(bool, &mut App) + 'static,
    on_select: impl Fn(V, &mut App) + 'static,
) -> impl IntoElement {
    let on_open_change = Rc::new(on_open_change);
    let on_select = Rc::new(on_select);
    let current_label = options
        .iter()
        .find(|(v, _)| *v == current)
        .map(|(_, l)| l.clone())
        .unwrap_or_default();

    let toggle = on_open_change.clone();
    let close = on_open_change.clone();
    div()
        .relative()
        .child(
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(if open {
                    theme::accent()
                } else {
                    rgba(0xffffff1f)
                })
                .bg(rgba(0xffffff0a))
                .text_size(px(13.0))
                .text_color(rgb(0xd8d8d8))
                .cursor_pointer()
                .hover(|s| s.bg(rgba(0xffffff0f)))
                .on_click(move |_, _, cx| toggle(!open, cx))
                .child(current_label)
                .child(icon(Icon::ChevronDown, px(12.0)).text_color(rgb(0xd8d8d8))),
        )
        .when(open, |s| {
            // Click-away backdrop below the menu.
            s.child(
                deferred(
                    div()
                        .absolute()
                        .top(px(-3000.0))
                        .left(px(-3000.0))
                        .w(px(8000.0))
                        .h(px(8000.0))
                        .occlude()
                        .on_mouse_down(gpui::MouseButton::Left, move |_, _: &mut Window, cx| {
                            close(false, cx)
                        }),
                )
                .with_priority(1),
            )
            .child(
                deferred(
                    div()
                        .absolute()
                        .top(px(38.0))
                        .when(align_right, |s| s.right_0())
                        .when(!align_right, |s| s.left_0())
                        .min_w(px(150.0))
                        .occlude()
                        .flex()
                        .flex_col()
                        .p(px(4.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(rgba(0xffffff24))
                        .bg(theme::bg_modal())
                        .shadow_lg()
                        .children(options.into_iter().enumerate().map(|(ix, (value, label))| {
                            let on_select = on_select.clone();
                            let on_open_change = on_open_change.clone();
                            div()
                                .id(SharedString::from(format!("{id}-{ix}")))
                                .px(px(10.0))
                                .py(px(7.0))
                                .rounded(px(4.0))
                                .cursor_pointer()
                                .text_size(px(13.0))
                                .text_color(if value == current {
                                    theme::accent()
                                } else {
                                    rgb(0xd8d8d8)
                                })
                                .hover(|s| s.bg(rgba(0xffffff0f)))
                                .on_click(move |_, _, cx| {
                                    on_open_change(false, cx);
                                    on_select(value, cx);
                                })
                                .child(label)
                        })),
                )
                .with_priority(2),
            )
        })
}
