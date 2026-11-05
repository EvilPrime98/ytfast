//! Audio output. `rodio`'s output stream is not `Send` on every platform, so a dedicated thread
//! owns it; the UI talks to that thread through a command channel and reads a small shared
//! snapshot (position, ended, last error) on a timer.

use std::fs::File;
use std::io::{BufReader, Seek as _};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod growing;
mod stretch;

pub use growing::Growth;

use growing::GrowingFile;
use rodio::cpal::traits::HostTrait as _;
use rodio::{Decoder, DeviceTrait as _, OutputStream, OutputStreamBuilder, Sink, Source as _};

const DEVICE_POLL: Duration = Duration::from_secs(1);
const MAX_SWITCH_ATTEMPTS: u32 = 5;

enum Cmd {
    Load {
        file: File,
        growth: Arc<Growth>,
        volume: f32,
        speed: f32,
    },
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    Volume(f32),
    Speed(f32),
    DeviceChanged(Option<String>),
}

/// What the audio thread last reported.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub position: Duration,
    /// Length as read from the file (the video metadata may be missing).
    pub duration: Option<Duration>,
    /// The loaded track played to its end.
    pub ended: bool,
    /// Taken (cleared) by [`Engine::take_error`].
    pub error: Option<String>,
}

pub struct Engine {
    tx: Sender<Cmd>,
    shared: Arc<Mutex<Snapshot>>,
}

impl Engine {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        let thread_shared = shared.clone();
        let poll_tx = tx.clone();
        std::thread::Builder::new()
            .name("ytfast-audio".into())
            .spawn(move || audio_thread(rx, thread_shared))
            .expect("failed to start the audio thread");
        std::thread::Builder::new()
            .name("ytfast-audio-poll".into())
            .spawn(move || device_poller(poll_tx))
            .expect("failed to start the audio device poller");
        Self { tx, shared }
    }

    fn send(&self, cmd: Cmd) {
        // The thread only ends with the process.
        let _ = self.tx.send(cmd);
    }

    pub fn load(&self, file: File, growth: Arc<Growth>, volume: f32, speed: f32) {
        *self.shared.lock().unwrap() = Snapshot::default();
        self.send(Cmd::Load {
            file,
            growth,
            volume,
            speed,
        });
    }

    pub fn pause(&self) {
        self.send(Cmd::Pause);
    }

    pub fn resume(&self) {
        self.send(Cmd::Resume);
    }

    pub fn stop(&self) {
        *self.shared.lock().unwrap() = Snapshot::default();
        self.send(Cmd::Stop);
    }

    pub fn seek(&self, to: Duration) {
        self.shared.lock().unwrap().position = to;
        self.send(Cmd::Seek(to));
    }

    pub fn set_volume(&self, volume: f32) {
        self.send(Cmd::Volume(volume));
    }

    pub fn set_speed(&self, speed: f32) {
        self.send(Cmd::Speed(speed));
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.lock().unwrap().clone()
    }

    pub fn take_error(&self) -> Option<String> {
        self.shared.lock().unwrap().error.take()
    }
}

struct Loaded {
    sink: Sink,
    file: File,
    growth: Arc<Growth>,
}

fn device_poller(tx: Sender<Cmd>) {
    let mut last = default_device_name();
    loop {
        std::thread::sleep(DEVICE_POLL);
        let now = default_device_name();
        if now != last {
            last = now.clone();
            if tx.send(Cmd::DeviceChanged(now)).is_err() {
                return;
            }
        }
    }
}

#[derive(Default)]
struct Pending {
    attempts: u32,
    last_try: Option<Instant>,
}

impl Pending {
    fn exhausted(&self) -> bool {
        self.attempts >= MAX_SWITCH_ATTEMPTS
    }

    fn due(&self, now: Instant) -> bool {
        !self.exhausted()
            && self
                .last_try
                .is_none_or(|t| now.duration_since(t) >= DEVICE_POLL)
    }
}

fn default_device_name() -> Option<String> {
    rodio::cpal::default_host()
        .default_output_device()
        .and_then(|d| d.name().ok())
}

fn changed_device(
    opened_on: &Option<String>,
    default_now: Option<String>,
) -> Option<Option<String>> {
    (*opened_on != default_now).then_some(default_now)
}

fn open_stream() -> Result<(OutputStream, Option<String>), String> {
    let name = default_device_name();
    let mut s = OutputStreamBuilder::open_default_stream().map_err(|e| e.to_string())?;
    s.log_on_drop(false);
    Ok((s, name))
}

fn build_sink(
    stream: &OutputStream,
    file: &File,
    growth: &Arc<Growth>,
    tempo: &stretch::Shared,
    volume: f32,
    resume: Option<(Duration, bool)>,
) -> Result<(Sink, Option<Duration>), String> {
    let mut file = file.try_clone().map_err(|e| e.to_string())?;
    file.rewind().map_err(|e| e.to_string())?;
    let source = Decoder::new(BufReader::new(GrowingFile::new(file, growth.clone())))
        .map_err(|e| e.to_string())?;
    let duration = source.total_duration();
    let sink = Sink::connect_new(stream.mixer());
    sink.set_volume(volume);
    if resume.is_some() {
        sink.pause();
    }
    sink.append(stretch::Stretch::new(source, tempo.clone()));
    if let Some((position, paused)) = resume {
        sink.try_seek(position)
            .map_err(|e| format!("seek after device switch failed: {e}"))?;
        if !paused {
            sink.play();
        }
    }
    Ok((sink, duration))
}

fn audio_thread(rx: mpsc::Receiver<Cmd>, shared: Arc<Mutex<Snapshot>>) {
    // Opened on first use so a machine without an audio device still starts the app.
    let mut stream: Option<OutputStream> = None;
    let mut stream_device: Option<String> = None;
    let mut pending: Option<Pending> = None;
    let mut failed_resume: Option<bool> = None;
    let mut loaded: Option<Loaded> = None;
    let tempo = stretch::Shared::new(1.0);

    let fail = |msg: String| {
        tracing::error!("audio: {msg}");
        shared.lock().unwrap().error = Some(msg);
    };

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Cmd::Load {
                file,
                growth,
                volume,
                speed,
            }) => {
                if let Some(old) = loaded.take() {
                    old.sink.stop();
                }
                if stream.is_none() {
                    match open_stream() {
                        Ok((s, name)) => {
                            stream = Some(s);
                            stream_device = name;
                        }
                        Err(e) => {
                            fail(format!("no audio output device: {e}"));
                            continue;
                        }
                    }
                }
                tempo.set_tempo(speed);
                match build_sink(
                    stream.as_ref().unwrap(),
                    &file,
                    &growth,
                    &tempo,
                    volume,
                    None,
                ) {
                    Ok((sink, duration)) => {
                        shared.lock().unwrap().duration = duration;
                        loaded = Some(Loaded { sink, file, growth });
                        failed_resume = None;
                    }
                    Err(e) => fail(format!("could not play the audio: {e}")),
                }
            }
            Ok(Cmd::Pause) => {
                if let Some(l) = &loaded {
                    l.sink.pause();
                }
                if failed_resume.is_some() {
                    failed_resume = Some(false);
                }
            }
            Ok(Cmd::Resume) => match (&mut pending, &loaded) {
                (Some(p), Some(_)) => {
                    *p = Pending::default();
                    failed_resume = Some(true);
                }
                (_, Some(l)) => l.sink.play(),
                _ => {}
            },
            Ok(Cmd::Stop) => {
                if let Some(l) = loaded.take() {
                    l.sink.stop();
                }
            }
            Ok(Cmd::Seek(to)) => {
                if let Some(l) = &loaded {
                    if let Err(e) = l.sink.try_seek(to) {
                        tracing::warn!("seek failed: {e}");
                    }
                }
            }
            Ok(Cmd::Volume(v)) => {
                if let Some(l) = &loaded {
                    l.sink.set_volume(v);
                }
            }
            Ok(Cmd::Speed(s)) => tempo.set_tempo(s),
            Ok(Cmd::DeviceChanged(now)) => {
                if stream.is_some() {
                    pending = changed_device(&stream_device, now).map(|new| {
                        tracing::info!("audio: default output device {stream_device:?} -> {new:?}");
                        Pending::default()
                    });
                    if pending.is_none() {
                        failed_resume = None;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        if let Some(p) = pending.as_mut().filter(|p| p.due(Instant::now())) {
            match loaded.as_mut().filter(|l| !l.sink.empty()) {
                None => {
                    stream = None;
                    stream_device = None;
                    failed_resume = None;
                    pending = None;
                }
                Some(l) => {
                    p.last_try = Some(Instant::now());
                    let paused = l.sink.is_paused();
                    let resume_paused = failed_resume.map_or(paused, |playing| !playing);
                    match open_stream().and_then(|(new_stream, name)| {
                        l.sink.pause();
                        let position = tempo.position();
                        build_sink(
                            &new_stream,
                            &l.file,
                            &l.growth,
                            &tempo,
                            l.sink.volume(),
                            Some((position, resume_paused)),
                        )
                        .map(|(sink, _)| (new_stream, name, sink))
                    }) {
                        Ok((new_stream, name, sink)) => {
                            std::mem::replace(&mut l.sink, sink).stop();
                            stream = Some(new_stream);
                            stream_device = name;
                            failed_resume = None;
                            pending = None;
                        }
                        Err(e) => {
                            l.sink.pause();
                            if failed_resume.is_none() {
                                failed_resume = Some(!paused);
                                fail(format!("could not switch the audio output device: {e}"));
                            }
                            p.attempts += 1;
                            tracing::warn!(
                                "audio: device switch failed ({}/{MAX_SWITCH_ATTEMPTS}): {e}",
                                p.attempts
                            );
                            if p.exhausted() {
                                fail(format!(
                                    "audio output device switch failed repeatedly, playback \
                                     stays paused: {e}"
                                ));
                            }
                        }
                    }
                }
            }
        }

        if let Some(l) = &loaded {
            let mut s = shared.lock().unwrap();
            s.position = tempo.position();
            s.ended = l.sink.empty();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_default_device_change() {
        let a = Some("Speakers".to_string());
        let b = Some("Headphones".to_string());
        assert_eq!(changed_device(&a, a.clone()), None);
        assert_eq!(changed_device(&a, b.clone()), Some(b));
        assert_eq!(changed_device(&a, None), Some(None));
    }

    #[test]
    fn switch_retries_are_rate_limited_and_capped() {
        let t0 = Instant::now();
        let mut p = Pending::default();
        assert!(p.due(t0));
        p.last_try = Some(t0);
        p.attempts = 1;
        assert!(!p.due(t0 + DEVICE_POLL / 2));
        assert!(p.due(t0 + DEVICE_POLL));
        p.attempts = MAX_SWITCH_ATTEMPTS;
        assert!(p.exhausted());
        assert!(!p.due(t0 + DEVICE_POLL * 10));
    }

    #[test]
    #[ignore]
    fn plays_while_downloading() {
        use crate::api::ytdlp;
        use std::time::Instant;

        let id = std::env::var("YTFAST_TEST_VIDEO").expect("set YTFAST_TEST_VIDEO");
        let dir = std::env::temp_dir().join(format!("ytfast-stream-test-{}", std::process::id()));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let started = Instant::now();
        let d = rt
            .block_on(ytdlp::download_audio("", &id, &dir, true))
            .unwrap();
        eprintln!(
            "ready after {:?}, still downloading: {}",
            started.elapsed(),
            d.growth.is_running()
        );

        let engine = Engine::new();
        engine.load(d.file, d.growth.clone(), 0.0, 1.0);
        std::thread::sleep(Duration::from_secs(2));
        let snap = engine.snapshot();
        eprintln!("after 2s: {snap:?}, downloaded {:?}", d.growth.fraction());
        assert!(engine.take_error().is_none());
        assert!(snap.position > Duration::from_secs(1), "{snap:?}");

        let wait_for = |at_least: Duration| {
            let t = Instant::now();
            while engine.snapshot().position < at_least && t.elapsed() < Duration::from_secs(40) {
                std::thread::sleep(Duration::from_millis(100));
            }
            engine.snapshot().position
        };
        engine.seek(Duration::from_secs(600));
        let at = wait_for(Duration::from_secs(600));
        eprintln!("seek to 10min -> {at:?} (in {:?})", started.elapsed());
        assert!(at >= Duration::from_secs(600));
        assert!(engine.take_error().is_none());

        engine.seek(Duration::from_secs(30));
        std::thread::sleep(Duration::from_secs(1));
        let back = engine.snapshot().position;
        eprintln!(
            "seek back to 30s -> {back:?}, error {:?}",
            engine.take_error()
        );
        assert!(back < Duration::from_secs(60), "{back:?}");

        while d.growth.is_running() {
            std::thread::sleep(Duration::from_millis(200));
        }
        eprintln!("download finished after {:?}", started.elapsed());
        engine.seek(Duration::from_secs(7200));
        let at = wait_for(Duration::from_secs(7200));
        eprintln!("seek to 2h after completion -> {at:?}");
        assert!(at >= Duration::from_secs(7200));
        assert!(engine.take_error().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Manual smoke test (needs an audio device):
    /// `YTFAST_TEST_AUDIO=/path/to/file.m4a cargo test -- --ignored plays_a_file`
    #[test]
    #[ignore]
    fn plays_a_file() {
        let path = std::env::var("YTFAST_TEST_AUDIO").expect("set YTFAST_TEST_AUDIO");
        let engine = Engine::new();
        let file = File::open(path).unwrap();
        engine.load(file, Growth::complete(), 0.0, 1.0);
        std::thread::sleep(Duration::from_millis(1500));
        let snap = engine.snapshot();
        assert!(engine.take_error().is_none());
        assert!(snap.position > Duration::from_millis(500), "{snap:?}");
        assert!(!snap.ended);
        engine.seek(Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(500));
        assert!(engine.snapshot().position >= Duration::from_secs(30));
    }
}
