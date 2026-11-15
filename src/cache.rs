use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use image::RgbaImage;

use crate::config::ScaleFilter;
use crate::debug_log;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScaledKey {
    pub page: usize,
    pub width: u32,
    pub height: u32,
    pub filter: ScaleFilter,
}

impl ScaledKey {
    pub fn new(page: usize, width: u32, height: u32, filter: ScaleFilter) -> Self {
        Self {
            page,
            width,
            height,
            filter,
        }
    }
}

/// Bounded cache for display-sized page images.
///
/// Entries are keyed by page, target size and filter. Full decoded pages are
/// never stored here; callers resize first and keep only the display result.
/// Visible pages are protected from eviction so drawing does not lose them
/// under memory pressure.
pub struct ScaledCache {
    max_entries: usize,
    max_bytes: usize,
    used_bytes: usize,
    entries: HashMap<ScaledKey, Arc<RgbaImage>>,
    order: VecDeque<ScaledKey>,
}

impl ScaledCache {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
            used_bytes: 0,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&self, key: ScaledKey) -> Option<Arc<RgbaImage>> {
        self.entries.get(&key).cloned()
    }

    /// Returns the largest cached image for a page at the given filter. A page
    /// can have a window preview and an exact spread target at the same time.
    pub fn get_page(&self, page: usize, filter: ScaleFilter) -> Option<Arc<RgbaImage>> {
        self.entries
            .iter()
            .filter(|(key, _)| key.page == page && key.filter == filter)
            .max_by_key(|(_, image)| u64::from(image.width()) * u64::from(image.height()))
            .map(|(_, image)| Arc::clone(image))
    }

    pub fn contains_page(&self, page: usize, filter: ScaleFilter) -> bool {
        self.entries
            .keys()
            .any(|key| key.page == page && key.filter == filter)
    }

    /// Inserts a display-sized image. Pages in `keep` are retained even when
    /// the byte budget is already full. When eviction is needed, entries whose
    /// page is farthest from `current` are removed first.
    pub fn insert(
        &mut self,
        key: ScaledKey,
        image: Arc<RgbaImage>,
        keep: &[usize],
        current: usize,
    ) -> bool {
        let bytes = image.as_raw().len();
        let protected = keep.contains(&key.page);
        if (self.max_entries == 0 && !protected) || (bytes > self.max_bytes && !protected) {
            debug_log::log(format_args!(
                "resized cache: skip page={} size={}x{} filter={} bytes={} over limit={}",
                key.page + 1,
                key.width,
                key.height,
                key.filter.as_str(),
                bytes,
                self.max_bytes
            ));
            return false;
        }

        if let Some(old) = self.entries.insert(key, image) {
            self.used_bytes = self.used_bytes.saturating_sub(old.as_raw().len());
            if let Some(index) = self.order.iter().position(|old| *old == key) {
                self.order.remove(index);
            }
        }
        self.order.push_back(key);
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
                    "resized cache: evict page={} distance={} size={}x{} filter={} bytes={}",
                    oldest.page + 1,
                    oldest.page.abs_diff(current),
                    oldest.width,
                    oldest.height,
                    oldest.filter.as_str(),
                    bytes
                ));
            }
        }

        let retained = self.entries.contains_key(&key);
        debug_log::log(format_args!(
            "resized cache: insert page={} size={}x{} filter={} bytes={} used={}/{} entries={} current={} keep={:?} retained={retained}",
            key.page + 1,
            key.width,
            key.height,
            key.filter.as_str(),
            bytes,
            self.used_bytes,
            self.max_bytes,
            self.entries.len(),
            current + 1,
            keep
        ));
        retained
    }

    /// Picks the farthest non-protected entry from `current`.
    ///
    /// Equal distances prefer pages behind the current page so near-future
    /// pages survive longer. Remaining ties use the older insertion.
    fn eviction_candidate(&self, current: usize, keep: &[usize]) -> Option<(ScaledKey, usize)> {
        let mut best: Option<(ScaledKey, usize, usize, bool)> = None;
        for (index, key) in self.order.iter().enumerate() {
            if keep.contains(&key.page) || !self.entries.contains_key(key) {
                continue;
            }
            let distance = key.page.abs_diff(current);
            let ahead = key.page > current;
            let replace = match best {
                None => true,
                Some((_, best_index, best_distance, best_ahead)) => {
                    distance > best_distance
                        || (distance == best_distance && ahead != best_ahead && !ahead)
                        || (distance == best_distance && ahead == best_ahead && index < best_index)
                }
            };
            if replace {
                best = Some((*key, index, distance, ahead));
            }
        }
        best.map(|(key, index, _, _)| (key, index))
    }

    pub fn clear(&mut self) {
        if !self.entries.is_empty() {
            debug_log::log(format_args!(
                "resized cache: clear entries={} used={}",
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
