use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui::RenderImage;
use image::{Frame, RgbaImage};

use super::Mpv;
use super::render::Renderer;

const JUMP_DRIFT: f64 = 0.5;
const DEAD_ZONE: f64 = 0.04;
const MAX_TRIM: f64 = 0.05;
const MAX_WIDTH: f32 = 1920.0;
const MAX_HEIGHT: f32 = 1080.0;

pub struct Playback {
    renderer: Renderer,
    mpv: Mpv,
    paused: Option<bool>,
    speed: f64,
}

impl Playback {
    pub fn new(lib_path: &str) -> Result<Self, String> {
        let mpv = Mpv::new(lib_path)?;
        let renderer = Renderer::new(&mpv)?;
        Ok(Self {
            renderer,
            mpv,
            paused: None,
            speed: 1.0,
        })
    }

    pub fn load(&mut self, file: &Path, start: Duration, paused: bool) -> Result<(), String> {
        self.paused = Some(paused);
        self.mpv.load(file, start, paused)
    }

    pub fn seek(&self, to: Duration) {
        self.mpv.seek(to);
    }

    pub fn set_size(&mut self, (w, h): (u32, u32)) {
        let scale = (MAX_WIDTH / w.max(1) as f32)
            .min(MAX_HEIGHT / h.max(1) as f32)
            .min(1.0);
        let fit = |v: u32| (v as f32 * scale).round() as u32;
        self.renderer.set_size((fit(w), fit(h)));
    }

    pub fn take_frame(&mut self) -> Option<Arc<RenderImage>> {
        let f = self.renderer.take()?;
        let img = RgbaImage::from_raw(f.width, f.height, f.bgra)?;
        Some(Arc::new(RenderImage::new(vec![Frame::new(img)])))
    }

    pub fn drive(&mut self, playing: bool, clock: Duration, speed: f32) {
        if self.paused != Some(!playing) {
            self.paused = Some(!playing);
            self.mpv.set_pause(!playing);
        }
        let mut target = f64::from(speed);
        if let Some(t) = self.mpv.time_pos().filter(|_| playing) {
            let drift = t - clock.as_secs_f64();
            if drift.abs() > JUMP_DRIFT {
                self.mpv.seek(clock);
            } else if drift.abs() > DEAD_ZONE {
                target *= 1.0 - (drift * 0.5).clamp(-MAX_TRIM, MAX_TRIM);
            }
        }
        if (target - self.speed).abs() > 0.001 {
            self.speed = target;
            self.mpv.set_speed(target);
        }
    }
}

#[cfg(test)]
mod smoke;
