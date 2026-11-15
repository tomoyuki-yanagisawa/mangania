use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use image::RgbaImage;

use crate::config::ScaleFilter;
use crate::debug_log;

pub struct Cache {
    max_pages: usize,
    max_bytes: usize,
    used_bytes: usize,
    entries: HashMap<usize, Arc<RgbaImage>>,
    order: VecDeque<usize>,
    evicted: Vec<usize>,
}

impl Cache {
    pub fn new(max_pages: usize, max_bytes: usize) -> Self {
        Self {
            max_pages,
            max_bytes,
            used_bytes: 0,
            entries: HashMap::new(),
            order: VecDeque::new(),
            evicted: Vec::new(),
        }
    }

    pub fn get(&self, page: usize) -> Option<Arc<RgbaImage>> {
        self.entries.get(&page).cloned()
    }

    pub fn contains(&self, page: usize) -> bool {
        self.entries.contains_key(&page)
    }

    pub fn insert(&mut self, page: usize, image: Arc<RgbaImage>, keep: &[usize]) -> bool {
        let bytes = image.as_raw().len();
        if bytes > self.max_bytes && !keep.contains(&page) {
            debug_log::log(format_args!(
                "cache: skip page={} bytes={} over limit={}",
                page + 1,
                bytes,
                self.max_bytes
            ));
            return false;
        }

        if let Some(old) = self.entries.insert(page, image) {
            self.used_bytes = self.used_bytes.saturating_sub(old.as_raw().len());
        } else {
            self.order.push_back(page);
        }
        self.used_bytes += bytes;

        while (self.order.len() > self.max_pages || self.used_bytes > self.max_bytes)
            && self.entries.len() > 1
            && self.entries.keys().any(|page| !keep.contains(page))
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if keep.contains(&oldest) {
                self.order.push_back(oldest);
                continue;
            }
            if let Some(image) = self.entries.remove(&oldest) {
                let bytes = image.as_raw().len();
                self.used_bytes = self.used_bytes.saturating_sub(bytes);
                self.evicted.push(oldest);
                debug_log::log(format_args!(
                    "cache: evict page={} bytes={}",
                    oldest + 1,
                    bytes
                ));
            }
        }

        let retained = self.entries.contains_key(&page);
        debug_log::log(format_args!(
            "cache: insert page={} bytes={} used={}/{} entries={} keep={:?} retained={retained}",
            page + 1,
            bytes,
            self.used_bytes,
            self.max_bytes,
            self.entries.len(),
            keep
        ));
        retained
    }

    /// Drains the pages evicted since the last call.
    pub fn take_evicted(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.evicted)
    }
}

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

/// Bounded cache for resized pages.
///
/// Entries are keyed by page, target size and filter so a plain redraw can reuse
/// the previous resize. The byte budget keeps the cache from growing with the
/// window size or with the number of page/filter combinations.
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

    pub fn insert(&mut self, key: ScaledKey, image: Arc<RgbaImage>) -> bool {
        let bytes = image.as_raw().len();
        if self.max_entries == 0 || bytes > self.max_bytes {
            debug_log::log(format_args!(
                "scaled cache: skip page={} size={}x{} filter={} bytes={} over limit={}",
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
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(image) = self.entries.remove(&oldest) {
                let bytes = image.as_raw().len();
                self.used_bytes = self.used_bytes.saturating_sub(bytes);
                debug_log::log(format_args!(
                    "scaled cache: evict page={} size={}x{} filter={} bytes={}",
                    oldest.page + 1,
                    oldest.width,
                    oldest.height,
                    oldest.filter.as_str(),
                    bytes
                ));
            }
        }

        let retained = self.entries.contains_key(&key);
        debug_log::log(format_args!(
            "scaled cache: insert page={} size={}x{} filter={} bytes={} used={}/{} entries={} retained={retained}",
            key.page + 1,
            key.width,
            key.height,
            key.filter.as_str(),
            bytes,
            self.used_bytes,
            self.max_bytes,
            self.entries.len()
        ));
        retained
    }

    pub fn remove_page(&mut self, page: usize) {
        let keys: Vec<ScaledKey> = self
            .entries
            .keys()
            .copied()
            .filter(|key| key.page == page)
            .collect();
        for key in keys {
            if let Some(image) = self.entries.remove(&key) {
                self.used_bytes = self.used_bytes.saturating_sub(image.as_raw().len());
            }
        }
        self.order.retain(|key| key.page != page);
    }

    pub fn clear(&mut self) {
        if !self.entries.is_empty() {
            debug_log::log(format_args!(
                "scaled cache: clear entries={} used={}",
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
