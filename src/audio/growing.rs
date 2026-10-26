use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const RUNNING: u8 = 0;
const FINISHED: u8 = 1;
const FAILED: u8 = 2;

const POLL: Duration = Duration::from_millis(20);
const STALL: Duration = Duration::from_secs(30);

pub struct Growth {
    state: AtomicU8,
    downloaded: AtomicU64,
    total: AtomicU64,
}

impl Growth {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: AtomicU8::new(RUNNING),
            downloaded: AtomicU64::new(0),
            total: AtomicU64::new(0),
        })
    }

    pub fn complete() -> Arc<Self> {
        let g = Self::new();
        g.finish();
        g
    }

    pub fn finish(&self) {
        let _ = self
            .state
            .compare_exchange(RUNNING, FINISHED, Ordering::AcqRel, Ordering::Acquire);
    }

    pub fn fail(&self) {
        let _ = self
            .state
            .compare_exchange(RUNNING, FAILED, Ordering::AcqRel, Ordering::Acquire);
    }

    pub fn set_progress(&self, downloaded: u64, total: Option<u64>) {
        self.downloaded.store(downloaded, Ordering::Relaxed);
        if let Some(total) = total {
            self.total.store(total, Ordering::Relaxed);
        }
    }

    pub fn downloaded(&self) -> u64 {
        self.downloaded.load(Ordering::Relaxed)
    }

    pub fn is_running(&self) -> bool {
        self.state.load(Ordering::Acquire) == RUNNING
    }

    pub fn is_failed(&self) -> bool {
        self.state.load(Ordering::Acquire) == FAILED
    }

    pub fn fraction(&self) -> Option<f32> {
        match self.state.load(Ordering::Acquire) {
            FINISHED => Some(1.0),
            _ => {
                let total = self.total.load(Ordering::Relaxed);
                (total > 0).then(|| (self.downloaded() as f32 / total as f32).min(1.0))
            }
        }
    }
}

pub struct GrowingFile {
    file: File,
    growth: Arc<Growth>,
}

impl GrowingFile {
    pub fn new(file: File, growth: Arc<Growth>) -> Self {
        Self { file, growth }
    }
}

impl Read for GrowingFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let started = Instant::now();
        loop {
            let state = self.growth.state.load(Ordering::Acquire);
            let n = self.file.read(buf)?;
            if n > 0 || buf.is_empty() {
                return Ok(n);
            }
            match state {
                FINISHED => return Ok(0),
                FAILED => return Err(io::Error::other("the download failed")),
                _ => {}
            }
            if started.elapsed() > STALL {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the download stalled",
                ));
            }
            std::thread::sleep(POLL);
        }
    }
}

impl Seek for GrowingFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        if matches!(pos, SeekFrom::End(_)) && self.growth.is_running() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the file is still downloading",
            ));
        }
        self.file.seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ytfast-growing-{}-{name}", std::process::id()))
    }

    #[test]
    fn waits_for_more_bytes_then_ends_when_finished() {
        let path = temp("waits");
        std::fs::write(&path, b"abc").unwrap();
        let growth = Growth::new();
        let mut reader = GrowingFile::new(File::open(&path).unwrap(), growth.clone());

        let writer = {
            let path = path.clone();
            let growth = growth.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
                f.write_all(b"def").unwrap();
                growth.finish();
            })
        };

        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        writer.join().unwrap();
        assert_eq!(all, b"abcdef");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fails_instead_of_hanging_when_cancelled() {
        let path = temp("cancel");
        std::fs::write(&path, b"").unwrap();
        let growth = Growth::new();
        let mut reader = GrowingFile::new(File::open(&path).unwrap(), growth.clone());
        let g = growth.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            g.fail();
        });
        let mut buf = [0u8; 4];
        assert!(reader.read(&mut buf).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn finish_does_not_override_a_failure() {
        let g = Growth::new();
        g.fail();
        g.finish();
        assert!(g.is_failed());
        assert_eq!(g.fraction(), None);
    }

    #[test]
    fn fraction_follows_progress() {
        let g = Growth::new();
        assert_eq!(g.fraction(), None);
        g.set_progress(25, Some(100));
        assert_eq!(g.fraction(), Some(0.25));
        g.finish();
        assert_eq!(g.fraction(), Some(1.0));
    }
}
