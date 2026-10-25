# YTFast

A desktop, audio-only YouTube player in the spirit of YouTube Music, built with Rust and
[GPUI](https://crates.io/crates/gpui-ce). Search for a song or paste a video / playlist link, and it plays
in a small native window with a queue, a saved library and a player bar.

<img width="1099" height="717" alt="image" src="https://github.com/user-attachments/assets/893d09d3-3a91-4f14-a347-4359fd4ccf03" />

## How it works

- **yt-dlp** finds videos (`ytsearchN:`), resolves pasted links and downloads the best audio stream
  (m4a preferred). Install it yourself and put it on `PATH`, or set its path in *Settings*.
- A track is downloaded to the audio cache first, then decoded and played with **rodio**. The first
  play of a track therefore waits for the download; replays are instant. The cache is trimmed to the
  size set in *Settings* at startup.
- Thumbnails come from `i.ytimg.com`. Everything else is local.

## Pages

| Page | What it does |
| --- | --- |
| Search | Search box (Enter), results, play / queue / save per track, *Play all* |
| Now playing | Current track, the queue, remove tracks, clear queue |
| Library | Tracks saved with the heart; stored in `<config dir>/YTFast/library.json` |
| Settings | yt-dlp path, autoplay, results per search, audio cache size / limit |

Keys: `Space` play / pause (when not typing), `Ctrl+Left` / `Ctrl+Right` previous / next,
`Alt+Left` / `Alt+Right` back / forward, `F11` fullscreen.

## Build

```sh
cargo run            # debug
cargo test
packaging/stage.sh   # release build staged in dist/YTFast
```

Linux needs the system libraries listed in `.github/workflows/ci.yml`. macOS is not packaged.

Logs: `<data dir>/YTFast/logs/`. Set `YTFAST_OPEN=search|now-playing|library|settings` to open a page
at startup.

## Notes

Use YTFast in line with YouTube's terms of service and the copyright rules where you live.
Fonts: Geist (SIL OFL 1.1), see `assets/fonts/OFL.txt`.
