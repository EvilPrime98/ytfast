# YTFast

A desktop, audio-only YouTube player in the spirit of YouTube Music, built with Rust and
[GPUI](https://crates.io/crates/gpui-ce). Search for a song or paste a video / playlist link, and it plays
in a small native window with a queue, a saved library and a player bar.

<img width="1092" height="717" alt="image" src="https://github.com/user-attachments/assets/f2d82d3d-ae27-4890-8c63-93b34b15b544" />

## How it works

- **yt-dlp** finds videos (`ytsearchN:`), resolves pasted links and downloads the best audio stream
  (m4a preferred). Install it yourself and put it on `PATH`, or set its path in *Settings*.
- A track is downloaded to the audio cache first, then decoded and played with **rodio**. The first
  play of a track therefore waits for the download; replays are instant. The cache is trimmed to the
  size set in *Settings* at startup.
- Thumbnails come from `i.ytimg.com`. Everything else is local.

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

## More Screenshots

<img width="1384" height="874" alt="image" src="https://github.com/user-attachments/assets/382caac0-5e1d-4c04-85aa-20479238b5c9" />

<img width="1383" height="886" alt="image" src="https://github.com/user-attachments/assets/057a9e02-2fe7-4f8b-b298-a44003619ca6" />
