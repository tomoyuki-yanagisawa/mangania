use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use image::RgbaImage;

pub struct Cache {
    max_pages: usize,
    max_bytes: usize,
    used_bytes: usize,
    entries: HashMap<usize, Arc<RgbaImage>>,
    order: VecDeque<usize>,
}

impl Cache {
    pub fn new(max_pages: usize, max_bytes: usize) -> Self {
        Self {
            max_pages,
            max_bytes,
            used_bytes: 0,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&self, page: usize) -> Option<Arc<RgbaImage>> {
        self.entries.get(&page).cloned()
    }

    pub fn contains(&self, page: usize) -> bool {
        self.entries.contains_key(&page)
    }

    pub fn insert(&mut self, page: usize, image: Arc<RgbaImage>, keep: usize) {
        let bytes = image.as_raw().len();
        if bytes > self.max_bytes && page != keep {
            return;
        }

        if let Some(old) = self.entries.insert(page, image) {
            self.used_bytes = self.used_bytes.saturating_sub(old.as_raw().len());
        } else {
            self.order.push_back(page);
        }
        self.used_bytes += bytes;

        while (self.order.len() > self.max_pages || self.used_bytes > self.max_bytes)
            && self.entries.len() > 1
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if oldest == keep {
                self.order.push_back(oldest);
                continue;
            }
            if let Some(image) = self.entries.remove(&oldest) {
                self.used_bytes = self.used_bytes.saturating_sub(image.as_raw().len());
            }
        }
    }
}
