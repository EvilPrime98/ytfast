use std::path::Path;
use std::time::Duration;

use super::{Mpv, Surface};

const JUMP_DRIFT: f64 = 0.5;
const DEAD_ZONE: f64 = 0.04;
const MAX_TRIM: f64 = 0.05;

#[derive(Debug, Default, Clone, Copy)]
pub struct Input {
    pub moved: bool,
    pub clicked: bool,
}

pub struct Playback {
    mpv: Mpv,
    surface: Surface,
    rect: (i32, i32, i32, i32),
    paused: Option<bool>,
    speed: f64,
    cursor: Option<(i32, i32)>,
    button_was_down: bool,
}

impl Playback {
    pub fn new(lib_path: &str, parent: isize) -> Result<Self, String> {
        let surface = Surface::new(parent)?;
        let mpv = Mpv::new(lib_path, surface.id())?;
        Ok(Self {
            mpv,
            surface,
            rect: (0, 0, 0, 0),
            paused: None,
            speed: 1.0,
            cursor: None,
            button_was_down: false,
        })
    }

    pub fn load(&mut self, file: &Path, start: Duration, paused: bool) -> Result<(), String> {
        self.paused = Some(paused);
        self.mpv.load(file, start, paused)
    }

    pub fn seek(&self, to: Duration) {
        self.mpv.seek(to);
    }

    pub fn place(&mut self, rect: (i32, i32, i32, i32)) {
        self.rect = rect;
        self.surface.place(rect.0, rect.1, rect.2, rect.3);
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

    pub fn poll_input(&mut self) -> Input {
        let cursor = self.surface.cursor();
        let moved = cursor.is_some() && cursor != self.cursor;
        self.cursor = cursor;
        let down = Surface::left_button_down();
        let (x, y, w, h) = self.rect;
        let over = cursor.is_some_and(|(cx, cy)| cx >= x && cx < x + w && cy >= y && cy < y + h);
        let clicked = down && !self.button_was_down && over;
        self.button_was_down = down;
        Input { moved, clicked }
    }
}

#[cfg(test)]
mod smoke;
