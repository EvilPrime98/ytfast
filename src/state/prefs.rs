//! User preferences. Client-only, persisted in `<config dir>/YTFast/prefs.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Prefs {
    /// Output volume, 0.0..=1.0.
    pub volume: f32,
    pub speed: f32,
    /// Path of the `yt-dlp` binary; empty means "find it on PATH".
    pub ytdlp_path: String,
    /// Play the next queued track when one ends.
    pub autoplay_next: bool,
    /// Results requested per search.
    pub search_limit: u32,
    /// Downloaded audio kept on disk, in megabytes.
    pub cache_limit_mb: u64,
    pub sidebar_collapsed: bool,
    /// Keep playing from the tray when the window is closed.
    pub keep_running_in_background: bool,
    pub channel_notifications: bool,
    pub channel_poll_minutes: u64,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            volume: 0.8,
            speed: 1.0,
            ytdlp_path: String::new(),
            autoplay_next: true,
            search_limit: 15,
            cache_limit_mb: 1024,
            sidebar_collapsed: false,
            keep_running_in_background: true,
            channel_notifications: true,
            channel_poll_minutes: 30,
        }
    }
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("YTFast").join("prefs.json"))
}

impl Prefs {
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Best effort: losing a preference is not worth an error dialog.
    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            if let Err(e) = std::fs::write(&p, json) {
                tracing::warn!("could not save prefs to {}: {e}", p.display());
            }
        }
    }
}

/// Entity wrapper: persisted [`Prefs`] plus session-only UI state.
pub struct PrefsStore {
    pub prefs: Prefs,
    /// Id of the dropdown that is open, if any (one at a time).
    pub open_menu: Option<&'static str>,
}

impl PrefsStore {
    pub fn load() -> Self {
        Self {
            prefs: Prefs::load(),
            open_menu: None,
        }
    }

    /// Mutate a persisted preference and write it out.
    pub fn update(&mut self, cx: &mut gpui::Context<Self>, change: impl FnOnce(&mut Prefs)) {
        change(&mut self.prefs);
        self.prefs.save();
        cx.notify();
    }

    pub fn set_menu_open(&mut self, id: &'static str, open: bool, cx: &mut gpui::Context<Self>) {
        if open {
            self.open_menu = Some(id);
        } else if self.open_menu == Some(id) {
            self.open_menu = None;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_files_fall_back_to_defaults() {
        let p: Prefs = serde_json::from_str(r#"{"volume":0.3}"#).unwrap();
        assert_eq!(p.volume, 0.3);
        assert!(p.autoplay_next);
        assert_eq!(p.search_limit, 15);
        assert!(p.ytdlp_path.is_empty());
    }
}
