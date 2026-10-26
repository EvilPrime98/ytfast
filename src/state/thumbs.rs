//! Thumbnail cache: `HashMap<id, Thumb>` with LRU eviction and bounded concurrent fetches.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use gpui::{Context, Image, ImageFormat};
use tokio::sync::Semaphore;

use crate::api::thumbnails;
use crate::runtime;

const MAX_ENTRIES: usize = 400;

fn permits() -> Arc<Semaphore> {
    static SEM: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEM.get_or_init(|| Arc::new(Semaphore::new(6))).clone()
}

#[derive(Clone)]
enum Thumb {
    Loading,
    Ready(Arc<Image>),
    Missing,
}

pub fn avatar_key(channel_id: &str) -> String {
    format!("ch:{channel_id}")
}

fn avatar_path(channel_id: &str) -> Option<PathBuf> {
    Some(
        dirs::cache_dir()?
            .join("YTFast")
            .join("avatars")
            .join(format!("{channel_id}.img")),
    )
}

fn sniff(bytes: &[u8]) -> ImageFormat {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        ImageFormat::Png
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        ImageFormat::Webp
    } else {
        ImageFormat::Jpeg
    }
}

#[derive(Default)]
pub struct Thumbnails {
    map: HashMap<String, Thumb>,
    order: VecDeque<String>,
}

impl Thumbnails {
    /// The decoded thumbnail, if it has arrived.
    pub fn get(&self, id: &str) -> Option<Arc<Image>> {
        match self.map.get(id) {
            Some(Thumb::Ready(img)) => Some(img.clone()),
            _ => None,
        }
    }

    pub fn avatar(&self, channel_id: &str) -> Option<Arc<Image>> {
        self.get(&avatar_key(channel_id))
    }

    /// Start fetching `id` unless it was already requested. Idempotent, so callers can invoke it
    /// for every track they are about to show.
    pub fn ensure(&mut self, id: &str, cx: &mut Context<Self>) {
        let fetch_id = id.to_string();
        self.load(id, cx, async move {
            let bytes = thumbnails::fetch(&fetch_id).await.ok()?;
            Some((bytes, ImageFormat::Jpeg))
        });
    }

    pub fn ensure_avatar(&mut self, channel_id: &str, url: &str, cx: &mut Context<Self>) {
        if url.is_empty() {
            return;
        }
        let (id, url) = (channel_id.to_string(), url.to_string());
        self.load(&avatar_key(channel_id), cx, async move {
            let path = avatar_path(&id);
            if let Some(bytes) = path.as_ref().and_then(|p| std::fs::read(p).ok()) {
                let format = sniff(&bytes);
                return Some((bytes, format));
            }
            let bytes = thumbnails::fetch_url(&url).await.ok()?;
            if let Some(p) = path {
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(p, &bytes);
            }
            let format = sniff(&bytes);
            Some((bytes, format))
        });
    }

    fn load(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
        fetch: impl Future<Output = Option<(Vec<u8>, ImageFormat)>> + Send + 'static,
    ) {
        if self.map.contains_key(key) {
            return;
        }
        self.map.insert(key.to_string(), Thumb::Loading);
        self.order.push_back(key.to_string());
        while self.order.len() > MAX_ENTRIES {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        let key = key.to_string();
        cx.spawn(async move |this, cx| {
            let loaded = runtime::run(async move {
                let _permit = permits().acquire_owned().await.ok()?;
                fetch.await
            })
            .await;
            this.update(cx, |this, cx| {
                // Evicted while in flight: nothing to store.
                if !this.map.contains_key(&key) {
                    return;
                }
                let thumb = match loaded {
                    Some((b, format)) => Thumb::Ready(Arc::new(Image::from_bytes(format, b))),
                    None => Thumb::Missing,
                };
                this.map.insert(key, thumb);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
