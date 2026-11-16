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
        cache_bytes: usize,
        viewport: Viewport,
    ) -> Self {
        let max_entries = depth.saturating_add(2).saturating_mul(2).max(4);
        Self {
            file_name,
            path,
            pages,
            current: 0,
            prefetch,
            cache: ScaledCache::new(max_entries, cache_bytes),
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
        self.request(current, true);
        if spread && current + 1 < self.pages.len() {
            self.request(current + 1, true);
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
        if page >= self.pages.len() || self.has_preview(page) || self.errors.contains_key(&page) {
            return;
        }
        if !urgent && self.attempted.contains(&page) {
            return;
        }
        let entry = self.pages[page].entry;
        let name = self.pages[page].name.clone();
        self.prefetch.request(page, entry, name, urgent);
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

#[cfg(test)]
mod tests {
    use super::{FrameState, frame_state};

    #[test]
    fn failed_pages_do_not_keep_the_previous_frame() {
        assert_eq!(frame_state(false, true, false), FrameState::Failed);
        assert_eq!(frame_state(false, false, false), FrameState::Loading);
        assert_eq!(frame_state(true, false, true), FrameState::Loading);
        assert_eq!(frame_state(true, false, false), FrameState::Ready);
    }
}
