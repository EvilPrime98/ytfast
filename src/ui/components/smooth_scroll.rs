//! Eased wheel scrolling for a vertically scrolling container.

use gpui::{ScrollDelta, ScrollHandle, ScrollWheelEvent, Window, point, px};

const WHEEL_STEP: f32 = 64.0;
const EASE: f32 = 0.15;
const SNAP: f32 = 0.5;

pub struct SmoothScroll {
    pub handle: ScrollHandle,
    target: f32,
    gliding: bool,
}

impl SmoothScroll {
    pub fn new() -> Self {
        Self {
            handle: ScrollHandle::new(),
            target: 0.0,
            gliding: false,
        }
    }

    /// Returns whether the event was taken over; the caller then stops propagation and notifies.
    pub fn on_wheel(&mut self, ev: &ScrollWheelEvent) -> bool {
        let max = f32::from(self.handle.max_offset().y);
        let current = -f32::from(self.handle.offset().y);
        let ScrollDelta::Lines(lines) = ev.delta else {
            self.gliding = false;
            return false;
        };
        let base = if self.gliding { self.target } else { current };
        self.target = (base - lines.y * WHEEL_STEP).clamp(0.0, max);
        self.gliding = true;
        true
    }

    pub fn reset(&mut self) {
        self.target = 0.0;
        self.gliding = false;
        self.handle.set_offset(point(px(0.0), px(0.0)));
    }

    /// Advances the glide by one frame; call it from `render`.
    pub fn step(&mut self, window: &mut Window) {
        if !self.gliding {
            return;
        }
        let max = f32::from(self.handle.max_offset().y);
        let target = self.target.min(max);
        let current = -f32::from(self.handle.offset().y);
        let next = if (target - current).abs() < SNAP {
            self.gliding = false;
            target
        } else {
            window.request_animation_frame();
            current + (target - current) * EASE
        };
        self.handle.set_offset(point(px(0.0), px(-next)));
    }
}
