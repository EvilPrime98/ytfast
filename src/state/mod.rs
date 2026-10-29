//! App state: one entity per data domain, shared through a gpui `Global`.

pub mod channels;
pub mod feed;
pub mod history;
pub mod library;
pub mod player;
pub mod playlists;
pub mod prefs;
pub mod search;
pub mod subscriptions;
pub mod thumbs;

use gpui::{App, AppContext as _, Entity, Global};

/// Handles to the shared entities. Stored as a gpui `Global` so rows and overlays can reach them
/// without threading them through every constructor.
#[derive(Clone)]
pub struct Stores {
    pub prefs: Entity<prefs::PrefsStore>,
    pub library: Entity<library::LibraryStore>,
    pub history: Entity<history::HistoryStore>,
    pub channels: Entity<channels::ChannelsStore>,
    pub playlists: Entity<playlists::PlaylistsStore>,
    pub search: Entity<search::SearchStore>,
    pub feed: Entity<feed::FeedStore>,
    pub subscriptions: Entity<subscriptions::SubscriptionsStore>,
    pub thumbs: Entity<thumbs::Thumbnails>,
    pub player: Entity<player::PlayerStore>,
}

impl Global for Stores {}

impl Stores {
    pub fn new(cx: &mut App) -> Self {
        let prefs = cx.new(|_| prefs::PrefsStore::load());
        let history = cx.new(|_| history::HistoryStore::load());
        let player = cx.new({
            let prefs = prefs.clone();
            let history = history.clone();
            |cx| player::PlayerStore::new(prefs, history, cx)
        });
        let channels = cx.new(|_| channels::ChannelsStore::load());
        channels.update(cx, |c, cx| c.start_polling(prefs.clone(), cx));
        cx.observe(&player, {
            let channels = channels.clone();
            move |player, cx| {
                let id = player.read(cx).current().map(|t| t.id.clone());
                if let Some(id) = id {
                    channels.update(cx, |c, cx| c.mark_seen(&id, cx));
                }
            }
        })
        .detach();
        let subscriptions = cx.new(|_| subscriptions::SubscriptionsStore::load());
        cx.observe(&channels, {
            let subscriptions = subscriptions.clone();
            move |channels, cx| {
                let ids = channels
                    .read(cx)
                    .favorites
                    .iter()
                    .map(|f| f.channel.id.clone())
                    .collect();
                subscriptions.update(cx, |s, cx| s.sync(ids, cx));
            }
        })
        .detach();
        Self {
            library: cx.new(|_| library::LibraryStore::load()),
            history,
            channels,
            playlists: cx.new(|_| playlists::PlaylistsStore::load()),
            search: cx.new(|_| search::SearchStore::default()),
            feed: cx.new(|_| feed::FeedStore::load()),
            subscriptions,
            thumbs: cx.new(|_| thumbs::Thumbnails::default()),
            player,
            prefs,
        }
    }
}
