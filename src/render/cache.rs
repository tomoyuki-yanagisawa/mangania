use std::collections::HashMap;
use std::sync::Arc;

use image::RgbaImage;

use crate::debug_log;

/// Bounded cache for display-sized page images.
///
/// The display target is fixed until the window size, filter, or display mode
/// changes, and the cache is cleared at that point. Entries are therefore
/// keyed by page only. Full decoded pages are never stored here.
pub struct PageCache {
    max_entries: usize,
    max_bytes: usize,
    used_bytes: usize,
    entries: HashMap<usize, Arc<RgbaImage>>,
}

impl PageCache {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
            used_bytes: 0,
            entries: HashMap::new(),
        }
    }

    pub fn get(&self, page: usize) -> Option<Arc<RgbaImage>> {
        self.entries.get(&page).cloned()
    }

    pub fn contains(&self, page: usize) -> bool {
        self.entries.contains_key(&page)
    }

    /// Inserts a display-sized image.
    ///
    /// `current` is the current page and `step` is the number of pages in one
    /// display step (1 for single-page mode, 2 for spread and mixed modes).
    /// The previous, current, and next display pages are protected. Older
    /// pages are evicted first, then pages farther in the future.
    pub fn insert(
        &mut self,
        page: usize,
        image: Arc<RgbaImage>,
        current: usize,
        step: usize,
    ) -> bool {
        let step = step.max(1);
        let width = image.width();
        let height = image.height();
        let bytes = image.as_raw().len();

        if let Some(old) = self.entries.insert(page, image) {
            self.used_bytes = self.used_bytes.saturating_sub(old.as_raw().len());
        }
        self.used_bytes += bytes;

        while (self.entries.len() > self.max_entries || self.used_bytes > self.max_bytes)
            && self.entries.len() > 1
        {
            let Some(oldest) = self.eviction_candidate(current, step) else {
                break;
            };
            if let Some(image) = self.entries.remove(&oldest) {
                let bytes = image.as_raw().len();
                self.used_bytes = self.used_bytes.saturating_sub(bytes);
                debug_log::log(format_args!(
                    "page cache: evict page={} current={} distance={} size={}x{} bytes={}",
                    oldest + 1,
                    current + 1,
                    oldest.abs_diff(current),
                    image.width(),
                    image.height(),
                    bytes
                ));
            }
        }

        let retained = self.entries.contains_key(&page);
        debug_log::log(format_args!(
            "page cache: insert page={} size={width}x{height} bytes={bytes} used={}/{} entries={} current={} step={step} retained={retained}",
            page + 1,
            self.used_bytes,
            self.max_bytes,
            self.entries.len(),
            current + 1
        ));
        retained
    }

    /// Picks a page to evict.
    ///
    /// Pages before the previous display have the lowest priority. Pages after
    /// the next display come next, farthest first. The previous, current, and
    /// next display pages are protected and may exceed the soft limits.
    fn eviction_candidate(&self, current: usize, step: usize) -> Option<usize> {
        let past_end = current.saturating_sub(step);
        let future_start = current.saturating_add(step.saturating_mul(2));
        self.entries
            .keys()
            .copied()
            .filter(|&page| page < past_end)
            .min()
            .or_else(|| {
                self.entries
                    .keys()
                    .copied()
                    .filter(|&page| page >= future_start)
                    .max()
            })
    }

    pub fn clear(&mut self) {
        if !self.entries.is_empty() {
            debug_log::log(format_args!(
                "page cache: clear entries={} used={}",
                self.entries.len(),
                self.used_bytes
            ));
        }
        self.entries.clear();
        self.used_bytes = 0;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use image::RgbaImage;

    use super::PageCache;

    fn image() -> Arc<RgbaImage> {
        Arc::new(RgbaImage::new(1, 1))
    }

    #[test]
    fn evicts_far_past_before_far_future() {
        let mut cache = PageCache::new(2, 1000);
        cache.insert(0, image(), 5, 1);
        cache.insert(8, image(), 5, 1);
        cache.insert(9, image(), 5, 1);

        assert!(!cache.contains(0));
        assert!(cache.contains(8));
        assert!(cache.contains(9));
    }

    #[test]
    fn evicts_the_farthest_future_first() {
        let mut cache = PageCache::new(2, 1000);
        cache.insert(6, image(), 5, 1);
        cache.insert(7, image(), 5, 1);
        cache.insert(8, image(), 5, 1);

        assert!(cache.contains(6));
        assert!(cache.contains(7));
        assert!(!cache.contains(8));
    }

    #[test]
    fn one_step_back_and_forward_may_exceed_the_limits() {
        let mut cache = PageCache::new(1, 1000);
        for page in 4..=6 {
            assert!(cache.insert(page, image(), 5, 1));
        }

        assert_eq!(cache.len(), 3);
    }
}
