use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use image::RgbaImage;

use crate::archive::Page;
use crate::config::{DisplayMode, ScaleFilter};
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
    mode: DisplayMode,
}

impl Viewport {
    pub(crate) fn new(width: u32, height: u32, filter: ScaleFilter, mode: DisplayMode) -> Self {
        Self {
            width,
            height,
            filter,
            mode,
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

/// Page images to draw for the current position.
#[derive(Default)]
pub(crate) struct Visible {
    pub(crate) left: Option<Arc<RgbaImage>>,
    pub(crate) current: Option<Arc<RgbaImage>>,
    /// True when the current page uses a left/right cell layout.
    pub(crate) spread: bool,
    /// True while a page needed for the layout is still loading.
    pub(crate) waiting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    fn from_size(width: u32, height: u32) -> Self {
        if height > width {
            Self::Portrait
        } else {
            Self::Landscape
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// One page in the middle of the window.
    Single,
    /// Two cells with the current page on the right; the left cell is blank.
    BlankLeft,
    /// Two cells with the next page on the left and the current page on the right.
    Spread,
}

pub(crate) struct Viewer {
    pub(crate) file_name: String,
    pub(crate) path: PathBuf,
    pub(crate) pages: Vec<Page>,
    pub(crate) current: usize,
    pub(crate) prefetch: Prefetch,
    pub(crate) cache: PageCache,
    pub(crate) errors: HashMap<usize, String>,
    /// Page orientation, recorded when the page is decoded. It survives cache
    /// clears because it does not depend on the window size or display mode.
    orientations: HashMap<usize, Orientation>,
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
            orientations: HashMap::new(),
            rejected: HashSet::new(),
            depth,
            viewport,
        }
    }

    pub(crate) fn go_to(&mut self, page: usize) -> bool {
        let spread = self.viewport.mode == DisplayMode::Spread;
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

    /// Advances or rewinds by one display. In mixed mode the step depends on
    /// the orientations of the current and neighbouring pages.
    pub(crate) fn step(&self, delta: isize) -> Option<usize> {
        let len = self.pages.len();
        match self.viewport.mode {
            DisplayMode::Single => navigation::step_single(self.current, len, delta),
            DisplayMode::Spread => navigation::step(self.current, len, true, delta),
            DisplayMode::Mixed => step_mixed(self.current, len, delta, |page| {
                self.orientation(page) == Some(Orientation::Portrait)
            }),
        }
    }

    /// Changes the display target. The page cache is cleared so images are
    /// decoded again at the new window size, filter, or display mode.
    /// Recorded page orientations are kept.
    pub(crate) fn set_viewport(&mut self, viewport: Viewport) -> bool {
        if self.viewport == viewport {
            return false;
        }
        self.viewport = viewport;
        self.cache.clear();
        self.rejected.clear();
        debug_log::log(format_args!(
            "viewer: viewport {}x{} filter={} mode={} cache cleared",
            viewport.width,
            viewport.height,
            viewport.filter.as_str(),
            viewport.mode.as_str()
        ));
        true
    }

    pub(crate) fn request_current(&mut self) {
        let current = self.current;
        let mut pages = vec![current];
        if self.viewport.mode != DisplayMode::Single && current + 1 < self.pages.len() {
            pages.push(current + 1);
        }
        // Ask for the whole spread at once. Requesting the pages separately
        // would make the second urgent request discard the first one. Mixed
        // mode also needs the next page to know whether it can be paired.
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
            self.viewport.mode,
        );
        for page in targets {
            self.request(page);
        }
    }

    fn needs_request(&self, page: usize) -> bool {
        page < self.pages.len() && !self.has_page(page) && !self.errors.contains_key(&page)
    }

    fn pending(&self, page: usize) -> bool {
        self.needs_request(page) && !self.rejected.contains(&page)
    }

    fn request(&mut self, page: usize) {
        if !self.needs_request(page) || self.rejected.contains(&page) {
            return;
        }
        let page_info = &self.pages[page];
        self.prefetch
            .request(page, page_info.entry, page_info.name.clone(), false);
    }

    /// Resizes a decoded page once to its display size and caches it. The
    /// display size depends only on the page's own orientation, so pairing it
    /// with a different neighbour never requires resizing it again. The
    /// decoded original is dropped.
    pub(crate) fn insert_decoded(&mut self, page: usize, image: Arc<RgbaImage>) {
        let (width, height) = image.dimensions();
        let orientation = Orientation::from_size(width, height);
        self.orientations.insert(page, orientation);
        let (cell_width, cell_height) = display_size(self.viewport, orientation);
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

    fn orientation(&self, page: usize) -> Option<Orientation> {
        self.orientations.get(&page).copied()
    }

    fn layout(&self) -> Layout {
        let has_next = self.current + 1 < self.pages.len();
        layout(
            self.viewport.mode,
            has_next,
            self.orientation(self.current),
            self.orientation(self.current + 1),
        )
    }

    /// Returns the page displayed together with the current page, if any.
    pub(crate) fn displayed_partner(&self) -> Option<usize> {
        (self.layout() == Layout::Spread).then_some(self.current + 1)
    }

    /// 1-based page range shown in the title bar.
    pub(crate) fn page_range(&self) -> (usize, usize) {
        let start = self.current + 1;
        let end = self.displayed_partner().map_or(start, |page| page + 1);
        (start, end)
    }

    /// Returns the images to draw and whether a needed page is still loading.
    pub(crate) fn visible_pages(&self) -> Visible {
        let Some(current) = self.cache.get(self.current) else {
            return Visible::default();
        };
        let next = self.current + 1;
        let next_orientation = self.orientation(next);
        match self.layout() {
            Layout::Single => Visible {
                current: Some(current),
                ..Visible::default()
            },
            Layout::Spread => {
                let left = self.cache.get(next);
                let waiting = left.is_none() && self.pending(next);
                Visible {
                    left,
                    current: Some(current),
                    spread: true,
                    waiting,
                }
            }
            Layout::BlankLeft => {
                // Wait for the next page's orientation before drawing a
                // portrait page alone, so the left cell does not switch from
                // blank to an image after the frame appears.
                let waiting = next_orientation.is_none() && self.pending(next);
                Visible {
                    current: Some(current),
                    spread: true,
                    waiting,
                    ..Visible::default()
                }
            }
        }
    }
}

fn layout(
    mode: DisplayMode,
    has_next: bool,
    current: Option<Orientation>,
    next: Option<Orientation>,
) -> Layout {
    match mode {
        DisplayMode::Single => Layout::Single,
        DisplayMode::Spread => {
            if has_next {
                Layout::Spread
            } else {
                Layout::BlankLeft
            }
        }
        DisplayMode::Mixed => {
            if current != Some(Orientation::Portrait) {
                Layout::Single
            } else if has_next && next == Some(Orientation::Portrait) {
                Layout::Spread
            } else {
                Layout::BlankLeft
            }
        }
    }
}

/// Steps by one display in mixed mode. Forward uses the pair that is actually
/// shown; backward assumes that the previous display paired two portrait
/// pages when both of the preceding pages are portrait.
fn step_mixed(
    current: usize,
    len: usize,
    delta: isize,
    is_portrait: impl Fn(usize) -> bool,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if delta > 0 {
        let paired = current + 1 < len && is_portrait(current) && is_portrait(current + 1);
        let target = current + if paired { 2 } else { 1 };
        return Some(target.min(len - 1));
    }
    if current == 0 {
        Some(0)
    } else if current >= 2 && is_portrait(current - 1) && is_portrait(current - 2) {
        Some(current - 2)
    } else {
        Some(current - 1)
    }
}

/// Size of the display image for a page. Single pages use the whole window.
/// Spread pages use one half of the window. Mixed mode chooses by the page's
/// own orientation, never by its neighbour.
fn display_size(viewport: Viewport, orientation: Orientation) -> (u32, u32) {
    let half = || ((viewport.width / 2).max(1), viewport.height);
    match viewport.mode {
        DisplayMode::Single => (viewport.width, viewport.height),
        DisplayMode::Spread => half(),
        DisplayMode::Mixed => match orientation {
            Orientation::Portrait => half(),
            Orientation::Landscape => (viewport.width, viewport.height),
        },
    }
}

/// Pages to prefetch ahead. Single-page mode counts pages; the other modes
/// request two pages per step so a spread or a mixed pair stays ahead.
fn prefetch_targets(current: usize, len: usize, depth: usize, mode: DisplayMode) -> Vec<usize> {
    let step = if mode == DisplayMode::Single { 1 } else { 2 };
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
    use super::{
        FrameState, Layout, Orientation, Viewport, display_size, frame_state, layout,
        prefetch_targets, step_mixed,
    };
    use crate::config::{DisplayMode, ScaleFilter};

    #[test]
    fn failed_pages_do_not_keep_the_previous_frame() {
        assert_eq!(frame_state(false, true, false), FrameState::Failed);
        assert_eq!(frame_state(false, false, false), FrameState::Loading);
        assert_eq!(frame_state(true, false, true), FrameState::Loading);
        assert_eq!(frame_state(true, false, false), FrameState::Ready);
    }

    #[test]
    fn prefetch_follows_pages_or_pairs() {
        assert_eq!(prefetch_targets(0, 10, 2, DisplayMode::Single), vec![1, 2]);
        assert_eq!(
            prefetch_targets(0, 10, 2, DisplayMode::Spread),
            vec![2, 3, 4, 5]
        );
        assert_eq!(prefetch_targets(5, 10, 1, DisplayMode::Mixed), vec![7, 8]);
    }

    #[test]
    fn prefetch_stops_at_the_last_page() {
        assert_eq!(prefetch_targets(0, 3, 2, DisplayMode::Spread), vec![2]);
        assert_eq!(
            prefetch_targets(8, 10, 2, DisplayMode::Spread),
            Vec::<usize>::new()
        );
        assert_eq!(
            prefetch_targets(9, 10, 2, DisplayMode::Single),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn display_images_fit_according_to_the_mode_and_orientation() {
        let single = Viewport::new(1200, 900, ScaleFilter::Triangle, DisplayMode::Single);
        assert_eq!(display_size(single, Orientation::Portrait), (1200, 900));
        assert_eq!(display_size(single, Orientation::Landscape), (1200, 900));

        let spread = Viewport::new(1200, 900, ScaleFilter::Triangle, DisplayMode::Spread);
        assert_eq!(display_size(spread, Orientation::Portrait), (600, 900));
        assert_eq!(display_size(spread, Orientation::Landscape), (600, 900));

        let mixed = Viewport::new(1200, 900, ScaleFilter::Triangle, DisplayMode::Mixed);
        assert_eq!(display_size(mixed, Orientation::Portrait), (600, 900));
        assert_eq!(display_size(mixed, Orientation::Landscape), (1200, 900));
    }

    #[test]
    fn square_pages_are_not_portrait() {
        assert_eq!(Orientation::from_size(100, 100), Orientation::Landscape);
        assert_eq!(Orientation::from_size(100, 101), Orientation::Portrait);
        assert_eq!(Orientation::from_size(101, 100), Orientation::Landscape);
    }

    #[test]
    fn mixed_layout_uses_the_current_and_next_orientations() {
        let portrait = Some(Orientation::Portrait);
        let landscape = Some(Orientation::Landscape);

        assert_eq!(
            layout(DisplayMode::Mixed, true, landscape, portrait),
            Layout::Single
        );
        assert_eq!(
            layout(DisplayMode::Mixed, true, portrait, portrait),
            Layout::Spread
        );
        assert_eq!(
            layout(DisplayMode::Mixed, true, portrait, landscape),
            Layout::BlankLeft
        );
        assert_eq!(
            layout(DisplayMode::Mixed, false, portrait, None),
            Layout::BlankLeft
        );
        assert_eq!(layout(DisplayMode::Mixed, true, None, None), Layout::Single);
    }

    #[test]
    fn mixed_steps_skip_paired_portrait_pages() {
        // Portrait: 0, 1, 3, 4. Landscape: 2, 5.
        let is_portrait = |page: usize| matches!(page, 0 | 1 | 3 | 4);

        assert_eq!(step_mixed(0, 6, 1, is_portrait), Some(2));
        assert_eq!(step_mixed(2, 6, 1, is_portrait), Some(3));
        assert_eq!(step_mixed(3, 6, 1, is_portrait), Some(5));
        assert_eq!(step_mixed(4, 6, 1, is_portrait), Some(5));

        assert_eq!(step_mixed(2, 6, -1, is_portrait), Some(0));
        assert_eq!(step_mixed(3, 6, -1, is_portrait), Some(2));
        assert_eq!(step_mixed(4, 6, -1, is_portrait), Some(3));
        assert_eq!(step_mixed(5, 6, -1, is_portrait), Some(3));
        assert_eq!(step_mixed(0, 6, -1, is_portrait), Some(0));
    }

    #[test]
    fn single_and_spread_layouts_ignore_orientation() {
        let portrait = Some(Orientation::Portrait);
        assert_eq!(
            layout(DisplayMode::Single, true, portrait, portrait),
            Layout::Single
        );
        assert_eq!(
            layout(DisplayMode::Spread, true, portrait, portrait),
            Layout::Spread
        );
        assert_eq!(
            layout(DisplayMode::Spread, false, portrait, None),
            Layout::BlankLeft
        );
    }
}
