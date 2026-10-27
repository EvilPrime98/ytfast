//! `yt-dlp` as the YouTube backend: search, link resolution and audio download.
//!
//! The binary is looked up on `PATH` unless the user points the settings at a specific file.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::sync::oneshot;

use crate::audio::Growth;
use crate::model::{Channel, Playlist, Track};

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

pub async fn search_channels(bin: &str, query: &str, limit: u32) -> Result<Vec<Channel>, YtError> {
    let url = reqwest::Url::parse_with_params(
        "https://www.youtube.com/results",
        [("search_query", query), ("sp", "EgIQAg==")],
    )
    .map_err(|e| YtError::Failed(e.to_string()))?;
    let mut cmd = command(bin);
    cmd.args(["--flat-playlist", "-J", "--no-warnings", "--playlist-end"])
        .arg(limit.to_string())
        .arg(url.as_str());
    parse_channels(&run(cmd).await?)
}

fn flat_range(bin: &str, url: &str, start: u32, end: u32) -> Command {
    let mut cmd = command(bin);
    cmd.args(["--flat-playlist", "-J", "--no-warnings"])
        .arg("--playlist-start")
        .arg(start.to_string())
        .arg("--playlist-end")
        .arg(end.to_string())
        .arg(url);
    cmd
}

pub async fn channel_videos(
    bin: &str,
    channel_id: &str,
    start: u32,
    end: u32,
) -> Result<ChannelPage, YtError> {
    let url = format!("https://www.youtube.com/channel/{channel_id}/videos");
    parse_channel_page(&run(flat_range(bin, &url, start, end)).await?)
}

pub async fn channel_playlists(
    bin: &str,
    channel_id: &str,
    start: u32,
    end: u32,
) -> Result<Vec<Playlist>, YtError> {
    let url = format!("https://www.youtube.com/channel/{channel_id}/playlists");
    parse_playlists(&run(flat_range(bin, &url, start, end)).await?)
}

pub async fn playlist_videos(
    bin: &str,
    playlist_id: &str,
    start: u32,
    end: u32,
) -> Result<PlaylistPage, YtError> {
    let url = format!("https://www.youtube.com/playlist?list={playlist_id}");
    parse_playlist_page(&run(flat_range(bin, &url, start, end)).await?)
}

pub struct ChannelPage {
    pub name: String,
    pub avatar_url: String,
    pub tracks: Vec<Track>,
}

pub struct PlaylistPage {
    pub title: String,
    pub owner: String,
    pub tracks: Vec<Track>,
}

fn is_channel_id(id: &str) -> bool {
    id.len() == 24 && id.starts_with("UC")
}

fn avatar_of(v: &Value) -> String {
    let Some(list) = v.get("thumbnails").and_then(Value::as_array) else {
        return String::new();
    };
    let url = |t: &Value| t.get("url").and_then(Value::as_str).map(str::to_string);
    let square = list.iter().find(|t| {
        let side = |k: &str| t.get(k).and_then(Value::as_u64);
        side("width").is_some() && side("width") == side("height")
    });
    let named = list.iter().find(|t| {
        t.get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id.contains("avatar"))
    });
    let found = square
        .and_then(url)
        .or_else(|| named.and_then(url))
        .or_else(|| list.last().and_then(url));
    match found {
        Some(u) if u.starts_with("//") => format!("https:{u}"),
        Some(u) => u,
        None => String::new(),
    }
}

pub fn parse_channels(json: &str) -> Result<Vec<Channel>, YtError> {
    let root: Value = serde_json::from_str(json).map_err(|e| YtError::Parse(e.to_string()))?;
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    let entries = root
        .get("entries")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    Ok(entries
        .iter()
        .filter_map(|e| {
            let id = text(e, "channel_id")
                .filter(|i| is_channel_id(i))
                .or_else(|| text(e, "id").filter(|i| is_channel_id(i)))?;
            if !seen.insert(id.clone()) {
                return None;
            }
            Some(Channel {
                name: text(e, "channel")
                    .or_else(|| text(e, "title"))
                    .or_else(|| text(e, "uploader"))
                    .unwrap_or_else(|| id.clone()),
                avatar_url: avatar_of(e),
                id,
            })
        })
        .collect())
}

pub fn parse_channel_page(json: &str) -> Result<ChannelPage, YtError> {
    let root: Value = serde_json::from_str(json).map_err(|e| YtError::Parse(e.to_string()))?;
    let name = ["channel", "uploader", "title"]
        .iter()
        .find_map(|k| root.get(*k).and_then(Value::as_str))
        .unwrap_or_default()
        .trim_end_matches(" - Videos")
        .to_string();
    let mut tracks = parse_tracks(json)?;
    for t in &mut tracks {
        if t.channel.is_empty() {
            t.channel = name.clone();
        }
    }
    Ok(ChannelPage {
        name,
        avatar_url: avatar_of(&root),
        tracks,
    })
}

fn video_id_of_thumbnail(url: &str) -> Option<&str> {
    let id = url.split("/vi/").nth(1)?.split('/').next()?;
    (id.len() == 11).then_some(id)
}

pub fn parse_playlists(json: &str) -> Result<Vec<Playlist>, YtError> {
    let root: Value = serde_json::from_str(json).map_err(|e| YtError::Parse(e.to_string()))?;
    let entries = root
        .get("entries")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    Ok(entries
        .iter()
        .filter_map(|e| {
            let text = |key: &str| e.get(key).and_then(Value::as_str);
            let id = text("id").filter(|id| !is_channel_id(id) && id.len() != 11)?;
            let thumb_id = e
                .get("thumbnails")
                .and_then(Value::as_array)
                .and_then(|list| list.iter().find_map(|t| t.get("url")?.as_str()))
                .and_then(video_id_of_thumbnail)
                .unwrap_or_default();
            Some(Playlist {
                id: id.to_string(),
                title: text("title").unwrap_or("Untitled").to_string(),
                thumb_id: thumb_id.to_string(),
            })
        })
        .collect())
}

pub fn parse_playlist_page(json: &str) -> Result<PlaylistPage, YtError> {
    let root: Value = serde_json::from_str(json).map_err(|e| YtError::Parse(e.to_string()))?;
    let text = |key: &str| root.get(key).and_then(Value::as_str).map(str::to_string);
    Ok(PlaylistPage {
        title: text("title").unwrap_or_default(),
        owner: text("channel")
            .or_else(|| text("uploader"))
            .unwrap_or_default(),
        tracks: parse_tracks(json)?,
    })
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
        upload_day: text("upload_date")
            .and_then(|d| crate::model::parse_upload_day(&d))
            .or_else(|| {
                let ts = v.get("timestamp").and_then(Value::as_i64)?;
                Some(ts.div_euclid(86_400))
            }),
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

const PROGRESS_TEMPLATE: &str = "download:%(progress.tmpfilename)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s";

pub struct Download {
    pub file: File,
    pub growth: Arc<Growth>,
}

/// Download the best audio stream of `id` into `dir` (m4a preferred: the player decodes it
pub async fn download_audio(
    bin: &str,
    id: &str,
    dir: &Path,
    stream: bool,
) -> Result<Download, YtError> {
    if let Some(p) = cached(dir, id) {
        let file = File::open(p).map_err(|e| YtError::Failed(e.to_string()))?;
        return Ok(Download {
            file,
            growth: Growth::complete(),
        });
    }
    std::fs::create_dir_all(dir).map_err(|e| YtError::Failed(e.to_string()))?;
    let mut cmd = command(bin);
    cmd.args([
        "-f",
        "bestaudio[ext=m4a]/bestaudio",
        "--no-playlist",
        "--no-warnings",
        "--quiet",
        "--progress",
        "--newline",
        "--progress-template",
        PROGRESS_TEMPLATE,
    ]);
    if stream {
        cmd.args(["--fixup", "never"]);
    } else {
        cmd.arg("--no-part");
    }
    cmd.arg("-o")
        .arg(dir.join("%(id)s.%(ext)s"))
        .arg(format!("https://www.youtube.com/watch?v={id}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => YtError::NotFound,
        _ => YtError::Failed(e.to_string()),
    })?;
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(YtError::Failed("could not read yt-dlp's output".into()));
    };

    let growth = Growth::new();
    let (ready_tx, ready_rx) = oneshot::channel();
    tokio::spawn(supervise(
        child,
        stdout,
        stderr,
        Job {
            dir: dir.to_path_buf(),
            id: id.to_string(),
            start_bytes: stream.then_some(START_BYTES),
        },
        growth.clone(),
        ready_tx,
    ));
    let file = ready_rx
        .await
        .map_err(|_| YtError::Failed("the download ended unexpectedly".into()))??;
    Ok(Download { file, growth })
}

const START_BYTES: u64 = 512 * 1024;

struct Job {
    dir: PathBuf,
    id: String,
    start_bytes: Option<u64>,
}

async fn supervise(
    mut child: Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
    job: Job,
    growth: Arc<Growth>,
    ready: oneshot::Sender<Result<File, YtError>>,
) {
    let mut ready = Some(ready);
    let errors = tokio::spawn(async move {
        let mut text = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut text).await;
        text
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut watch = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                let Some((part, downloaded, total)) = parse_progress(&line) else { continue };
                growth.set_progress(downloaded, total);
                let enough = job.start_bytes.is_some_and(|min| downloaded >= min);
                if enough && ready.is_some() {
                    let file = File::open(&part).or_else(|_| open_cached(&job.dir, &job.id));
                    if let Some(tx) = ready.take() {
                        let _ = tx.send(file.map_err(|e| YtError::Failed(e.to_string())));
                    }
                }
            }
            _ = watch.tick() => {
                if !growth.is_running() {
                    let _ = child.kill().await;
                    break;
                }
            }
        }
    }

    let status = child.wait().await;
    let outcome = match status {
        Ok(s) if s.success() && growth.is_running() => {
            open_cached(&job.dir, &job.id).map_err(|e| YtError::Failed(e.to_string()))
        }
        Ok(_) if !growth.is_running() => Err(YtError::Failed("the download was cancelled".into())),
        Ok(_) => {
            let text = errors.await.unwrap_or_default();
            Err(YtError::Failed(last_error_line(&text)))
        }
        Err(e) => Err(YtError::Failed(e.to_string())),
    };
    match outcome {
        Ok(file) => {
            growth.finish();
            if let Some(tx) = ready.take() {
                let _ = tx.send(Ok(file));
            }
        }
        Err(e) => {
            tracing::warn!("download of {} failed: {e}", job.id);
            growth.fail();
            if let Some(tx) = ready.take() {
                let _ = tx.send(Err(e));
            }
        }
    }
}

fn open_cached(dir: &Path, id: &str) -> std::io::Result<File> {
    match cached(dir, id) {
        Some(p) => File::open(p),
        None => Err(std::io::Error::other("the download left no audio file")),
    }
}

fn parse_progress(line: &str) -> Option<(PathBuf, u64, Option<u64>)> {
    let mut fields = line.trim().rsplitn(4, '|');
    let estimate = fields.next()?.parse::<u64>().ok();
    let total = fields.next()?.parse::<u64>().ok();
    let downloaded = fields.next()?.parse::<u64>().ok()?;
    let part = fields.next()?;
    Some((PathBuf::from(part), downloaded, total.or(estimate)))
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
    fn parses_channel_search_results() {
        let json = r#"{"entries":[
            {"id":"UCuAXFkgsw1L7xaCfnd5JJOw","title":"Rick Astley","thumbnails":[{"url":"//yt3.ggpht.com/a"}]},
            {"id":"UCuAXFkgsw1L7xaCfnd5JJOw","title":"dup"},
            {"id":"dQw4w9WgXcQ","title":"a video"}
        ]}"#;
        let c = parse_channels(json).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "Rick Astley");
        assert_eq!(c[0].avatar_url, "https://yt3.ggpht.com/a");
    }

    #[test]
    fn channel_page_fills_in_the_channel_name() {
        let json = r#"{"channel":"Rick","thumbnails":[{"id":"0","url":"b"},{"id":"avatar_uncropped","url":"a"}],
            "entries":[{"id":"dQw4w9WgXcQ","title":"x"}]}"#;
        let p = parse_channel_page(json).unwrap();
        assert_eq!(p.avatar_url, "a");
        assert_eq!(p.tracks[0].channel, "Rick");
    }

    #[test]
    fn parses_channel_playlists() {
        let json = r#"{"entries":[
            {"id":"PLlaN88a7y2_qHDbY9eQbuNTAuEJUSEeuu","title":"Live at The O2",
             "thumbnails":[{"url":"https://i.ytimg.com/vi/PXC_PYeB6F8/hqdefault.jpg?sqp=x"}]},
            {"id":"PLbare","title":"No thumbnail"},
            {"id":"dQw4w9WgXcQ","title":"a video"}
        ]}"#;
        let p = parse_playlists(json).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].thumb_id, "PXC_PYeB6F8");
        assert_eq!(p[1].thumb_id, "");
    }

    #[test]
    fn playlist_page_has_title_owner_and_videos() {
        let json = r#"{"title":"Mix","channel":"Rick",
            "entries":[{"id":"dQw4w9WgXcQ","title":"x"}]}"#;
        let p = parse_playlist_page(json).unwrap();
        assert_eq!((p.title.as_str(), p.owner.as_str()), ("Mix", "Rick"));
        assert_eq!(p.tracks.len(), 1);
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
    fn parses_progress_lines() {
        let (part, done, total) =
            parse_progress("C:\\c\\a|b\\id.m4a.part|1024|204129073|NA\r\n").unwrap();
        assert_eq!(part, PathBuf::from("C:\\c\\a|b\\id.m4a.part"));
        assert_eq!((done, total), (1024, Some(204129073)));
        let (_, _, total) = parse_progress("id.m4a.part|5|NA|900").unwrap();
        assert_eq!(total, Some(900));
        assert!(parse_progress("[download] Destination: x").is_none());
    }

    #[test]
    fn error_line_prefers_error_marker() {
        let s = "WARNING: x\nERROR: [youtube] abc: Video unavailable\n";
        assert_eq!(last_error_line(s), "[youtube] abc: Video unavailable");
    }
}
