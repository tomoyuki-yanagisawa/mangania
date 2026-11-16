use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use image::RgbaImage;

use crate::archive::Page;
use crate::config::ScaleFilter;
use crate::debug_log;
use crate::input::navigation;
use crate::render::cache::{ScaledCache, ScaledKey};
use crate::render::prefetch::Prefetch;
use crate::render::view;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Viewport {
    width: u32,
    height: u32,
    filter: ScaleFilter,
}

impl Viewport {
    pub(crate) fn new(width: u32, height: u32, filter: ScaleFilter) -> Self {
        Self {
            width,
            height,
            filter,
        }
    }
}

/// A page that is currently visible on screen.
pub(crate) struct VisiblePage {
    pub(crate) index: usize,
    /// Display-sized image.
    pub(crate) image: Arc<RgbaImage>,
    /// Original page dimensions, used to lay out spreads.
    pub(crate) original_size: (u32, u32),
}

impl VisiblePage {
    pub(crate) fn render_page(&self) -> view::RenderPage<'_> {
        view::RenderPage {
            index: self.index,
            image: self.image.as_ref(),
            original_size: self.original_size,
        }
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
    pub(crate) cache: ScaledCache,
    pub(crate) errors: HashMap<usize, String>,
    previews: HashMap<usize, ScaledKey>,
    sizes: HashMap<usize, (u32, u32)>,
    attempted: HashSet<usize>,
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
            cache: ScaledCache::new(cache_limits.entries, cache_limits.bytes),
            errors: HashMap::new(),
            previews: HashMap::new(),
            sizes: HashMap::new(),
            attempted: HashSet::new(),
            depth,
            viewport,
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

    /// Changes the display target. The resized cache is cleared so pages are
    /// decoded again at the new window size or filter.
    pub(crate) fn set_viewport(&mut self, width: u32, height: u32, filter: ScaleFilter) -> bool {
        let viewport = Viewport::new(width, height, filter);
        if self.viewport == viewport {
            return false;
        }
        self.viewport = viewport;
        self.cache.clear();
        self.previews.clear();
        self.attempted.clear();
        debug_log::log(format_args!(
            "viewer: viewport {}x{} filter={} cache cleared",
            width,
            height,
            filter.as_str()
        ));
        true
    }

    pub(crate) fn request_current(&mut self, spread: bool) {
        let current = self.current;
        let mut pages = vec![current];
        if spread && current + 1 < self.pages.len() {
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

    pub(crate) fn prefetch_around(&mut self, spread: bool) {
        for page in prefetch_targets(self.current, self.pages.len(), self.depth, spread) {
            self.request(page);
        }
    }

    fn needs_request(&self, page: usize) -> bool {
        page < self.pages.len() && !self.has_preview(page) && !self.errors.contains_key(&page)
    }

    fn request(&mut self, page: usize) {
        if !self.needs_request(page) || self.attempted.contains(&page) {
            return;
        }
        let page_info = &self.pages[page];
        self.prefetch
            .request(page, page_info.entry, page_info.name.clone(), false);
    }

    /// Resizes an arrived decoded page to the current window preview, caches
    /// the display image and drops the decoded original.
    pub(crate) fn insert_decoded(&mut self, page: usize, image: Arc<RgbaImage>) {
        self.attempted.insert(page);
        let (width, height) = image.dimensions();
        let target = view::fit(
            width,
            height,
            self.viewport.width as usize,
            self.viewport.height as usize,
        );
        if target.0 == 0 || target.1 == 0 {
            self.previews.remove(&page);
            debug_log::log(format_args!(
                "viewer: skip empty preview page={} source={}x{}",
                page + 1,
                width,
                height
            ));
            return;
        }

        self.sizes.insert(page, (width, height));
        let preview = if target == image.dimensions() {
            image
        } else {
            view::resized(&image, target, self.viewport.filter)
        };
        let preview_size = preview.dimensions();
        let key = ScaledKey::new(page, preview_size.0, preview_size.1, self.viewport.filter);
        let next = self.current.saturating_add(1);
        let keep = [self.current, next];
        let retained = self.cache.insert(key, preview, &keep, self.current);
        if retained {
            self.previews.insert(page, key);
        } else {
            self.previews.remove(&page);
        }
        debug_log::log(format_args!(
            "viewer: preview page={} {}x{} -> {}x{} filter={} cached={retained}",
            page + 1,
            width,
            height,
            preview_size.0,
            preview_size.1,
            self.viewport.filter.as_str()
        ));
    }

    fn has_preview(&self, page: usize) -> bool {
        self.previews
            .get(&page)
            .is_some_and(|key| self.cache.get(*key).is_some())
    }

    pub(crate) fn has_page(&self, page: usize) -> bool {
        self.has_preview(page)
    }

    fn visible(&self, page: usize) -> Option<VisiblePage> {
        let key = *self.previews.get(&page)?;
        let image = self.cache.get(key)?;
        let original_size = *self.sizes.get(&page)?;
        Some(VisiblePage {
            index: page,
            image,
            original_size,
        })
    }

    /// Returns the current page, the next page in spread mode, and whether the
    /// next page is still loading. Images are window previews; original page
    /// dimensions are returned separately for spread layout.
    pub(crate) fn visible_pages(
        &self,
        spread: bool,
    ) -> (Option<VisiblePage>, Option<VisiblePage>, bool) {
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
    use super::{FrameState, frame_state, prefetch_targets};

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
}
