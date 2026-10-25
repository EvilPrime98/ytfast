//! App state: one entity per data domain, shared through a gpui `Global`.

pub mod history;
pub mod library;
pub mod player;
pub mod prefs;
pub mod search;
pub mod thumbs;

use gpui::{App, AppContext as _, Entity, Global};

/// Handles to the shared entities. Stored as a gpui `Global` so rows and overlays can reach them
/// without threading them through every constructor.
#[derive(Clone)]
pub struct Stores {
    pub prefs: Entity<prefs::PrefsStore>,
    pub library: Entity<library::LibraryStore>,
    pub history: Entity<history::HistoryStore>,
    pub search: Entity<search::SearchStore>,
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
        Self {
            library: cx.new(|_| library::LibraryStore::load()),
            history,
            search: cx.new(|_| search::SearchStore::default()),
            thumbs: cx.new(|_| thumbs::Thumbnails::default()),
            player,
            prefs,
        }
    }
}
