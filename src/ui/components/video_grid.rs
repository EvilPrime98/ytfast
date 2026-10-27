use gpui::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, ScrollWheelEvent,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::*, px,
};

use crate::model::Track;
use crate::state::Stores;
use crate::ui::components::button::{ButtonVariant, button};
use crate::ui::components::smooth_scroll::SmoothScroll;
use crate::ui::components::track_row::RowAction;
use crate::ui::components::video_card::{GRID_GAP, video_card};
use crate::ui::icons::Icon;
use crate::ui::toast;

#[allow(clippy::too_many_arguments)]
pub fn video_grid(
    prefix: &'static str,
    stores: &Stores,
    tracks: &[Track],
    card_w: f32,
    scroll: &SmoothScroll,
    more: Option<bool>,
    on_wheel: impl Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static,
    on_more: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let playing_id = stores.player.read(cx).current().map(|t| t.id.clone());
    let cards: Vec<_> = tracks
        .iter()
        .enumerate()
        .map(|(ix, track)| {
            let thumb = stores.thumbs.read(cx).get(&track.id);
            let saved = stores.library.read(cx).contains(&track.id);
            let (play_stores, all) = (stores.clone(), tracks.to_vec());
            let (queue_track, save_track) = (track.clone(), track.clone());
            let (queue_stores, save_stores) = (stores.clone(), stores.clone());
            video_card(
                prefix,
                ix,
                card_w,
                track,
                thumb,
                playing_id.as_deref() == Some(track.id.as_str()),
                move |_, cx| {
                    play_stores
                        .player
                        .update(cx, |p, cx| p.play_tracks(all.clone(), ix, cx))
                },
                vec![
                    RowAction::new(Icon::Plus, move |_, cx| {
                        queue_stores
                            .player
                            .update(cx, |p, cx| p.enqueue(queue_track.clone(), cx));
                        toast::success(cx, "Added to queue");
                    }),
                    RowAction::new(
                        if saved {
                            Icon::Heart
                        } else {
                            Icon::HeartOutline
                        },
                        move |_, cx| {
                            save_stores
                                .library
                                .update(cx, |l, cx| l.toggle(&save_track, cx));
                        },
                    )
                    .active(saved),
                ],
            )
        })
        .collect();

    div()
        .id(SharedString::from(format!("{prefix}-videos")))
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&scroll.handle)
        .on_scroll_wheel(on_wheel)
        .gap(px(16.0))
        .child(div().flex().flex_wrap().gap(px(GRID_GAP)).children(cards))
        .when_some(more, |s, loading| {
            s.child(div().flex().justify_center().pb(px(8.0)).child(button(
                SharedString::from(format!("{prefix}-more")),
                if loading { "Loading…" } else { "Load more" },
                ButtonVariant::Secondary,
                on_more,
            )))
        })
}
