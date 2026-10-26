//! `yt-dlp` as the YouTube backend: search, link resolution and audio download.
//!
//! The binary is looked up on `PATH` unless the user points the settings at a specific file.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::Value;
use tokio::process::Command;

use crate::model::Track;

#[derive(Debug, thiserror::Error)]
pub enum YtError {
    #[error("yt-dlp was not found. Install it or set its path in Settings.")]
    NotFound,
    #[error("{0}")]
    Failed(String),
    #[error("yt-dlp returned data this app cannot read: {0}")]
    Parse(String),
}

fn command(bin: &str) -> Command {
    let bin = bin.trim();
    let mut cmd = Command::new(if bin.is_empty() { "yt-dlp" } else { bin });
    cmd.stdin(Stdio::null()).kill_on_drop(true);
    // No console window flashing up behind the GUI.
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    cmd
}

async fn run(mut cmd: Command) -> Result<String, YtError> {
    let out = cmd.output().await.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => YtError::NotFound,
        _ => YtError::Failed(e.to_string()),
    })?;
    if !out.status.success() {
        return Err(YtError::Failed(last_error_line(&String::from_utf8_lossy(
            &out.stderr,
        ))));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// yt-dlp prints `ERROR: ...` as the last meaningful stderr line.
fn last_error_line(stderr: &str) -> String {
    stderr
        .lines()
        .rev()
        .find(|l| l.contains("ERROR"))
        .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()))
        .map(|l| l.trim().trim_start_matches("ERROR:").trim().to_string())
        .unwrap_or_else(|| "yt-dlp failed without a message".into())
}

/// `yt-dlp --version`, to tell the user early when the tool is missing.
pub async fn version(bin: &str) -> Result<String, YtError> {
    let mut cmd = command(bin);
    cmd.arg("--version");
    Ok(run(cmd).await?.trim().to_string())
}

/// `ytsearchN:<query>`, flat (no per-video extraction, so it is fast).
pub async fn search(bin: &str, query: &str, limit: u32) -> Result<Vec<Track>, YtError> {
    let mut cmd = command(bin);
    cmd.args(["--flat-playlist", "-J", "--no-warnings"])
        .arg(format!("ytsearch{limit}:{query}"));
    parse_tracks(&run(cmd).await?)
}

/// A pasted video or playlist link.
pub async fn resolve(bin: &str, url: &str) -> Result<Vec<Track>, YtError> {
    let mut cmd = command(bin);
    cmd.args([
        "--flat-playlist",
        "-J",
        "--no-warnings",
        "--playlist-end",
        "100",
    ])
    .arg(url);
    parse_tracks(&run(cmd).await?)
}

/// YouTube's radio mix for `id`.
pub async fn related(bin: &str, id: &str) -> Result<Vec<Track>, YtError> {
    resolve(
        bin,
        &format!("https://www.youtube.com/watch?v={id}&list=RD{id}"),
    )
    .await
}

/// The music chart, used when there is no history yet. YouTube has changed this feed before, so a
/// failure or an empty page falls back to a plain search.
pub async fn trending(bin: &str) -> Result<Vec<Track>, YtError> {
    let chart = resolve(
        bin,
        "https://www.youtube.com/feed/trending?bp=4gINGgt5dG1hX2NoYXJ0cw%3D%3D",
    )
    .await;
    match chart {
        Ok(tracks) if !tracks.is_empty() => Ok(tracks),
        _ => search(bin, "top songs this week", 20).await,
    }
}

/// Turn yt-dlp's `-J` output (a single video or a playlist) into tracks.
pub fn parse_tracks(json: &str) -> Result<Vec<Track>, YtError> {
    let root: Value = serde_json::from_str(json).map_err(|e| YtError::Parse(e.to_string()))?;
    let tracks: Vec<Track> = match root.get("entries").and_then(Value::as_array) {
        Some(entries) => entries.iter().filter_map(track_from_value).collect(),
        None => track_from_value(&root).into_iter().collect(),
    };
    Ok(tracks)
}

fn track_from_value(v: &Value) -> Option<Track> {
    let text = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    let id = text("id")?;
    // Channel pages and playlists show up as entries too; videos have 11-char ids.
    if id.len() != 11 {
        return None;
    }
    Some(Track {
        id,
        title: text("title").unwrap_or_else(|| "Untitled".into()),
        channel: text("channel")
            .or_else(|| text("uploader"))
            .unwrap_or_default(),
        duration_secs: v
            .get("duration")
            .and_then(Value::as_f64)
            .map(|d| d.max(0.0) as u64),
    })
}

/// The cached file for `id`, if a finished download exists.
fn cached(dir: &Path, id: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let p = e.path();
        let ext = p.extension()?.to_str()?;
        let done = !matches!(ext, "part" | "ytdl" | "temp");
        (done && p.file_stem()?.to_str()? == id).then_some(p)
    })
}

/// Download the best audio stream of `id` into `dir` (m4a preferred: the player decodes it
/// natively) and return the file. A previous download is reused.
pub async fn download_audio(bin: &str, id: &str, dir: &Path) -> Result<PathBuf, YtError> {
    if let Some(p) = cached(dir, id) {
        return Ok(p);
    }
    std::fs::create_dir_all(dir).map_err(|e| YtError::Failed(e.to_string()))?;
    let mut cmd = command(bin);
    cmd.args([
        "-f",
        "bestaudio[ext=m4a]/bestaudio",
        "--no-playlist",
        "--no-part",
        "--no-warnings",
        "-o",
    ])
    .arg(dir.join("%(id)s.%(ext)s"))
    .arg(format!("https://www.youtube.com/watch?v={id}"));
    run(cmd).await?;
    cached(dir, id).ok_or_else(|| YtError::Failed("the download left no audio file".into()))
}

/// Delete the oldest cached files until the folder is under `max_bytes`.
pub fn prune_cache(dir: &Path, max_bytes: u64) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<_> = read
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((meta.modified().ok()?, meta.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (_, len, path) in files {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

/// Size of the cache folder in bytes.
pub fn cache_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|r| {
            r.flatten()
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("YTFast")
        .join("audio")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_results() {
        let json = r#"{"entries":[
            {"id":"dQw4w9WgXcQ","title":"Never Gonna Give You Up","channel":"Rick Astley","duration":213.0},
            {"id":"UCuAXFkgsw1L7xaCfnd5JJOw","title":"a channel"},
            {"id":"9bZkp7q19f0","title":"Gangnam Style","uploader":"officialpsy"}
        ]}"#;
        let t = parse_tracks(json).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].duration_secs, Some(213));
        assert_eq!(t[1].channel, "officialpsy");
        assert_eq!(t[1].duration_secs, None);
    }

    #[test]
    fn parses_a_single_video() {
        let t = parse_tracks(r#"{"id":"dQw4w9WgXcQ","title":"x","duration":10}"#).unwrap();
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_tracks("not json").is_err());
    }

    #[test]
    fn error_line_prefers_error_marker() {
        let s = "WARNING: x\nERROR: [youtube] abc: Video unavailable\n";
        assert_eq!(last_error_line(s), "[youtube] abc: Video unavailable");
    }
}
