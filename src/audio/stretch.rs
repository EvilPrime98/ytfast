use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use rodio::Source;
use rodio::source::SeekError;
use wsola::TimeStretch;

const READ_SAMPLES: usize = 8192;
const HOPS_PER_REFILL: usize = 4;

#[derive(Clone)]
pub struct Shared {
    tempo: Arc<AtomicU32>,
    position_us: Arc<AtomicU64>,
}

impl Shared {
    pub fn new(tempo: f32) -> Self {
        Self {
            tempo: Arc::new(AtomicU32::new(tempo.to_bits())),
            position_us: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn set_tempo(&self, tempo: f32) {
        self.tempo.store(tempo.to_bits(), Ordering::Relaxed);
    }

    pub fn position(&self) -> Duration {
        Duration::from_micros(self.position_us.load(Ordering::Relaxed))
    }
}

pub struct Stretch<S: Source> {
    inner: S,
    stretcher: TimeStretch,
    shared: Shared,
    channels: u16,
    sample_rate: u32,
    out: Vec<f32>,
    out_pos: usize,
    base_frames: u64,
    pushed_frames: u64,
    input_done: bool,
    finished: bool,
}

impl<S: Source> Stretch<S> {
    pub fn new(inner: S, shared: Shared) -> Self {
        let channels = inner.channels();
        let sample_rate = inner.sample_rate();
        let stretcher = TimeStretch::new(
            NonZero::new(sample_rate).unwrap_or(NonZero::<u32>::MIN),
            NonZero::new(channels).unwrap_or(NonZero::<u16>::MIN),
        )
        .expect("time stretcher");
        Self {
            inner,
            stretcher,
            shared,
            channels,
            sample_rate,
            out: Vec::new(),
            out_pos: 0,
            base_frames: 0,
            pushed_frames: 0,
            input_done: false,
            finished: false,
        }
    }

    fn publish_position(&self) {
        let buffered = self.stretcher.buffered() as u64;
        let consumed = (self.base_frames + self.pushed_frames).saturating_sub(buffered);
        let micros = consumed * 1_000_000 / u64::from(self.sample_rate.max(1));
        self.shared.position_us.store(micros, Ordering::Relaxed);
    }

    fn refill(&mut self) -> bool {
        let tempo = f32::from_bits(self.shared.tempo.load(Ordering::Relaxed));
        if tempo != self.stretcher.tempo() {
            self.stretcher.set_tempo(tempo);
        }
        let unit = self.stretcher.min_output_buff_size();
        self.out.resize(unit * HOPS_PER_REFILL, 0.0);
        self.out_pos = 0;

        loop {
            match self.stretcher.pull(&mut self.out) {
                Ok(n) if n > 0 => {
                    self.out.truncate(n);
                    self.publish_position();
                    return true;
                }
                _ => {
                    if self.input_done {
                        self.finished = true;
                        self.out.clear();
                        return false;
                    }
                    self.feed();
                }
            }
        }
    }

    fn feed(&mut self) {
        let ch = usize::from(self.channels.max(1));
        let want = READ_SAMPLES - READ_SAMPLES % ch;
        let mut chunk = Vec::with_capacity(want);
        chunk.extend(self.inner.by_ref().take(want));
        let whole = chunk.len() - chunk.len() % ch;
        chunk.truncate(whole);
        if chunk.len() < want {
            self.input_done = true;
        }
        self.pushed_frames += (chunk.len() / ch) as u64;
        self.stretcher.push(&chunk);
        if self.input_done {
            self.stretcher.set_draining();
        }
    }
}

impl<S: Source> Iterator for Stretch<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.out_pos >= self.out.len() {
            if self.finished || !self.refill() {
                return None;
            }
        }
        let s = self.out[self.out_pos];
        self.out_pos += 1;
        Some(s)
    }
}

impl<S: Source> Source for Stretch<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)?;
        self.stretcher.reset();
        self.out.clear();
        self.out_pos = 0;
        self.input_done = false;
        self.finished = false;
        self.base_frames = (pos.as_secs_f64() * f64::from(self.sample_rate)) as u64;
        self.pushed_frames = 0;
        self.publish_position();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    fn sine(rate: u32, secs: f32, hz: f32) -> SamplesBuffer {
        let n = (rate as f32 * secs) as usize;
        let data: Vec<f32> = (0..n)
            .map(|i| (i as f32 / rate as f32 * hz * std::f32::consts::TAU).sin() * 0.5)
            .collect();
        SamplesBuffer::new(1, rate, data)
    }

    fn crossing_rate(samples: &[f32], rate: u32) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        crossings as f32 / (samples.len() as f32 / rate as f32)
    }

    #[test]
    fn speeds_up_without_changing_pitch() {
        let rate = 44_100;
        let shared = Shared::new(2.0);
        let out: Vec<f32> = Stretch::new(sine(rate, 4.0, 440.0), shared.clone()).collect();
        let secs = out.len() as f32 / rate as f32;
        assert!((1.8..2.3).contains(&secs), "duration {secs}");
        let hz = crossing_rate(&out, rate);
        assert!((420.0..460.0).contains(&hz), "pitch {hz}");
        assert!(shared.position() > Duration::from_secs(3));
    }

    #[test]
    fn slows_down_without_changing_pitch() {
        let rate = 44_100;
        let out: Vec<f32> = Stretch::new(sine(rate, 2.0, 440.0), Shared::new(0.5)).collect();
        let secs = out.len() as f32 / rate as f32;
        assert!((3.7..4.4).contains(&secs), "duration {secs}");
        let hz = crossing_rate(&out, rate);
        assert!((420.0..460.0).contains(&hz), "pitch {hz}");
    }
}
