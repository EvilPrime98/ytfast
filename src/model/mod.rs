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
    pub upload_day: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub avatar_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Playlist {
    pub id: String,
    pub title: String,
    pub thumb_id: String,
    pub owner: String,
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

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn parse_upload_day(s: &str) -> Option<i64> {
    let digits: String = s.chars().take(10).filter(char::is_ascii_digit).collect();
    if digits.len() < 8 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| digits[r].parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(4..6)?, num(6..8)?);
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| days_from_civil(y, m, d))
}

pub fn format_age(upload_day: i64, today: i64) -> String {
    let days = (today - upload_day).max(0);
    let (n, unit) = match days {
        0 => return "today".into(),
        1..=6 => (days, "day"),
        7..=29 => (days / 7, "week"),
        30..=364 => (days / 30, "month"),
        _ => (days / 365, "year"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

pub fn today_day() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or_default()
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
    fn upload_days_parse_and_format() {
        assert_eq!(parse_upload_day("19700102"), Some(1));
        assert_eq!(parse_upload_day("2024-03-01T10:00:00+00:00"), Some(19783));
        assert_eq!(parse_upload_day("nope"), None);
        assert_eq!(format_age(10, 10), "today");
        assert_eq!(format_age(9, 10), "1 day ago");
        assert_eq!(format_age(0, 14), "2 weeks ago");
        assert_eq!(format_age(0, 400), "1 year ago");
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
