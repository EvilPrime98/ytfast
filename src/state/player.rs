//! The play queue and the transport (play / pause / seek / volume). Owns the audio engine.

use std::sync::Arc;
use std::time::Duration;

use gpui::{Context, Entity};

use crate::api::ytdlp;
use crate::audio::{Engine, Growth};
use crate::model::Track;
use crate::runtime;
use crate::state::history::HistoryStore;
use crate::state::prefs::PrefsStore;
use crate::ui::toast;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    /// Nothing is playing.
    Idle,
    /// The audio is being downloaded.
    Loading,
    Playing,
    Paused,
}

pub const SPEEDS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

/// A track within this long of its start restarts on "previous"; later it goes to the one before.
const RESTART_THRESHOLD: Duration = Duration::from_secs(3);

const STREAM_MIN_DURATION: u64 = 10 * 60;

const SEEK_MARGIN: f32 = 0.01;

pub struct PlayerStore {
    pub queue: Vec<Track>,
    /// Index into `queue` of the current track.
    pub index: Option<usize>,
    pub state: PlayState,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub volume: f32,
    pub speed: f32,
    /// Bumped when the queue or the current track change, so views can memoize.
    pub revision: u64,
    prefs: Entity<PrefsStore>,
    history: Entity<HistoryStore>,
    engine: Engine,
    growth: Option<Arc<Growth>>,
    /// Bumped per load so a slow download of a skipped track is dropped.
    load_generation: u64,
}

impl PlayerStore {
    pub fn new(
        prefs: Entity<PrefsStore>,
        history: Entity<HistoryStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        let volume = prefs.read(cx).prefs.volume;
        let speed = prefs
            .read(cx)
            .prefs
            .speed
            .clamp(SPEEDS[0], SPEEDS[SPEEDS.len() - 1]);
        let store = Self {
            queue: Vec::new(),
            index: None,
            state: PlayState::Idle,
            position: Duration::ZERO,
            duration: None,
            volume,
            speed,
            revision: 0,
            prefs,
            history,
            engine: Engine::new(),
            growth: None,
            load_generation: 0,
        };
        // Poll the audio thread. Ends when the store is dropped.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this.update(cx, |s, cx| s.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        store
    }

    pub fn current(&self) -> Option<&Track> {
        self.index.and_then(|ix| self.queue.get(ix))
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if let Some(e) = self.engine.take_error() {
            self.state = PlayState::Idle;
            toast::error(cx, e);
            cx.notify();
            return;
        }
        match self.growth.as_ref() {
            Some(g) if g.is_failed() => {
                self.growth = None;
                self.state = PlayState::Idle;
                self.engine.stop();
                toast::error(cx, "The download failed.".to_string());
                cx.notify();
                return;
            }
            Some(g) if !g.is_running() => self.growth = None,
            _ => {}
        }
        if self.state != PlayState::Playing {
            return;
        }
        let snap = self.engine.snapshot();
        if snap.ended {
            self.on_ended(cx);
            return;
        }
        let mut changed = snap.position.as_secs() != self.position.as_secs();
        self.position = snap.position;
        if self.duration.is_none() && snap.duration.is_some() {
            self.duration = snap.duration;
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    fn on_ended(&mut self, cx: &mut Context<Self>) {
        let autoplay = self.prefs.read(cx).prefs.autoplay_next;
        match self.index {
            Some(ix) if autoplay && ix + 1 < self.queue.len() => self.play_index(ix + 1, cx),
            _ => {
                self.state = PlayState::Idle;
                self.position = Duration::ZERO;
                self.stop_engine();
                cx.notify();
            }
        }
    }

    fn stop_engine(&mut self) {
        if let Some(g) = self.growth.take() {
            g.fail();
        }
        self.engine.stop();
    }

    /// Replace the queue with `tracks` and start `start`.
    pub fn play_tracks(&mut self, tracks: Vec<Track>, start: usize, cx: &mut Context<Self>) {
        self.queue = tracks;
        self.revision += 1;
        self.play_index(start, cx);
    }

    pub fn play_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(track) = self.queue.get(ix).cloned() else {
            return;
        };
        self.stop_engine();
        self.index = Some(ix);
        self.state = PlayState::Loading;
        self.position = Duration::ZERO;
        self.duration = track.duration_secs.map(Duration::from_secs);
        self.revision += 1;
        self.load_generation += 1;
        let generation = self.load_generation;
        let bin = self.prefs.read(cx).prefs.ytdlp_path.clone();
        let stream = track
            .duration_secs
            .is_some_and(|d| d >= STREAM_MIN_DURATION);
        self.history.update(cx, |h, cx| h.record(&track, cx));
        cx.notify();

        cx.spawn(async move |this, cx| {
            let id = track.id.clone();
            let result = runtime::run(async move {
                ytdlp::download_audio(&bin, &id, &ytdlp::cache_dir(), stream).await
            })
            .await;
            let error = this
                .update(cx, |s, cx| {
                    if s.load_generation != generation {
                        if let Ok(d) = &result {
                            d.growth.fail();
                        }
                        return None;
                    }
                    let error = match result {
                        Ok(d) => {
                            s.engine.load(d.file, d.growth.clone(), s.volume, s.speed);
                            s.growth = Some(d.growth);
                            s.state = PlayState::Playing;
                            None
                        }
                        Err(e) => {
                            s.state = PlayState::Idle;
                            Some(e.to_string())
                        }
                    };
                    cx.notify();
                    error
                })
                .ok()
                .flatten();
            if let Some(e) = error {
                tracing::warn!("could not load {}: {e}", track.id);
                cx.update(|cx| toast::error(cx, e));
            }
        })
        .detach();
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        match self.state {
            PlayState::Playing => {
                self.engine.pause();
                self.state = PlayState::Paused;
            }
            PlayState::Paused => {
                self.engine.resume();
                self.state = PlayState::Playing;
            }
            PlayState::Idle => match self.index.or((!self.queue.is_empty()).then_some(0)) {
                Some(ix) => return self.play_index(ix, cx),
                None => return,
            },
            PlayState::Loading => return,
        }
        cx.notify();
    }

    pub fn next(&mut self, cx: &mut Context<Self>) {
        match self.index {
            Some(ix) if ix + 1 < self.queue.len() => self.play_index(ix + 1, cx),
            _ => {}
        }
    }

    pub fn previous(&mut self, cx: &mut Context<Self>) {
        match self.index {
            Some(ix) if ix > 0 && self.position < RESTART_THRESHOLD => self.play_index(ix - 1, cx),
            Some(ix) => self.play_index(ix, cx),
            None => {}
        }
    }

    /// Seek to `fraction` (0..=1) of the track, when its length is known.
    pub fn seek_fraction(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let (Some(duration), true) = (
            self.duration,
            matches!(self.state, PlayState::Playing | PlayState::Paused),
        ) else {
            return;
        };
        let mut fraction = fraction.clamp(0.0, 1.0);
        if let Some(g) = self.growth.as_ref().filter(|g| g.is_running()) {
            let Some(have) = g.fraction() else {
                return;
            };
            fraction = fraction.min((have - SEEK_MARGIN).max(0.0));
        }
        let to = duration.mul_f32(fraction);
        self.engine.seek(to);
        self.position = to;
        cx.notify();
    }

    pub fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        self.volume = volume.clamp(0.0, 1.0);
        self.engine.set_volume(self.volume);
        let v = self.volume;
        self.prefs
            .update(cx, |p, cx| p.update(cx, |p| p.volume = v));
        cx.notify();
    }

    pub fn cycle_speed(&mut self, cx: &mut Context<Self>) {
        self.speed = SPEEDS
            .iter()
            .copied()
            .find(|s| *s > self.speed + 0.001)
            .unwrap_or(SPEEDS[0]);
        self.engine.set_speed(self.speed);
        let s = self.speed;
        self.prefs.update(cx, |p, cx| p.update(cx, |p| p.speed = s));
        cx.notify();
    }

    /// Append to the queue without interrupting playback.
    pub fn enqueue(&mut self, track: Track, cx: &mut Context<Self>) {
        self.queue.push(track);
        self.revision += 1;
        cx.notify();
    }

    pub fn remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.queue.len() {
            return;
        }
        self.queue.remove(ix);
        self.revision += 1;
        match self.index {
            Some(cur) if cur == ix => {
                self.load_generation += 1;
                self.stop_engine();
                self.index = None;
                self.state = PlayState::Idle;
                self.position = Duration::ZERO;
            }
            Some(cur) if cur > ix => self.index = Some(cur - 1),
            _ => {}
        }
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.load_generation += 1;
        self.stop_engine();
        self.queue.clear();
        self.index = None;
        self.state = PlayState::Idle;
        self.position = Duration::ZERO;
        self.duration = None;
        self.revision += 1;
        cx.notify();
    }
}
