//! Saved tracks, persisted in `<config dir>/YTFast/library.json`.

use std::path::PathBuf;

use crate::model::Track;

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("YTFast").join("library.json"))
}

#[derive(Default)]
pub struct LibraryStore {
    pub tracks: Vec<Track>,
    /// Bumped on every change so views can memoize.
    pub revision: u64,
}

impl LibraryStore {
    pub fn load() -> Self {
        let tracks = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            tracks,
            revision: 0,
        }
    }

    fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(&self.tracks) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&p, json) {
                    tracing::warn!("could not save the library to {}: {e}", p.display());
                }
            }
            Err(e) => tracing::warn!("could not serialize the library: {e}"),
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.tracks.iter().any(|t| t.id == id)
    }

    /// Save the track, or remove it when already saved. Returns whether it is saved now.
    pub fn toggle(&mut self, track: &Track, cx: &mut gpui::Context<Self>) -> bool {
        let saved = match self.tracks.iter().position(|t| t.id == track.id) {
            Some(ix) => {
                self.tracks.remove(ix);
                false
            }
            None => {
                self.tracks.insert(0, track.clone());
                true
            }
        };
        self.revision += 1;
        self.save();
        cx.notify();
        saved
    }
}
