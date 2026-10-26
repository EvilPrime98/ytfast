use std::path::PathBuf;
use std::time::Duration;

use gpui::{Context, Entity, Task};
use serde::{Deserialize, Serialize};

use crate::api::feeds;
use crate::model::{Channel, Track};
use crate::runtime;
use crate::state::prefs::PrefsStore;
use crate::ui::toast;

const MAX_SEEN: usize = 50;
const MAX_UNSEEN: usize = 50;
pub const UNAVAILABLE_AFTER: u32 = 5;
const POLL_GAP: Duration = Duration::from_secs(2);
const FIRST_POLL_DELAY: Duration = Duration::from_secs(20);

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("YTFast").join("channels.json"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Favorite {
    pub channel: Channel,
    pub notify: bool,
    pub baselined: bool,
    pub seen: Vec<String>,
    pub unseen: Vec<Track>,
    pub failures: u32,
}

impl Default for Favorite {
    fn default() -> Self {
        Self {
            channel: Channel::default(),
            notify: true,
            baselined: false,
            seen: Vec::new(),
            unseen: Vec::new(),
            failures: 0,
        }
    }
}

impl Favorite {
    pub fn unavailable(&self) -> bool {
        self.failures >= UNAVAILABLE_AFTER
    }

    fn apply_feed(&mut self, feed: &[Track]) -> Vec<Track> {
        self.failures = 0;
        if !self.baselined {
            self.baselined = true;
            self.seen = feed.iter().map(|t| t.id.clone()).collect();
            self.seen.truncate(MAX_SEEN);
            return Vec::new();
        }
        let fresh: Vec<Track> = feed
            .iter()
            .filter(|t| !self.seen.contains(&t.id) && !self.unseen.iter().any(|u| u.id == t.id))
            .cloned()
            .collect();
        for t in fresh.iter().rev() {
            self.unseen.insert(0, t.clone());
        }
        self.unseen.truncate(MAX_UNSEEN);
        fresh
    }

    fn mark_seen(&mut self, video_id: &str) -> bool {
        let Some(ix) = self.unseen.iter().position(|t| t.id == video_id) else {
            return false;
        };
        self.unseen.remove(ix);
        self.remember(video_id);
        true
    }

    fn mark_all_seen(&mut self) -> bool {
        let ids: Vec<String> = self.unseen.drain(..).map(|t| t.id).collect();
        for id in ids.iter().rev() {
            self.remember(id);
        }
        !ids.is_empty()
    }

    fn remember(&mut self, video_id: &str) {
        self.seen.retain(|s| s != video_id);
        self.seen.insert(0, video_id.to_string());
        self.seen.truncate(MAX_SEEN);
    }
}

#[derive(Default)]
pub struct ChannelsStore {
    pub favorites: Vec<Favorite>,
    pub checking: bool,
}

impl ChannelsStore {
    pub fn load() -> Self {
        let favorites = path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            favorites,
            checking: false,
        }
    }

    fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_string_pretty(&self.favorites) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&p, json) {
                    tracing::warn!("could not save the channels to {}: {e}", p.display());
                }
            }
            Err(e) => tracing::warn!("could not serialize the channels: {e}"),
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.save();
        cx.notify();
    }

    pub fn get(&self, id: &str) -> Option<&Favorite> {
        self.favorites.iter().find(|f| f.channel.id == id)
    }

    pub fn is_favorite(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    pub fn unseen_total(&self) -> usize {
        self.favorites.iter().map(|f| f.unseen.len()).sum()
    }

    pub fn unseen_videos(&self) -> Vec<Track> {
        self.favorites
            .iter()
            .flat_map(|f| f.unseen.iter().cloned())
            .collect()
    }

    pub fn toggle(&mut self, channel: &Channel, cx: &mut Context<Self>) {
        if let Some(ix) = self.favorites.iter().position(|f| f.channel.id == channel.id) {
            self.favorites.remove(ix);
            self.changed(cx);
            return;
        }
        self.favorites.push(Favorite {
            channel: channel.clone(),
            ..Favorite::default()
        });
        self.changed(cx);
        self.check(vec![channel.id.clone()], false, cx).detach();
    }

    pub fn set_notify(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        if let Some(f) = self.favorites.iter_mut().find(|f| f.channel.id == id) {
            f.notify = on;
            self.changed(cx);
        }
    }

    pub fn update_channel(&mut self, channel: &Channel, cx: &mut Context<Self>) {
        let Some(f) = self.favorites.iter_mut().find(|f| f.channel.id == channel.id) else {
            return;
        };
        let mut changed = false;
        if !channel.name.is_empty() && f.channel.name != channel.name {
            f.channel.name = channel.name.clone();
            changed = true;
        }
        if !channel.avatar_url.is_empty() && f.channel.avatar_url != channel.avatar_url {
            f.channel.avatar_url = channel.avatar_url.clone();
            changed = true;
        }
        if changed {
            self.changed(cx);
        }
    }

    pub fn mark_seen(&mut self, video_id: &str, cx: &mut Context<Self>) {
        let mut any = false;
        for f in &mut self.favorites {
            any |= f.mark_seen(video_id);
        }
        if any {
            self.changed(cx);
        }
    }

    pub fn mark_channel_seen(&mut self, id: &str, cx: &mut Context<Self>) {
        let hit = self
            .favorites
            .iter_mut()
            .find(|f| f.channel.id == id)
            .is_some_and(Favorite::mark_all_seen);
        if hit {
            self.changed(cx);
        }
    }

    pub fn mark_all_seen(&mut self, cx: &mut Context<Self>) {
        let mut any = false;
        for f in &mut self.favorites {
            any |= f.mark_all_seen();
        }
        if any {
            self.changed(cx);
        }
    }

    fn apply(
        &mut self,
        id: &str,
        result: Result<Vec<Track>, String>,
        cx: &mut Context<Self>,
    ) -> Option<(String, bool, Vec<Track>)> {
        let f = self.favorites.iter_mut().find(|f| f.channel.id == id)?;
        let out = match result {
            Ok(feed) => {
                let fresh = f.apply_feed(&feed);
                (f.channel.name.clone(), f.notify, fresh)
            }
            Err(e) => {
                f.failures += 1;
                tracing::warn!("polling {} failed ({}): {e}", f.channel.name, f.failures);
                (String::new(), false, Vec::new())
            }
        };
        self.changed(cx);
        Some(out)
    }

    pub fn check(&mut self, ids: Vec<String>, announce: bool, cx: &mut Context<Self>) -> Task<()> {
        if ids.is_empty() {
            return Task::ready(());
        }
        self.checking = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            for (n, id) in ids.iter().enumerate() {
                if n > 0 {
                    cx.background_executor().timer(POLL_GAP).await;
                }
                let target = id.clone();
                let result = runtime::run(async move {
                    feeds::latest(&target).await.map_err(|e| e.to_string())
                })
                .await;
                let id = id.clone();
                let found = this
                    .update(cx, |s, cx| s.apply(&id, result, cx))
                    .ok()
                    .flatten();
                if let Some((name, notify, fresh)) = found
                    && announce
                    && notify
                    && !fresh.is_empty()
                {
                    announce_new(cx, &name, &fresh).await;
                }
            }
            this.update(cx, |s, cx| {
                s.checking = false;
                cx.notify();
            })
            .ok();
        })
    }

    pub fn check_all(&mut self, announce: bool, cx: &mut Context<Self>) -> Task<()> {
        let ids = self.favorites.iter().map(|f| f.channel.id.clone()).collect();
        self.check(ids, announce, cx)
    }

    pub fn start_polling(&mut self, prefs: Entity<PrefsStore>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_POLL_DELAY).await;
            loop {
                let (on, minutes) = cx.update(|cx| {
                    let p = &prefs.read(cx).prefs;
                    (p.channel_notifications, p.channel_poll_minutes.max(1))
                });
                if on {
                    let task = this.update(cx, |s, cx| s.check_all(true, cx));
                    match task {
                        Ok(task) => task.await,
                        Err(_) => break,
                    }
                }
                cx.background_executor()
                    .timer(Duration::from_secs(minutes * 60))
                    .await;
            }
        })
        .detach();
    }
}

async fn announce_new(cx: &mut gpui::AsyncApp, channel: &str, fresh: &[Track]) {
    let (summary, body) = match fresh {
        [one] => (channel.to_string(), one.title.clone()),
        many => (
            channel.to_string(),
            format!("{} new videos, latest: {}", many.len(), many[0].title),
        ),
    };
    let toast_text = format!("{summary}: {body}");
    cx.update(|cx| toast::success(cx, toast_text));
    let _ = runtime::run(async move {
        tokio::task::spawn_blocking(move || {
            if let Err(e) = notify_rust::Notification::new()
                .appname("YTFast")
                .summary(&summary)
                .body(&body)
                .show()
            {
                tracing::warn!("could not show the notification: {e}");
            }
        })
        .await
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            ..Track::default()
        }
    }

    fn feed(ids: &[&str]) -> Vec<Track> {
        ids.iter().map(|i| track(i)).collect()
    }

    #[test]
    fn first_feed_is_the_baseline() {
        let mut f = Favorite::default();
        assert!(f.apply_feed(&feed(&["a", "b"])).is_empty());
        assert!(f.baselined);
        assert!(f.unseen.is_empty());
        assert_eq!(f.seen, ["a", "b"]);
    }

    #[test]
    fn later_feeds_report_only_new_videos() {
        let mut f = Favorite::default();
        f.apply_feed(&feed(&["a", "b"]));
        let fresh = f.apply_feed(&feed(&["c", "a", "b"]));
        assert_eq!(fresh.len(), 1);
        assert_eq!(f.unseen[0].id, "c");
        assert!(f.apply_feed(&feed(&["c", "a", "b"])).is_empty());
        assert_eq!(f.unseen.len(), 1);
    }

    #[test]
    fn newest_unseen_comes_first() {
        let mut f = Favorite::default();
        f.apply_feed(&feed(&["a"]));
        f.apply_feed(&feed(&["b", "a"]));
        f.apply_feed(&feed(&["c", "b", "a"]));
        let ids: Vec<_> = f.unseen.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["c", "b"]);
    }

    #[test]
    fn seen_videos_do_not_come_back() {
        let mut f = Favorite::default();
        f.apply_feed(&feed(&["a"]));
        f.apply_feed(&feed(&["b", "a"]));
        assert!(f.mark_seen("b"));
        assert!(!f.mark_seen("b"));
        assert!(f.apply_feed(&feed(&["b", "a"])).is_empty());
        assert!(f.unseen.is_empty());
    }

    #[test]
    fn mark_all_seen_clears_and_remembers() {
        let mut f = Favorite::default();
        f.apply_feed(&feed(&["a"]));
        f.apply_feed(&feed(&["c", "b", "a"]));
        assert!(f.mark_all_seen());
        assert!(f.unseen.is_empty());
        assert!(f.seen.contains(&"b".to_string()) && f.seen.contains(&"c".to_string()));
        assert!(!f.mark_all_seen());
    }

    #[test]
    fn seen_ids_are_capped() {
        let mut f = Favorite::default();
        for i in 0..(MAX_SEEN + 10) {
            f.remember(&i.to_string());
        }
        assert_eq!(f.seen.len(), MAX_SEEN);
        assert_eq!(f.seen[0], (MAX_SEEN + 9).to_string());
    }

    #[test]
    fn partial_json_defaults_to_notifying() {
        let f: Favorite = serde_json::from_str(r#"{"channel":{"id":"UC1"}}"#).unwrap();
        assert!(f.notify);
        assert!(!f.baselined);
    }
}
