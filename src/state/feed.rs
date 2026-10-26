//! The home feed: sections of suggested tracks built from the history and the library.
//!
//! YouTube's personalized feed is not reachable anonymously, so the sections come from seeded
//! searches and radio mixes. The last feed is cached on disk and reused until it is older than
//! [`TTL_SECS`]. A generation counter drops responses that were superseded by a newer refresh.

use std::collections::HashSet;
use std::path::PathBuf;

use futures_util::future::join_all;
use gpui::{Context, Task};
use serde::{Deserialize, Serialize};

use crate::api::ytdlp::{self, YtError};
use crate::model::Track;
use crate::runtime;
use crate::state::history::{HistoryEntry, now_secs};

const TTL_SECS: u64 = 6 * 3600;
const SECTION_LEN: usize = 12;
const FAVORITE_CHANNELS: usize = 2;

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("YTFast").join("feed.json"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedSection {
    pub title: String,
    pub tracks: Vec<Track>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Cache {
    fetched_at: u64,
    sections: Vec<FeedSection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    Related(String),
    Search(String),
    Trending,
}

#[derive(Debug, PartialEq, Eq)]
struct Plan {
    title: String,
    sources: Vec<Source>,
}

type Fetched = Vec<(String, Vec<Result<Vec<Track>, YtError>>)>;

#[derive(Default)]
pub struct FeedStore {
    pub sections: Vec<FeedSection>,
    pub loading: bool,
    /// Bumped whenever `sections` change.
    pub revision: u64,
    fetched_at: u64,
    generation: u64,
}

impl FeedStore {
    pub fn load() -> Self {
        let cache: Cache = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            sections: cache.sections,
            fetched_at: cache.fetched_at,
            ..Self::default()
        }
    }

    fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let cache = Cache {
            fetched_at: self.fetched_at,
            sections: self.sections.clone(),
        };
        match serde_json::to_string(&cache) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&p, json) {
                    tracing::warn!("could not save the feed to {}: {e}", p.display());
                }
            }
            Err(e) => tracing::warn!("could not serialize the feed: {e}"),
        }
    }

    fn fresh(&self) -> bool {
        !self.sections.is_empty() && now_secs().saturating_sub(self.fetched_at) < TTL_SECS
    }

    /// Rebuild the sections in the background unless the cached feed is still fresh. The old
    /// sections stay on screen until the new ones arrive.
    pub fn refresh(
        &mut self,
        bin: String,
        history: Vec<HistoryEntry>,
        library: Vec<Track>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        if self.loading || self.fresh() {
            return Task::ready(());
        }
        self.generation += 1;
        let generation = self.generation;
        self.loading = true;
        cx.notify();

        let played: HashSet<String> = history.iter().map(|e| e.track.id.clone()).collect();
        let plans = plan(&history, &library);
        cx.spawn(async move |this, cx| {
            let fetched: Fetched = runtime::run(async move {
                let sections = plans.iter().map(|p| async {
                    let results = join_all(p.sources.iter().map(|s| fetch(&bin, s))).await;
                    (p.title.clone(), results)
                });
                join_all(sections).await
            })
            .await;
            this.update(cx, |s, cx| {
                if generation != s.generation {
                    return;
                }
                s.loading = false;
                let sections = assemble(fetched, &played);
                if !sections.is_empty() {
                    s.sections = sections;
                    s.fetched_at = now_secs();
                    s.revision += 1;
                    s.save();
                }
                cx.notify();
            })
            .ok();
        })
    }
}

async fn fetch(bin: &str, source: &Source) -> Result<Vec<Track>, YtError> {
    match source {
        Source::Related(id) => ytdlp::related(bin, id).await,
        Source::Search(query) => ytdlp::search(bin, query, 10).await,
        Source::Trending => ytdlp::trending(bin).await,
    }
}

/// What to fetch: a radio mix for the latest track, the favorite channels, and the charts.
fn plan(history: &[HistoryEntry], library: &[Track]) -> Vec<Plan> {
    let mut plans = Vec::new();

    let seed = history
        .first()
        .map(|e| &e.track)
        .or_else(|| library.first());
    if let Some(seed) = seed {
        plans.push(Plan {
            title: format!("Because you listened to {}", seed.title),
            sources: vec![Source::Related(seed.id.clone())],
        });
    }

    let channels = favorite_channels(history.iter().map(|e| &e.track).chain(library));
    if !channels.is_empty() {
        plans.push(Plan {
            title: "From your favorite channels".into(),
            sources: channels
                .into_iter()
                .map(|c| Source::Search(format!("{c} new")))
                .collect(),
        });
    }

    plans.push(Plan {
        title: "Trending".into(),
        sources: vec![Source::Trending],
    });
    plans
}

/// The most frequent channels, ties broken by first appearance.
fn favorite_channels<'a>(tracks: impl Iterator<Item = &'a Track>) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for t in tracks.filter(|t| !t.channel.is_empty()) {
        match counts.iter_mut().find(|(c, _)| *c == t.channel) {
            Some((_, n)) => *n += 1,
            None => counts.push((t.channel.clone(), 1)),
        }
    }
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    counts
        .into_iter()
        .take(FAVORITE_CHANNELS)
        .map(|(c, _)| c)
        .collect()
}

/// Merge each section's sources round-robin, drop played and already shown tracks, and skip the
/// sections that failed or ended up empty.
fn assemble(fetched: Fetched, played: &HashSet<String>) -> Vec<FeedSection> {
    let mut seen = played.clone();
    let mut sections = Vec::new();
    for (title, results) in fetched {
        let lists: Vec<Vec<Track>> = results
            .into_iter()
            .filter_map(|r| {
                r.inspect_err(|e| tracing::warn!("feed section \"{title}\" failed: {e}"))
                    .ok()
            })
            .collect();
        let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
        let mut tracks = Vec::new();
        for i in 0..longest {
            for t in lists.iter().filter_map(|l| l.get(i)) {
                if tracks.len() < SECTION_LEN && seen.insert(t.id.clone()) {
                    tracks.push(t.clone());
                }
            }
        }
        if !tracks.is_empty() {
            sections.push(FeedSection { title, tracks });
        }
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, channel: &str) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            channel: channel.into(),
            ..Track::default()
        }
    }

    fn entry(id: &str, channel: &str) -> HistoryEntry {
        HistoryEntry {
            track: track(id, channel),
            played_at: 0,
        }
    }

    #[test]
    fn cold_start_only_asks_for_trending() {
        let p = plan(&[], &[]);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].sources, vec![Source::Trending]);
    }

    #[test]
    fn plan_seeds_from_history_and_library() {
        let history = [entry("a", "X"), entry("b", "Y"), entry("c", "X")];
        let library = [track("d", "Z")];
        let p = plan(&history, &library);
        assert_eq!(p[0].sources, vec![Source::Related("a".into())]);
        assert_eq!(
            p[1].sources,
            vec![
                Source::Search("X new".into()),
                Source::Search("Y new".into())
            ]
        );
        assert_eq!(p[2].sources, vec![Source::Trending]);
    }

    #[test]
    fn library_seeds_when_history_is_empty() {
        let p = plan(&[], &[track("d", "Z")]);
        assert_eq!(p[0].sources, vec![Source::Related("d".into())]);
    }

    #[test]
    fn assemble_dedupes_skips_played_and_failed() {
        let played: HashSet<String> = ["p".to_string()].into();
        let fetched: Fetched = vec![
            (
                "one".into(),
                vec![
                    Ok(vec![track("a", ""), track("p", ""), track("b", "")]),
                    Ok(vec![track("c", ""), track("a", "")]),
                    Err(YtError::NotFound),
                ],
            ),
            ("two".into(), vec![Ok(vec![track("a", "")])]),
            ("three".into(), vec![Err(YtError::NotFound)]),
        ];
        let s = assemble(fetched, &played);
        assert_eq!(s.len(), 1);
        let ids: Vec<_> = s[0].tracks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "c", "b"]);
    }
}
