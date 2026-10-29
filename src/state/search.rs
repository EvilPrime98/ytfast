//! Search results. A generation counter drops responses that were superseded by a newer query.

use std::future::Future;

use gpui::{Context, Task};

use crate::api::ytdlp;
use crate::model::{Channel, Playlist, Track, looks_like_url};
use crate::runtime;
use crate::ui::toast;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchMode {
    #[default]
    Videos,
    Channels,
    Playlists,
}

#[derive(Default)]
pub struct SearchStore {
    pub mode: SearchMode,
    pub channels: Vec<Channel>,
    pub playlists: Vec<Playlist>,
    pub query: String,
    pub results: Vec<Track>,
    pub loading: bool,
    /// Something was searched for at least once (to tell "no results" from "nothing yet").
    pub searched: bool,
    /// Bumped whenever `results` change.
    pub revision: u64,
    generation: u64,
    pending: u8,
}

impl SearchStore {
    pub fn set_mode(&mut self, mode: SearchMode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            cx.notify();
        }
    }

    pub fn run(
        &mut self,
        query: String,
        bin: String,
        limit: u32,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let query = query.trim().to_string();
        if query.is_empty() {
            return Task::ready(());
        }
        self.generation += 1;
        let generation = self.generation;
        self.query = query.clone();
        self.searched = true;
        self.results.clear();
        self.channels.clear();
        self.playlists.clear();
        self.revision += 1;
        let is_link = looks_like_url(&query);
        self.pending = if is_link { 1 } else { 3 };
        self.loading = true;
        cx.notify();

        let limit = limit.max(1);
        let videos = {
            let (bin, query) = (bin.clone(), query.clone());
            cx.spawn(async move |this, cx| {
                let result = runtime::run(async move {
                    if is_link {
                        ytdlp::resolve(&bin, &query).await
                    } else {
                        ytdlp::search(&bin, &query, limit).await
                    }
                })
                .await;
                let error = this
                    .update(cx, |s, cx| {
                        if generation != s.generation {
                            return None;
                        }
                        let error = match result {
                            Ok(tracks) => {
                                s.results = tracks;
                                None
                            }
                            Err(e) => Some(e.to_string()),
                        };
                        s.revision += 1;
                        s.finish_one();
                        cx.notify();
                        error
                    })
                    .ok()
                    .flatten();
                if let Some(e) = error {
                    tracing::warn!("search failed: {e}");
                    cx.update(|cx| toast::error(cx, e));
                }
            })
        };
        if is_link {
            return videos;
        }
        let channels = {
            let (bin, query) = (bin.clone(), query.clone());
            self.spawn_list(
                "channel",
                generation,
                async move { ytdlp::search_channels(&bin, &query, limit).await },
                |s, channels| s.channels = channels,
                cx,
            )
        };
        let playlists = self.spawn_list(
            "playlist",
            generation,
            async move { ytdlp::search_playlists(&bin, &query, limit).await },
            |s, playlists| s.playlists = playlists,
            cx,
        );
        cx.spawn(async move |_, _| {
            videos.await;
            channels.await;
            playlists.await;
        })
    }

    fn spawn_list<T: Send + 'static>(
        &self,
        what: &'static str,
        generation: u64,
        fetch: impl Future<Output = Result<Vec<T>, ytdlp::YtError>> + Send + 'static,
        store: fn(&mut Self, Vec<T>),
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let result = runtime::run(fetch).await;
            let error = this
                .update(cx, |s, cx| {
                    if generation != s.generation {
                        return None;
                    }
                    let error = match result {
                        Ok(items) => {
                            store(s, items);
                            None
                        }
                        Err(e) => Some(e.to_string()),
                    };
                    s.finish_one();
                    cx.notify();
                    error
                })
                .ok()
                .flatten();
            if let Some(e) = error {
                tracing::warn!("{what} search failed: {e}");
                cx.update(|cx| toast::error(cx, e));
            }
        })
    }

    fn finish_one(&mut self) {
        self.pending = self.pending.saturating_sub(1);
        self.loading = self.pending > 0;
    }
}
