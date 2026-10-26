//! Audio output. `rodio`'s output stream is not `Send` on every platform, so a dedicated thread
//! owns it; the UI talks to that thread through a command channel and reads a small shared
//! snapshot (position, ended, last error) on a timer.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod stretch;

use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source as _};

enum Cmd {
    Load {
        path: PathBuf,
        volume: f32,
        speed: f32,
    },
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    Volume(f32),
    Speed(f32),
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
        std::thread::Builder::new()
            .name("ytfast-audio".into())
            .spawn(move || audio_thread(rx, thread_shared))
            .expect("failed to start the audio thread");
        Self { tx, shared }
    }

    fn send(&self, cmd: Cmd) {
        // The thread only ends with the process.
        let _ = self.tx.send(cmd);
    }

    pub fn load(&self, path: PathBuf, volume: f32, speed: f32) {
        *self.shared.lock().unwrap() = Snapshot::default();
        self.send(Cmd::Load {
            path,
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
}

fn audio_thread(rx: mpsc::Receiver<Cmd>, shared: Arc<Mutex<Snapshot>>) {
    // Opened on first use so a machine without an audio device still starts the app.
    let mut stream: Option<OutputStream> = None;
    let mut loaded: Option<Loaded> = None;
    let tempo = stretch::Shared::new(1.0);

    let fail = |msg: String| {
        tracing::error!("audio: {msg}");
        shared.lock().unwrap().error = Some(msg);
    };

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Cmd::Load {
                path,
                volume,
                speed,
            }) => {
                if let Some(old) = loaded.take() {
                    old.sink.stop();
                }
                if stream.is_none() {
                    match OutputStreamBuilder::open_default_stream() {
                        Ok(mut s) => {
                            s.log_on_drop(false);
                            stream = Some(s);
                        }
                        Err(e) => {
                            fail(format!("no audio output device: {e}"));
                            continue;
                        }
                    }
                }
                let source = File::open(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|f| Decoder::new(BufReader::new(f)).map_err(|e| e.to_string()));
                match source {
                    Ok(source) => {
                        let duration = source.total_duration();
                        let sink = Sink::connect_new(stream.as_ref().unwrap().mixer());
                        sink.set_volume(volume);
                        tempo.set_tempo(speed);
                        sink.append(stretch::Stretch::new(source, tempo.clone()));
                        shared.lock().unwrap().duration = duration;
                        loaded = Some(Loaded { sink });
                    }
                    Err(e) => fail(format!("could not play {}: {e}", path.display())),
                }
            }
            Ok(Cmd::Pause) => {
                if let Some(l) = &loaded {
                    l.sink.pause();
                }
            }
            Ok(Cmd::Resume) => {
                if let Some(l) = &loaded {
                    l.sink.play();
                }
            }
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
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
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

    /// Manual smoke test (needs an audio device):
    /// `YTFAST_TEST_AUDIO=/path/to/file.m4a cargo test -- --ignored plays_a_file`
    #[test]
    #[ignore]
    fn plays_a_file() {
        let path = std::env::var("YTFAST_TEST_AUDIO").expect("set YTFAST_TEST_AUDIO");
        let engine = Engine::new();
        engine.load(PathBuf::from(path), 0.0, 1.0);
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
