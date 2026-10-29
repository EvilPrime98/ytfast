use std::path::PathBuf;

use gpui::Context;

use crate::model::Playlist;

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("YTFast").join("playlists.json"))
}

#[derive(Default)]
pub struct PlaylistsStore {
    pub playlists: Vec<Playlist>,
}

impl PlaylistsStore {
    pub fn load() -> Self {
        let playlists = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { playlists }
    }

    fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(&self.playlists) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&p, json) {
                    tracing::warn!("could not save the playlists to {}: {e}", p.display());
                }
            }
            Err(e) => tracing::warn!("could not serialize the playlists: {e}"),
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.save();
        cx.notify();
    }

    pub fn is_favorite(&self, id: &str) -> bool {
        self.playlists.iter().any(|p| p.id == id)
    }

    pub fn toggle(&mut self, playlist: &Playlist, cx: &mut Context<Self>) {
        self.flip(playlist);
        self.changed(cx);
    }

    pub fn refresh(&mut self, playlist: &Playlist, cx: &mut Context<Self>) {
        if self.update_snapshot(playlist) {
            self.changed(cx);
        }
    }

    fn flip(&mut self, playlist: &Playlist) {
        match self.playlists.iter().position(|p| p.id == playlist.id) {
            Some(ix) => {
                self.playlists.remove(ix);
            }
            None => self.playlists.insert(0, playlist.clone()),
        }
    }

    fn update_snapshot(&mut self, playlist: &Playlist) -> bool {
        let Some(stored) = self.playlists.iter_mut().find(|p| p.id == playlist.id) else {
            return false;
        };
        let mut next = stored.clone();
        if !playlist.title.is_empty() {
            next.title = playlist.title.clone();
        }
        if !playlist.thumb_id.is_empty() {
            next.thumb_id = playlist.thumb_id.clone();
        }
        if !playlist.owner.is_empty() {
            next.owner = playlist.owner.clone();
        }
        let changed = next != *stored;
        *stored = next;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playlist(id: &str) -> Playlist {
        Playlist {
            id: id.into(),
            title: format!("title {id}"),
            ..Playlist::default()
        }
    }

    #[test]
    fn toggle_adds_at_front_and_removes() {
        let mut s = PlaylistsStore::default();
        s.flip(&playlist("a"));
        s.flip(&playlist("b"));
        assert_eq!(s.playlists[0].id, "b");
        assert!(s.is_favorite("a"));
        s.flip(&playlist("a"));
        assert!(!s.is_favorite("a"));
        assert_eq!(s.playlists.len(), 1);
    }

    #[test]
    fn refresh_updates_only_when_favorited() {
        let mut s = PlaylistsStore::default();
        let mut p = playlist("a");
        assert!(!s.update_snapshot(&p));
        s.flip(&p);
        assert!(!s.update_snapshot(&p));
        p.owner = "Somebody".into();
        p.title = "renamed".into();
        assert!(s.update_snapshot(&p));
        assert_eq!(s.playlists[0].owner, "Somebody");
        assert_eq!(s.playlists[0].title, "renamed");
        p.owner.clear();
        assert!(!s.update_snapshot(&p));
        assert_eq!(s.playlists[0].owner, "Somebody");
    }

    #[test]
    fn json_round_trip() {
        let mut p = playlist("a");
        p.owner = "Somebody".into();
        let json = serde_json::to_string(&vec![p.clone()]).unwrap();
        assert!(json.contains("thumbId"));
        let back: Vec<Playlist> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, vec![p]);
    }
}
