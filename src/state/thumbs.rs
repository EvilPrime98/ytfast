//! Thumbnail cache: `HashMap<id, Thumb>` with LRU eviction and bounded concurrent fetches.

use std::collections::{HashMap, VecDeque};
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

    /// Start fetching `id` unless it was already requested. Idempotent, so callers can invoke it
    /// for every track they are about to show.
    pub fn ensure(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.map.contains_key(id) {
            return;
        }
        self.map.insert(id.to_string(), Thumb::Loading);
        self.order.push_back(id.to_string());
        while self.order.len() > MAX_ENTRIES {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        let key = id.to_string();
        cx.spawn(async move |this, cx| {
            let fetch_id = key.clone();
            let bytes = runtime::run(async move {
                let _permit = permits().acquire_owned().await.ok()?;
                thumbnails::fetch(&fetch_id).await.ok()
            })
            .await;
            this.update(cx, |this, cx| {
                // Evicted while in flight: nothing to store.
                if !this.map.contains_key(&key) {
                    return;
                }
                let thumb = match bytes {
                    Some(b) => Thumb::Ready(Arc::new(Image::from_bytes(ImageFormat::Jpeg, b))),
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
