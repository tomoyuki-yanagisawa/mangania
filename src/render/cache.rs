use std::collections::{HashMap, VecDeque};
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
    order: VecDeque<usize>,
}

impl PageCache {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
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

    /// Inserts a display-sized image. Pages in `keep` are retained even when
    /// the byte budget is already full. When eviction is needed, entries whose
    /// page is farthest from `current` are removed first.
    pub fn insert(
        &mut self,
        page: usize,
        image: Arc<RgbaImage>,
        keep: &[usize],
        current: usize,
    ) -> bool {
        let width = image.width();
        let height = image.height();
        let bytes = image.as_raw().len();
        let protected = keep.contains(&page);
        if (self.max_entries == 0 && !protected) || (bytes > self.max_bytes && !protected) {
            debug_log::log(format_args!(
                "page cache: skip page={} size={width}x{height} bytes={bytes} over limit={}",
                page + 1,
                self.max_bytes
            ));
            return false;
        }

        if let Some(old) = self.entries.insert(page, image) {
            self.used_bytes = self.used_bytes.saturating_sub(old.as_raw().len());
            if let Some(index) = self.order.iter().position(|old| *old == page) {
                self.order.remove(index);
            }
        }
        self.order.push_back(page);
        self.used_bytes += bytes;

        while (self.order.len() > self.max_entries || self.used_bytes > self.max_bytes)
            && self.entries.len() > 1
        {
            let Some((oldest, index)) = self.eviction_candidate(current, keep) else {
                break;
            };
            self.order.remove(index);
            if let Some(image) = self.entries.remove(&oldest) {
                let bytes = image.as_raw().len();
                self.used_bytes = self.used_bytes.saturating_sub(bytes);
                debug_log::log(format_args!(
                    "page cache: evict page={} distance={} size={}x{} bytes={}",
                    oldest + 1,
                    oldest.abs_diff(current),
                    image.width(),
                    image.height(),
                    bytes
                ));
            }
        }

        let retained = self.entries.contains_key(&page);
        debug_log::log(format_args!(
            "page cache: insert page={} size={width}x{height} bytes={bytes} used={}/{} entries={} current={} keep={:?} retained={retained}",
            page + 1,
            self.used_bytes,
            self.max_bytes,
            self.entries.len(),
            current + 1,
            keep
        ));
        retained
    }

    /// Picks the farthest non-protected page from `current`.
    ///
    /// Equal distances prefer pages behind the current page so near-future
    /// pages survive longer. Remaining ties use the older insertion.
    fn eviction_candidate(&self, current: usize, keep: &[usize]) -> Option<(usize, usize)> {
        let mut best: Option<(usize, usize, usize, bool)> = None;
        for (index, page) in self.order.iter().enumerate() {
            if keep.contains(page) || !self.entries.contains_key(page) {
                continue;
            }
            let distance = page.abs_diff(current);
            let ahead = *page > current;
            let replace = match best {
                None => true,
                Some((_, best_index, best_distance, best_ahead)) => {
                    distance > best_distance
                        || (distance == best_distance && ahead != best_ahead && !ahead)
                        || (distance == best_distance && ahead == best_ahead && index < best_index)
                }
            };
            if replace {
                best = Some((*page, index, distance, ahead));
            }
        }
        best.map(|(page, index, _, _)| (page, index))
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
        self.order.clear();
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
