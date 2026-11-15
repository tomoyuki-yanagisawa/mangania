use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use image::RgbaImage;

use crate::cache::Cache;
use crate::navigation;
use crate::prefetch::Prefetch;

pub(crate) struct Viewer {
    pub(crate) file_name: String,
    pub(crate) path: PathBuf,
    pub(crate) pages: Vec<usize>,
    pub(crate) current: usize,
    pub(crate) prefetch: Prefetch,
    pub(crate) cache: Cache,
    pub(crate) errors: HashMap<usize, String>,
    uncached: HashSet<usize>,
    depth: usize,
}

impl Viewer {
    pub(crate) fn new(
        file_name: String,
        path: PathBuf,
        pages: Vec<usize>,
        prefetch: Prefetch,
        depth: usize,
        cache_bytes: usize,
    ) -> Self {
        Self {
            file_name,
            path,
            pages,
            current: 0,
            prefetch,
            cache: Cache::new(depth.saturating_add(2), cache_bytes),
            errors: HashMap::new(),
            uncached: HashSet::new(),
            depth,
        }
    }

    pub(crate) fn go_to(&mut self, page: usize, spread: bool) -> bool {
        let Some(page) = navigation::clamp(page, self.pages.len(), spread) else {
            return false;
        };
        self.apply_page(page)
    }

    pub(crate) fn go_to_unaligned(&mut self, page: usize) -> bool {
        let Some(page) = navigation::clamp_page(page, self.pages.len()) else {
            return false;
        };
        self.apply_page(page)
    }

    fn apply_page(&mut self, page: usize) -> bool {
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
        self.uncached.remove(&current);
        self.request(current, true);
        if spread && current + 1 < self.pages.len() {
            self.uncached.remove(&(current + 1));
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
        if !urgent && self.uncached.contains(&page) {
            return;
        }
        let entry = self.pages[page];
        self.prefetch.request(page, entry, urgent);
    }

    pub(crate) fn insert(&mut self, page: usize, image: Arc<RgbaImage>, spread: bool) {
        let next = self.current.saturating_add(1);
        let keep = if spread && next < self.pages.len() {
            [self.current, next]
        } else {
            [self.current, self.current]
        };
        if self.cache.insert(page, image, &keep) || keep.contains(&page) {
            self.uncached.remove(&page);
        } else {
            self.uncached.insert(page);
        }
    }

    /// Drains pages evicted from the decoded cache so callers can drop any
    /// resized versions of them as well.
    pub(crate) fn take_evicted_pages(&mut self) -> Vec<usize> {
        self.cache.take_evicted()
    }

    /// Returns the current page, the next page in spread mode, and whether the
    /// next page is still loading.
    pub(crate) fn visible_pages(
        &self,
        spread: bool,
    ) -> (Option<Arc<RgbaImage>>, Option<Arc<RgbaImage>>, bool) {
        let next = if spread {
            self.cache.get(self.current + 1)
        } else {
            None
        };
        let waiting = spread
            && self.current + 1 < self.pages.len()
            && next.is_none()
            && !self.errors.contains_key(&(self.current + 1));
        (self.cache.get(self.current), next, waiting)
    }
}
