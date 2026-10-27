use std::cmp::Reverse;
use std::collections::HashSet;
use std::path::PathBuf;

use futures_util::StreamExt;
use futures_util::stream;
use gpui::{Context, Task};
use serde::{Deserialize, Serialize};

use crate::api::feeds;
use crate::model::Track;
use crate::runtime;
use crate::state::history::now_secs;

const TTL_SECS: u64 = 30 * 60;
const MAX_KEPT: usize = 200;
const CONCURRENCY: usize = 6;

fn path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("YTFast")
            .join("subscriptions.json"),
    )
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ChannelVideos {
    id: String,
    tracks: Vec<Track>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Cache {
    fetched_at: u64,
    channels: Vec<ChannelVideos>,
}

type Fetched = Vec<(String, Result<Vec<Track>, String>)>;

#[derive(Default)]
pub struct SubscriptionsStore {
    pub videos: Vec<Track>,
    pub loading: bool,
    pub revision: u64,
    channels: Vec<ChannelVideos>,
    fetched_at: u64,
    generation: u64,
}

impl SubscriptionsStore {
    pub fn load() -> Self {
        let cache: Cache = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut store = Self {
            channels: cache.channels,
            fetched_at: cache.fetched_at,
            ..Self::default()
        };
        let ids: Vec<String> = store.channels.iter().map(|c| c.id.clone()).collect();
        store.rebuild(&ids);
        store
    }

    fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let cache = Cache {
            fetched_at: self.fetched_at,
            channels: self.channels.clone(),
        };
        match serde_json::to_string(&cache) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&p, json) {
                    tracing::warn!("could not save the subscriptions to {}: {e}", p.display());
                }
            }
            Err(e) => tracing::warn!("could not serialize the subscriptions: {e}"),
        }
    }

    fn fresh(&self) -> bool {
        !self.channels.is_empty() && now_secs().saturating_sub(self.fetched_at) < TTL_SECS
    }

    fn rebuild(&mut self, favorites: &[String]) {
        self.channels.retain(|c| favorites.contains(&c.id));
        let mut seen = HashSet::new();
        let mut videos: Vec<Track> = favorites
            .iter()
            .filter_map(|id| self.channels.iter().find(|c| c.id == *id))
            .flat_map(|c| c.tracks.iter())
            .filter(|t| seen.insert(t.id.clone()))
            .cloned()
            .collect();
        videos.sort_by_key(|t| Reverse(t.upload_day));
        videos.truncate(MAX_KEPT);
        self.videos = videos;
        self.revision += 1;
    }

    pub fn sync(&mut self, favorites: Vec<String>, cx: &mut Context<Self>) {
        if self.reconcile(&favorites) {
            self.save();
            cx.notify();
        }
    }

    fn reconcile(&mut self, favorites: &[String]) -> bool {
        let removed = self.channels.iter().any(|c| !favorites.contains(&c.id));
        let added = favorites
            .iter()
            .any(|id| !self.channels.iter().any(|c| c.id == *id));
        if added {
            self.fetched_at = 0;
        }
        if removed {
            self.rebuild(favorites);
        }
        removed
    }

    fn apply(&mut self, fetched: Fetched) -> bool {
        let mut any = false;
        for (id, result) in fetched {
            match result {
                Ok(tracks) => {
                    any = true;
                    match self.channels.iter_mut().find(|c| c.id == id) {
                        Some(c) => c.tracks = tracks,
                        None => self.channels.push(ChannelVideos { id, tracks }),
                    }
                }
                Err(e) => tracing::warn!("subscriptions: fetching {id} failed: {e}"),
            }
        }
        any
    }

    pub fn refresh(
        &mut self,
        favorites: Vec<String>,
        force: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        if self.loading || favorites.is_empty() || (!force && self.fresh()) {
            return Task::ready(());
        }
        self.generation += 1;
        let generation = self.generation;
        self.loading = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let ids = favorites.clone();
            let fetched: Fetched = runtime::run(async move {
                stream::iter(ids)
                    .map(|id| async move {
                        let result = feeds::latest(&id).await.map_err(|e| e.to_string());
                        (id, result)
                    })
                    .buffered(CONCURRENCY)
                    .collect()
                    .await
            })
            .await;
            this.update(cx, |s, cx| {
                if generation != s.generation {
                    return;
                }
                s.loading = false;
                if s.apply(fetched) {
                    s.fetched_at = now_secs();
                }
                s.rebuild(&favorites);
                s.save();
                cx.notify();
            })
            .ok();
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, day: Option<i64>) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            upload_day: day,
            ..Track::default()
        }
    }

    fn store(channels: &[(&str, Vec<Track>)]) -> SubscriptionsStore {
        SubscriptionsStore {
            channels: channels
                .iter()
                .map(|(id, tracks)| ChannelVideos {
                    id: (*id).into(),
                    tracks: tracks.clone(),
                })
                .collect(),
            ..SubscriptionsStore::default()
        }
    }

    fn ids(s: &SubscriptionsStore) -> Vec<&str> {
        s.videos.iter().map(|t| t.id.as_str()).collect()
    }

    fn favorites(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|i| (*i).to_string()).collect()
    }

    #[test]
    fn newest_first_with_stable_ties() {
        let mut s = store(&[
            ("A", vec![track("a1", Some(5)), track("a2", Some(3))]),
            ("B", vec![track("b1", Some(5)), track("b2", Some(4))]),
        ]);
        s.rebuild(&favorites(&["A", "B"]));
        assert_eq!(ids(&s), ["a1", "b1", "b2", "a2"]);
    }

    #[test]
    fn ties_follow_the_favorites_order() {
        let mut s = store(&[
            ("A", vec![track("a1", Some(5))]),
            ("B", vec![track("b1", Some(5))]),
        ]);
        s.rebuild(&favorites(&["B", "A"]));
        assert_eq!(ids(&s), ["b1", "a1"]);
    }

    #[test]
    fn dedupes_across_channels() {
        let mut s = store(&[
            ("A", vec![track("x", Some(2))]),
            ("B", vec![track("x", Some(2)), track("y", Some(1))]),
        ]);
        s.rebuild(&favorites(&["A", "B"]));
        assert_eq!(ids(&s), ["x", "y"]);
    }

    #[test]
    fn unknown_upload_day_sorts_last() {
        let mut s = store(&[("A", vec![track("n", None), track("d", Some(1))])]);
        s.rebuild(&favorites(&["A"]));
        assert_eq!(ids(&s), ["d", "n"]);
    }

    #[test]
    fn failed_channel_keeps_its_cached_videos() {
        let mut s = store(&[
            ("A", vec![track("old-a", Some(1))]),
            ("B", vec![track("old-b", Some(1))]),
        ]);
        let any = s.apply(vec![
            ("A".into(), Err("boom".into())),
            ("B".into(), Ok(vec![track("new-b", Some(2))])),
        ]);
        assert!(any);
        s.rebuild(&favorites(&["A", "B"]));
        assert_eq!(ids(&s), ["new-b", "old-a"]);
    }

    #[test]
    fn all_channels_failing_reports_no_success() {
        let mut s = store(&[("A", vec![track("a", Some(1))])]);
        assert!(!s.apply(vec![("A".into(), Err("boom".into()))]));
    }

    #[test]
    fn rebuild_drops_a_removed_favorite() {
        let mut s = store(&[
            ("A", vec![track("a", Some(1))]),
            ("B", vec![track("b", Some(2))]),
        ]);
        s.rebuild(&favorites(&["A"]));
        assert_eq!(ids(&s), ["a"]);
        assert_eq!(s.channels.len(), 1);
    }

    #[test]
    fn a_new_favorite_makes_the_cache_stale() {
        let mut s = store(&[("A", vec![track("a", Some(1))])]);
        s.fetched_at = now_secs();
        assert!(s.fresh());
        assert!(!s.reconcile(&favorites(&["A", "B"])));
        assert!(!s.fresh());
    }

    #[test]
    fn an_unchanged_set_is_a_no_op() {
        let mut s = store(&[("A", vec![track("a", Some(1))])]);
        s.fetched_at = now_secs();
        let revision = s.revision;
        assert!(!s.reconcile(&favorites(&["A"])));
        assert!(s.fresh());
        assert_eq!(s.revision, revision);
    }

    #[test]
    fn a_fetched_channel_is_added() {
        let mut s = SubscriptionsStore::default();
        assert!(s.apply(vec![("A".into(), Ok(vec![track("a", Some(1))]))]));
        s.rebuild(&favorites(&["A"]));
        assert_eq!(ids(&s), ["a"]);
    }

    #[test]
    fn fresh_needs_a_recent_fetch_of_something() {
        let mut s = store(&[("A", vec![])]);
        assert!(!s.fresh());
        s.fetched_at = now_secs();
        assert!(s.fresh());
        s.fetched_at = 0;
        assert!(!s.fresh());
        assert!(!SubscriptionsStore::default().fresh());
    }
}
