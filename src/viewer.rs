use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use image::RgbaImage;

use crate::cache::Cache;
use crate::navigation;
use crate::prefetch::Prefetch;

const MAX_CACHE_BYTES: usize = 128 * 1024 * 1024;

pub(crate) struct Viewer {
    pub(crate) file_name: String,
    pub(crate) path: PathBuf,
    pub(crate) pages: Vec<usize>,
    pub(crate) current: usize,
    pub(crate) prefetch: Prefetch,
    pub(crate) cache: Cache,
    pub(crate) errors: HashMap<usize, String>,
    depth: usize,
}

impl Viewer {
    pub(crate) fn new(
        file_name: String,
        path: PathBuf,
        pages: Vec<usize>,
        prefetch: Prefetch,
        depth: usize,
    ) -> Self {
        Self {
            file_name,
            path,
            pages,
            current: 0,
            prefetch,
            cache: Cache::new(depth + 2, MAX_CACHE_BYTES),
            errors: HashMap::new(),
            depth,
        }
    }

    pub(crate) fn go_to(&mut self, page: usize, spread: bool) -> bool {
        let Some(page) = navigation::clamp(page, self.pages.len(), spread) else {
            return false;
        };
        if self.current == page {
            return false;
        }
        self.current = page;
        self.prefetch.clear_pending();
        true
    }

    pub(crate) fn align_spread(&mut self) {
        self.current -= self.current % 2;
    }

    pub(crate) fn request_current(&mut self, spread: bool) {
        let current = self.current;
        self.request(current, true);
        if spread && current + 1 < self.pages.len() {
            self.request(current + 1, false);
        }
    }

    pub(crate) fn prefetch_around(&mut self) {
        let current = self.current;
        for offset in 1..=self.depth {
            let Some(page) = current.checked_add(offset) else {
                break;
            };
            if page >= self.pages.len() {
                break;
            }
            self.request(page, false);
        }
    }

    fn request(&mut self, page: usize, urgent: bool) {
        if page >= self.pages.len() || self.cache.contains(page) || self.errors.contains_key(&page)
        {
            return;
        }
        let entry = self.pages[page];
        self.prefetch.request(page, entry, urgent);
    }

    pub(crate) fn insert(&mut self, page: usize, image: Arc<RgbaImage>) {
        self.cache.insert(page, image, self.current);
    }
}
