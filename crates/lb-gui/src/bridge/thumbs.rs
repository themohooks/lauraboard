//! Media thumbnails: fetched in the background from
//! `/media/{id}/thumb.jpg`, decoded off the UI thread, cached as Slint
//! images (bounded, least recently requested evicted first).

use std::collections::{HashMap, HashSet, VecDeque};

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

use super::Ctx;
use crate::net::poller::decode_jpeg;
use crate::net::{Frame, Msg};

const LIMIT: usize = 300;

#[derive(Default)]
pub struct Thumbs {
    images: HashMap<String, Option<Image>>,
    order: VecDeque<String>,
    pending: HashSet<String>,
}

impl Thumbs {
    /// Cached thumbnail; `None` while unknown or unavailable.
    pub fn get(&self, id: &str) -> Option<Image> {
        self.images.get(id).cloned().flatten()
    }

    fn insert(&mut self, id: String, img: Option<Image>) {
        self.pending.remove(&id);
        if self.images.insert(id.clone(), img).is_none() {
            self.order.push_back(id);
        }
        while self.order.len() > LIMIT {
            if let Some(old) = self.order.pop_front() {
                self.images.remove(&old);
            }
        }
    }

    /// Forget everything (channel switched).
    pub fn clear(&mut self) {
        self.images.clear();
        self.order.clear();
        self.pending.clear();
    }
}

/// Make sure the thumbnail of `id` is loaded or being loaded.
pub fn request(c: &mut Ctx, id: &str) {
    if id.is_empty() || c.thumbs.images.contains_key(id) || c.thumbs.pending.contains(id) {
        return;
    }
    c.thumbs.pending.insert(id.to_string());
    let (ch, id) = (c.model.ch(), id.to_string());
    c.worker.spawn(move |api| {
        // Missing endpoint / audio-only media simply yield no image.
        let frame = api.get_bytes(&format!("/channels/{ch}/media/{id}/thumb.jpg?w=320")).ok().and_then(|b| decode_jpeg(&b));
        Ok(Msg::Thumb(id, frame))
    });
}

/// A thumbnail arrived from the worker.
pub fn arrived(c: &mut Ctx, id: String, frame: Option<Frame>) {
    let img = frame.filter(|f| f.rgba.len() == (f.width * f.height * 4) as usize).map(|f| {
        Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&f.rgba, f.width, f.height))
    });
    c.thumbs.insert(id, img);
}
