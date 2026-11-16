use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use image::RgbaImage;

use crate::archive::Page;
use crate::config::ScaleFilter;
use crate::debug_log;
use crate::input::navigation;
use crate::render::cache::PageCache;
use crate::render::prefetch::Prefetch;
use crate::render::resize::resized;
use crate::render::view;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Viewport {
    width: u32,
    height: u32,
    filter: ScaleFilter,
    spread: bool,
}

impl Viewport {
    pub(crate) fn new(width: u32, height: u32, filter: ScaleFilter, spread: bool) -> Self {
        Self {
            width,
            height,
            filter,
            spread,
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FrameState {
    Failed,
    Loading,
    Ready,
}

pub(crate) fn frame_state(
    current: bool,
    current_failed: bool,
    waiting_for_next: bool,
) -> FrameState {
    if current_failed {
        FrameState::Failed
    } else if !current || waiting_for_next {
        FrameState::Loading
    } else {
        FrameState::Ready
    }
}

/// Memory cache limits. Whichever is reached first evicts entries.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CacheLimits {
    pub(crate) entries: usize,
    pub(crate) bytes: usize,
}

pub(crate) struct Viewer {
    pub(crate) file_name: String,
    pub(crate) path: PathBuf,
    pub(crate) pages: Vec<Page>,
    pub(crate) current: usize,
    pub(crate) prefetch: Prefetch,
    pub(crate) cache: PageCache,
    pub(crate) errors: HashMap<usize, String>,
    /// Pages whose decoded image could not be kept in the cache. Prefetch
    /// skips them so it does not decode the same page in a loop. Pages that
    /// were cached and later evicted are not listed, so a later prefetch can
    /// request them again.
    rejected: HashSet<usize>,
    depth: usize,
    viewport: Viewport,
}

impl Viewer {
    pub(crate) fn new(
        file_name: String,
        path: PathBuf,
        pages: Vec<Page>,
        prefetch: Prefetch,
        depth: usize,
        cache_limits: CacheLimits,
        viewport: Viewport,
    ) -> Self {
        Self {
            file_name,
            path,
            pages,
            current: 0,
            prefetch,
            cache: PageCache::new(cache_limits.entries, cache_limits.bytes),
            errors: HashMap::new(),
            rejected: HashSet::new(),
            depth,
            viewport,
        }
    }

    pub(crate) fn go_to(&mut self, page: usize) -> bool {
        let Some(page) = navigation::clamp(page, self.pages.len(), self.viewport.spread) else {
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

    /// Changes the display target. The page cache is cleared so images are
    /// decoded again at the new window size, filter, or display mode.
    pub(crate) fn set_viewport(&mut self, viewport: Viewport) -> bool {
        if self.viewport == viewport {
            return false;
        }
        self.viewport = viewport;
        self.cache.clear();
        self.rejected.clear();
        debug_log::log(format_args!(
            "viewer: viewport {}x{} filter={} spread={} cache cleared",
            viewport.width,
            viewport.height,
            viewport.filter.as_str(),
            viewport.spread
        ));
        true
    }

    pub(crate) fn request_current(&mut self) {
        let current = self.current;
        let mut pages = vec![current];
        if self.viewport.spread && current + 1 < self.pages.len() {
            pages.push(current + 1);
        }
        // Ask for the whole spread at once. Requesting the pages separately
        // would make the second urgent request discard the first one.
        let jobs = pages
            .into_iter()
            .filter(|&page| self.needs_request(page))
            .map(|page| {
                let page_info = &self.pages[page];
                (page, page_info.entry, page_info.name.clone())
            })
            .collect();
        self.prefetch.request_batch(jobs);
    }

    pub(crate) fn prefetch_around(&mut self) {
        let targets = prefetch_targets(
            self.current,
            self.pages.len(),
            self.depth,
            self.viewport.spread,
        );
        for page in targets {
            self.request(page);
        }
    }

    fn needs_request(&self, page: usize) -> bool {
        page < self.pages.len() && !self.has_page(page) && !self.errors.contains_key(&page)
    }

    fn request(&mut self, page: usize) {
        if !self.needs_request(page) || self.rejected.contains(&page) {
            return;
        }
        let page_info = &self.pages[page];
        self.prefetch
            .request(page, page_info.entry, page_info.name.clone(), false);
    }

    /// Resizes a decoded page once to its display size and caches it. Spread
    /// pages are fitted to a half-window cell; single pages use the whole
    /// window. The decoded original is dropped.
    pub(crate) fn insert_decoded(&mut self, page: usize, image: Arc<RgbaImage>) {
        let (width, height) = image.dimensions();
        let (cell_width, cell_height) = display_size(self.viewport);
        let target = view::fit(width, height, cell_width as usize, cell_height as usize);
        if target.0 == 0 || target.1 == 0 {
            self.rejected.insert(page);
            debug_log::log(format_args!(
                "viewer: skip empty display page={} source={}x{}",
                page + 1,
                width,
                height
            ));
            return;
        }

        let display = if target == image.dimensions() {
            image
        } else {
            resized(&image, target, self.viewport.filter)
        };
        let display_size = display.dimensions();
        let next = self.current.saturating_add(1);
        let keep = [self.current, next];
        let retained = self.cache.insert(page, display, &keep, self.current);
        if retained {
            self.rejected.remove(&page);
        } else {
            self.rejected.insert(page);
        }
        debug_log::log(format_args!(
            "viewer: display page={} {}x{} -> {}x{} filter={} cached={retained}",
            page + 1,
            width,
            height,
            display_size.0,
            display_size.1,
            self.viewport.filter.as_str()
        ));
    }

    pub(crate) fn has_page(&self, page: usize) -> bool {
        self.cache.contains(page)
    }

    fn visible(&self, page: usize) -> Option<Arc<RgbaImage>> {
        self.cache.get(page)
    }

    /// Returns the current page, the next page in spread mode, and whether the
    /// next page is still loading.
    pub(crate) fn visible_pages(&self) -> (Option<Arc<RgbaImage>>, Option<Arc<RgbaImage>>, bool) {
        let spread = self.viewport.spread;
        let next = if spread {
            self.visible(self.current + 1)
        } else {
            None
        };
        let waiting = spread
            && self.current + 1 < self.pages.len()
            && next.is_none()
            && !self.errors.contains_key(&(self.current + 1));
        (self.visible(self.current), next, waiting)
    }
}

/// Size of the display image for a page. Spread mode uses one half of the
/// window as the cell for each page.
fn display_size(viewport: Viewport) -> (u32, u32) {
    if viewport.spread {
        ((viewport.width / 2).max(1), viewport.height)
    } else {
        (viewport.width, viewport.height)
    }
}

/// Pages to prefetch ahead. In spread mode the depth counts spreads, so each
/// step covers the two pages of the next spread.
fn prefetch_targets(current: usize, len: usize, depth: usize, spread: bool) -> Vec<usize> {
    let step = if spread { 2 } else { 1 };
    let mut targets = Vec::new();
    for offset in 1..=depth {
        let Some(start) = current.checked_add(offset.saturating_mul(step)) else {
            break;
        };
        for page in start..start.saturating_add(step) {
            if page >= len {
                return targets;
            }
            targets.push(page);
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::{FrameState, Viewport, display_size, frame_state, prefetch_targets};
    use crate::config::ScaleFilter;

    #[test]
    fn failed_pages_do_not_keep_the_previous_frame() {
        assert_eq!(frame_state(false, true, false), FrameState::Failed);
        assert_eq!(frame_state(false, false, false), FrameState::Loading);
        assert_eq!(frame_state(true, false, true), FrameState::Loading);
        assert_eq!(frame_state(true, false, false), FrameState::Ready);
    }

    #[test]
    fn prefetch_follows_pages_or_spreads() {
        assert_eq!(prefetch_targets(0, 10, 2, false), vec![1, 2]);
        assert_eq!(prefetch_targets(0, 10, 2, true), vec![2, 3, 4, 5]);
        assert_eq!(prefetch_targets(5, 10, 1, true), vec![7, 8]);
    }

    #[test]
    fn prefetch_stops_at_the_last_page() {
        assert_eq!(prefetch_targets(0, 3, 2, true), vec![2]);
        assert_eq!(prefetch_targets(8, 10, 2, true), Vec::<usize>::new());
        assert_eq!(prefetch_targets(9, 10, 2, false), Vec::<usize>::new());
    }

    #[test]
    fn spread_display_images_fit_half_the_window() {
        let single = Viewport::new(1200, 900, ScaleFilter::Triangle, false);
        assert_eq!(display_size(single), (1200, 900));

        let spread = Viewport::new(1200, 900, ScaleFilter::Triangle, true);
        assert_eq!(display_size(spread), (600, 900));
    }
}
