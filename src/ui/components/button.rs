//! Text button: 34 px tall, 4 px radius, 13 px / 600.

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::ui::icons::{Icon, icon};
use crate::ui::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Outlined accent; the confirm button in dialogs.
    Classic,
    Secondary,
}

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<gpui::Div> {
    styled_button(id, None, label, variant, on_click)
}

pub fn icon_button(
    id: impl Into<ElementId>,
    glyph: Icon,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<gpui::Div> {
    styled_button(id, Some(glyph), label, variant, on_click)
}

fn styled_button(
    id: impl Into<ElementId>,
    glyph: Option<Icon>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<gpui::Div> {
    let base = div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .h(px(34.0))
        .min_w(px(96.0))
        .px(px(16.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(gpui::transparent_black())
        .text_size(px(13.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .cursor_pointer()
        .on_click(on_click)
        .children(glyph.map(|g| icon(g, px(16.0))))
        .child(label.into());
    match variant {
        ButtonVariant::Classic => base
            .text_color(theme::accent())
            .border_2()
            .border_color(theme::accent())
            .hover(|s| s.bg(theme::accent()).text_color(theme::on_accent()))
            .active(|s| s.bg(theme::accent()).text_color(theme::on_accent())),
        ButtonVariant::Secondary => base
            .bg(theme::bg_panel())
            .border_color(theme::border_subtle())
            .text_color(rgb(0xf2f2f2))
            .hover(|s| s.bg(rgb(0x2a2a2a)).border_color(gpui::rgba(0xffffff29)))
            .active(|s| s.bg(rgb(0x262626))),
    }
}
