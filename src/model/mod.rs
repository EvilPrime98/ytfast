//! Plain data types shared by the stores, the backend and the UI.

use serde::{Deserialize, Serialize};

/// A playable YouTube video.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Track {
    /// The 11-character YouTube video id.
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration_secs: Option<u64>,
}

/// `3:07`, `1:02:09`; `--:--` when unknown.
pub fn format_duration(secs: Option<u64>) -> String {
    let Some(s) = secs else {
        return "--:--".into();
    };
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Whether the text the user typed is a link rather than a search query.
pub fn looks_like_url(input: &str) -> bool {
    let t = input.trim();
    t.starts_with("http://") || t.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_format() {
        assert_eq!(format_duration(Some(187)), "3:07");
        assert_eq!(format_duration(Some(3729)), "1:02:09");
        assert_eq!(format_duration(Some(0)), "0:00");
        assert_eq!(format_duration(None), "--:--");
    }

    #[test]
    fn urls_are_detected() {
        assert!(looks_like_url(" https://youtu.be/dQw4w9WgXcQ "));
        assert!(!looks_like_url("never gonna give you up"));
    }

    #[test]
    fn track_partial_json_uses_defaults() {
        let t: Track = serde_json::from_str(r#"{"id":"abc","title":"T"}"#).unwrap();
        assert_eq!(t.id, "abc");
        assert_eq!(t.channel, "");
        assert_eq!(t.duration_secs, None);
    }
}
