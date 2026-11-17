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
///
/// The variants make an accidental blank left cell impossible: a spread is
/// only drawn with both images, and a blank left cell is drawn only when it is
/// intentional.
pub(crate) enum Visible {
    /// A page needed for the display is still loading.
    Waiting,
    /// One page fills the window.
    Single(Arc<RgbaImage>),
    /// Both cells of a spread are ready.
    Spread {
        left: Arc<RgbaImage>,
        right: Arc<RgbaImage>,
    },
    /// Only the right cell is used; the left cell is intentionally blank.
    RightOnly(Arc<RgbaImage>),
}

impl Visible {
    /// Short name for the draw debug log.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Single(_) => "single",
            Self::Spread { .. } => "spread",
            Self::RightOnly(_) => "right-only",
        }
    }
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

/// Display chosen for a layout after checking the cache and errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Display {
    Waiting,
    Single,
    Spread,
    RightOnly,
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
    /// Pages whose decoded image could not be kept in the cache. Prefetching
    /// skips them so it does not decode the same page in a loop. Pages that
    /// are on screen are still requested when missing, even after a rejection.
    /// Pages that were cached and later evicted are not listed, so a later
    /// prefetch can request them again.
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
        // Visible pages are requested again even if prefetch rejected them
        // earlier: a rejection only prevents repeated prefetches of pages that
        // are not on screen.
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
            // A page that cannot be fitted will never be displayable. Record
            // it as an error so waiting does not retry it forever.
            debug_log::log(format_args!(
                "viewer: page={} source={}x{} cannot be fitted for display",
                page + 1,
                width,
                height
            ));
            self.rejected.remove(&page);
            self.errors.insert(
                page,
                format!("cannot fit {width}x{height} page for display"),
            );
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

    /// True while the current display is waiting for a page image.
    pub(crate) fn is_loading(&self) -> bool {
        matches!(self.visible_pages(), Visible::Waiting)
    }

    /// Returns the images to draw. A spread stays `Waiting` until both pages
    /// are cached, so the previous frame remains on screen instead of showing
    /// a right-only frame that gains a left page a moment later.
    pub(crate) fn visible_pages(&self) -> Visible {
        let Some(current) = self.cache.get(self.current) else {
            return Visible::Waiting;
        };
        let next = self.current + 1;
        let has_next = next < self.pages.len();
        let left = self.cache.get(next);
        let next_known = !has_next || self.orientation(next).is_some();
        let next_failed = self.errors.contains_key(&next);
        match display(self.layout(), left.is_some(), next_known, next_failed) {
            Display::Waiting => Visible::Waiting,
            Display::Single => Visible::Single(current),
            Display::Spread => match left {
                Some(left) => Visible::Spread {
                    left,
                    right: current,
                },
                None => Visible::Waiting,
            },
            Display::RightOnly => Visible::RightOnly(current),
        }
    }
}

/// Chooses what to draw for `layout` once the current page is cached.
///
/// `left_ready` says whether the left image is cached, `next_known` whether no
/// next page remains or its orientation is known, and `next_failed` whether the
/// next page failed to decode or to fit. A failed page ends the wait; a
/// rejected prefetch does not, because the page is requested again when it is
/// visible.
fn display(layout: Layout, left_ready: bool, next_known: bool, next_failed: bool) -> Display {
    match layout {
        Layout::Single => Display::Single,
        Layout::Spread => {
            if left_ready {
                Display::Spread
            } else if next_failed {
                Display::RightOnly
            } else {
                Display::Waiting
            }
        }
        Layout::BlankLeft => {
            if next_known || next_failed {
                Display::RightOnly
            } else {
                // Mixed mode still has to learn whether the next page is
                // portrait before the left cell can be left blank for good.
                Display::Waiting
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
    use std::path::PathBuf;
    use std::sync::Arc;

    use image::RgbaImage;

    use super::{
        CacheLimits, Display, FrameState, Layout, Orientation, Prefetch, Viewer, Viewport, Visible,
        display, display_size, frame_state, layout, prefetch_targets, step_mixed,
    };
    use crate::archive::Page;
    use crate::config::{DisplayMode, ScaleFilter};

    fn test_viewer(mode: DisplayMode, pages: usize) -> Viewer {
        let pages = (0..pages)
            .map(|entry| Page {
                name: format!("{entry}.png"),
                entry,
            })
            .collect();
        Viewer::new(
            "test.zip".to_string(),
            PathBuf::from("test.zip"),
            pages,
            Prefetch::idle(),
            1,
            CacheLimits {
                entries: 8,
                bytes: usize::MAX,
            },
            Viewport::new(1200, 900, ScaleFilter::Triangle, mode),
        )
    }

    fn portrait() -> Arc<RgbaImage> {
        Arc::new(RgbaImage::new(600, 900))
    }

    #[test]
    fn failed_pages_do_not_keep_the_previous_frame() {
        assert_eq!(frame_state(false, true, false), FrameState::Failed);
        assert_eq!(frame_state(false, false, false), FrameState::Loading);
        assert_eq!(frame_state(true, false, true), FrameState::Loading);
        assert_eq!(frame_state(true, false, false), FrameState::Ready);
    }

    #[test]
    fn display_waits_until_both_spread_pages_are_ready() {
        assert_eq!(
            display(Layout::Spread, false, true, false),
            Display::Waiting
        );
        assert_eq!(display(Layout::Spread, true, true, false), Display::Spread);
        assert_eq!(
            display(Layout::Spread, false, false, true),
            Display::RightOnly
        );
    }

    #[test]
    fn display_waits_for_an_unknown_mixed_neighbour() {
        assert_eq!(
            display(Layout::BlankLeft, false, false, false),
            Display::Waiting
        );
        assert_eq!(
            display(Layout::BlankLeft, false, true, false),
            Display::RightOnly
        );
        assert_eq!(
            display(Layout::BlankLeft, false, false, true),
            Display::RightOnly
        );
        assert_eq!(
            display(Layout::Single, false, false, false),
            Display::Single
        );
    }

    #[test]
    fn loading_follows_the_display_state() {
        let mut viewer = test_viewer(DisplayMode::Spread, 4);
        assert!(viewer.is_loading());

        viewer.insert_decoded(0, portrait());
        assert!(viewer.is_loading());

        viewer.insert_decoded(1, portrait());
        assert!(!viewer.is_loading());
    }

    #[test]
    fn rejected_left_page_still_waits_for_its_image() {
        let mut viewer = test_viewer(DisplayMode::Spread, 4);
        viewer.insert_decoded(0, portrait());
        viewer.rejected.insert(1);

        assert!(matches!(viewer.visible_pages(), Visible::Waiting));

        viewer.errors.insert(1, "failed".to_string());
        assert!(matches!(viewer.visible_pages(), Visible::RightOnly(_)));
    }

    #[test]
    fn empty_fitted_page_ends_in_errors_not_rejections() {
        let mut viewer = test_viewer(DisplayMode::Spread, 4);
        viewer.insert_decoded(0, Arc::new(RgbaImage::new(0, 0)));

        assert!(viewer.errors.contains_key(&0));
        assert!(!viewer.rejected.contains(&0));
        assert!(!viewer.needs_request(0));
    }

    #[test]
    fn last_spread_page_uses_only_the_right_cell() {
        let mut viewer = test_viewer(DisplayMode::Spread, 1);
        viewer.insert_decoded(0, portrait());

        assert!(matches!(viewer.visible_pages(), Visible::RightOnly(_)));
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
