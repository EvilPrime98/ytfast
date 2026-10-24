//! Search results. A generation counter drops responses that were superseded by a newer query.

use gpui::{Context, Task};

use crate::api::ytdlp;
use crate::model::{Track, looks_like_url};
use crate::runtime;
use crate::ui::toast;

#[derive(Default)]
pub struct SearchStore {
    pub query: String,
    pub results: Vec<Track>,
    pub loading: bool,
    /// Something was searched for at least once (to tell "no results" from "nothing yet").
    pub searched: bool,
    /// Bumped whenever `results` change.
    pub revision: u64,
    generation: u64,
}

impl SearchStore {
    /// A pasted link is resolved (video or playlist); anything else is a search query.
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
        self.loading = true;
        self.searched = true;
        cx.notify();

        let limit = limit.max(1);
        cx.spawn(async move |this, cx| {
            let result = runtime::run(async move {
                if looks_like_url(&query) {
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
                    s.loading = false;
                    let error = match result {
                        Ok(tracks) => {
                            s.results = tracks;
                            s.revision += 1;
                            None
                        }
                        Err(e) => {
                            s.results.clear();
                            s.revision += 1;
                            Some(e.to_string())
                        }
                    };
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
    }
}
