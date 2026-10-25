use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::model::Track;

const MAX_ENTRIES: usize = 200;

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("YTFast").join("history.json"))
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub track: Track,
    pub played_at: u64,
}

#[derive(Default)]
pub struct HistoryStore {
    pub entries: Vec<HistoryEntry>,
}

impl HistoryStore {
    pub fn load() -> Self {
        let entries = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { entries }
    }

    fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(&self.entries) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&p, json) {
                    tracing::warn!("could not save the history to {}: {e}", p.display());
                }
            }
            Err(e) => tracing::warn!("could not serialize the history: {e}"),
        }
    }

    fn push(&mut self, track: &Track, played_at: u64) {
        if self.entries.first().is_some_and(|e| e.track.id == track.id) {
            self.entries.remove(0);
        }
        self.entries.insert(
            0,
            HistoryEntry {
                track: track.clone(),
                played_at,
            },
        );
        self.entries.truncate(MAX_ENTRIES);
    }

    pub fn record(&mut self, track: &Track, cx: &mut gpui::Context<Self>) {
        self.push(track, now_secs());
        self.changed(cx);
    }

    pub fn remove(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        if ix < self.entries.len() {
            self.entries.remove(ix);
            self.changed(cx);
        }
    }

    pub fn clear(&mut self, cx: &mut gpui::Context<Self>) {
        self.entries.clear();
        self.changed(cx);
    }

    fn changed(&mut self, cx: &mut gpui::Context<Self>) {
        self.save();
        cx.notify();
    }
}

pub fn relative_time(played_at: u64, now: u64) -> String {
    let secs = now.saturating_sub(played_at);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            id: id.into(),
            ..Track::default()
        }
    }

    #[test]
    fn newest_entry_goes_first() {
        let mut h = HistoryStore::default();
        h.push(&track("a"), 1);
        h.push(&track("b"), 2);
        assert_eq!(h.entries[0].track.id, "b");
        assert_eq!(h.entries.len(), 2);
    }

    #[test]
    fn consecutive_repeats_collapse() {
        let mut h = HistoryStore::default();
        h.push(&track("a"), 1);
        h.push(&track("a"), 2);
        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.entries[0].played_at, 2);
    }

    #[test]
    fn non_consecutive_repeats_are_kept() {
        let mut h = HistoryStore::default();
        h.push(&track("a"), 1);
        h.push(&track("b"), 2);
        h.push(&track("a"), 3);
        assert_eq!(h.entries.len(), 3);
    }

    #[test]
    fn history_is_capped() {
        let mut h = HistoryStore::default();
        for i in 0..(MAX_ENTRIES + 20) {
            h.push(&track(&i.to_string()), i as u64);
        }
        assert_eq!(h.entries.len(), MAX_ENTRIES);
        assert_eq!(h.entries[0].track.id, (MAX_ENTRIES + 19).to_string());
    }

    #[test]
    fn relative_times_format() {
        assert_eq!(relative_time(100, 130), "just now");
        assert_eq!(relative_time(0, 300), "5m ago");
        assert_eq!(relative_time(0, 7200), "2h ago");
        assert_eq!(relative_time(0, 172800), "2d ago");
    }
}
