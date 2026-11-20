use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use image::RgbaImage;

use crate::archive::Page;
use crate::config::{DisplayMode, ScaleFilter};
use crate::debug_log;
use crate::input::navigation;
use crate::render::cache::PageCache;
use crate::render::image::{fit, resized};
use crate::render::prefetch::Prefetch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Viewport {
    width: u32,
    height: u32,
    filter: ScaleFilter,
    mode: DisplayMode,
    allow_upscale: bool,
}

impl Viewport {
    pub(crate) fn new(
        width: u32,
        height: u32,
        filter: ScaleFilter,
        mode: DisplayMode,
        allow_upscale: bool,
    ) -> Self {
        Self {
            width,
            height,
            filter,
            mode,
            allow_upscale,
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Memory cache limits. Whichever is reached first evicts entries.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CacheLimits {
    pub(crate) entries: usize,
    pub(crate) bytes: usize,
}

/// The frame to draw for the current position.
///
/// The variants make an accidental blank left cell impossible: a spread is
/// only drawn with both images, and a blank left cell is drawn only when it is
/// intentional.
pub(crate) enum Frame {
    /// A page needed for the display is still loading. The previous frame is
    /// kept on screen.
    Loading,
    /// The current page failed to decode or fit. The frame is blank.
    Failed,
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

/// What the screen shows, without the images themselves.
///
/// Used to tell whether a decoded page changes the display. Prefetched pages
/// do not, so they must not trigger another full redraw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameKind {
    Loading,
    Failed,
    Single,
    Spread,
    RightOnly,
}

impl FrameKind {
    /// Short name for the draw debug log.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Loading => "waiting",
            Self::Failed => "failed",
            Self::Single => "single",
            Self::Spread => "spread",
            Self::RightOnly => "right-only",
        }
    }
}

impl Frame {
    pub(crate) fn kind(&self) -> FrameKind {
        match self {
            Self::Loading => FrameKind::Loading,
            Self::Failed => FrameKind::Failed,
            Self::Single(_) => FrameKind::Single,
            Self::Spread { .. } => FrameKind::Spread,
            Self::RightOnly(_) => FrameKind::RightOnly,
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

/// Per-page state that survives display-cache clears.
#[derive(Default)]
struct PageState {
    orientation: Option<Orientation>,
    error: Option<String>,
    /// The decoded page could not be kept in the display cache.
    rejected: bool,
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
    pub(crate) path: PathBuf,
    pub(crate) pages: Vec<Page>,
    pub(crate) current: usize,
    pub(crate) prefetch: Prefetch,
    pub(crate) cache: PageCache,
    /// Per-page state, indexed by page number.
    states: Vec<PageState>,
    depth: usize,
    viewport: Viewport,
}

impl Viewer {
    pub(crate) fn new(
        path: PathBuf,
        pages: Vec<Page>,
        prefetch: Prefetch,
        depth: usize,
        cache_limits: CacheLimits,
        viewport: Viewport,
    ) -> Self {
        let states = (0..pages.len()).map(|_| PageState::default()).collect();
        Self {
            path,
            pages,
            current: 0,
            prefetch,
            cache: PageCache::new(cache_limits.entries, cache_limits.bytes),
            states,
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
    pub(crate) fn set_viewport(&mut self, viewport: Viewport) {
        if self.viewport == viewport {
            return;
        }
        self.viewport = viewport;
        self.cache.clear();
        for state in &mut self.states {
            state.rejected = false;
        }
        self.prefetch.clear_pending();
        debug_log::log(format_args!(
            "viewer: viewport {}x{} filter={} mode={} upscale={} cache cleared",
            viewport.width,
            viewport.height,
            viewport.filter.as_str(),
            viewport.mode.as_str(),
            viewport.allow_upscale
        ));
    }

    /// Rebuilds the worker queue in decode order: the visible page (or pair),
    /// then the pages ahead. Cached pages are skipped, and rejected prefetch
    /// pages are not retried until they become visible.
    pub(crate) fn request_display(&mut self) {
        let mut jobs = Vec::new();
        let mut seen = HashSet::new();
        let current = self.current;
        self.push_job(&mut jobs, &mut seen, current, true);
        if self.viewport.mode != DisplayMode::Single && current + 1 < self.pages.len() {
            self.push_job(&mut jobs, &mut seen, current + 1, true);
        }
        for page in prefetch_targets(current, self.pages.len(), self.depth, self.viewport.mode) {
            self.push_job(&mut jobs, &mut seen, page, false);
        }
        self.prefetch.set_jobs(jobs);
    }

    fn needs_request(&self, page: usize) -> bool {
        page < self.pages.len() && !self.has_page(page) && self.states[page].error.is_none()
    }

    fn push_job(
        &self,
        jobs: &mut Vec<(usize, usize, String)>,
        seen: &mut HashSet<usize>,
        page: usize,
        visible: bool,
    ) {
        if page >= self.pages.len()
            || !seen.insert(page)
            || !self.needs_request(page)
            || (!visible && self.states[page].rejected)
        {
            return;
        }
        let page_info = &self.pages[page];
        jobs.push((page, page_info.entry, page_info.name.clone()));
    }

    /// Resizes a decoded page once to its display size and caches it. The
    /// display size depends only on the page's own orientation, so pairing it
    /// with a different neighbour never requires resizing it again. The
    /// decoded original is dropped.
    pub(crate) fn insert_decoded(&mut self, page: usize, image: Arc<RgbaImage>) {
        let (width, height) = image.dimensions();
        let orientation = Orientation::from_size(width, height);
        self.states[page].orientation = Some(orientation);
        let (cell_width, cell_height) = display_size(self.viewport, orientation);
        let target = fit(
            width,
            height,
            cell_width as usize,
            cell_height as usize,
            self.viewport.allow_upscale,
        );
        if target.0 == 0 || target.1 == 0 {
            // A page that cannot be fitted will never be displayable. Record
            // it as an error so waiting does not retry it forever.
            debug_log::log(format_args!(
                "viewer: page={} source={}x{} cannot be fitted for display",
                page + 1,
                width,
                height
            ));
            self.states[page].rejected = false;
            self.states[page].error = Some(format!("cannot fit {width}x{height} page for display"));
            return;
        }

        let display = if target == image.dimensions() {
            image
        } else {
            resized(&image, target, self.viewport.filter)
        };
        let display_size = display.dimensions();
        let step = if self.viewport.mode == DisplayMode::Single {
            1
        } else {
            2
        };
        let retained = self.cache.insert(page, display, self.current, step);
        self.states[page].rejected = !retained;
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
        self.states.get(page).and_then(|state| state.orientation)
    }

    fn layout(&self) -> Layout {
        let next = self.current + 1;
        layout_for(
            self.viewport.mode,
            next < self.pages.len(),
            self.orientation(self.current),
            self.orientation(next),
        )
    }

    /// True when the current page is paired with the next page on screen.
    fn shows_spread(&self) -> bool {
        self.layout() == Layout::Spread
    }

    /// Returns the page displayed together with the current page, if any.
    pub(crate) fn displayed_partner(&self) -> Option<usize> {
        self.shows_spread().then_some(self.current + 1)
    }

    /// 1-based page range shown in the title bar.
    pub(crate) fn page_range(&self) -> (usize, usize) {
        let start = self.current + 1;
        let end = self.displayed_partner().map_or(start, |page| page + 1);
        (start, end)
    }

    /// File or directory name shown in the title bar.
    pub(crate) fn name(&self) -> Cow<'_, str> {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| Cow::Owned(self.path.display().to_string()))
    }

    /// True while the current display is waiting for a page image.
    pub(crate) fn is_loading(&self) -> bool {
        matches!(self.frame(), Frame::Loading)
    }

    pub(crate) fn error(&self, page: usize) -> Option<&str> {
        self.states
            .get(page)
            .and_then(|state| state.error.as_deref())
    }

    pub(crate) fn fail_page(&mut self, page: usize, error: String) {
        if let Some(state) = self.states.get_mut(page) {
            state.error = Some(error);
            state.rejected = false;
        }
    }

    /// Returns the frame to draw. A spread stays `Loading` until both pages
    /// are cached, so the previous frame remains on screen instead of showing
    /// a right-only frame that gains a left page a moment later.
    pub(crate) fn frame(&self) -> Frame {
        if self.error(self.current).is_some() {
            return Frame::Failed;
        }
        let Some(right) = self.cache.get(self.current) else {
            return Frame::Loading;
        };
        let next = self.current + 1;
        let has_next = next < self.pages.len();
        frame_for(
            self.layout(),
            right,
            self.cache.get(next),
            !has_next || self.orientation(next).is_some(),
            self.error(next).is_some(),
        )
    }
}

/// Chooses the frame for `layout` once the current page image `right` is
/// cached.
///
/// `left` is the cached next page, `next_known` says whether no next page
/// remains or its orientation is known, and `next_failed` says whether the next
/// page failed to decode or to fit. A failed page ends the wait; a rejected
/// prefetch does not, because the page is requested again when it is visible.
fn frame_for(
    layout: Layout,
    right: Arc<RgbaImage>,
    left: Option<Arc<RgbaImage>>,
    next_known: bool,
    next_failed: bool,
) -> Frame {
    match layout {
        Layout::Single => Frame::Single(right),
        Layout::Spread => spread_frame(right, left, next_failed),
        Layout::BlankLeft => {
            if next_known || next_failed {
                Frame::RightOnly(right)
            } else {
                // Mixed mode still has to learn whether the next page is
                // portrait before the left cell can be left blank for good.
                Frame::Loading
            }
        }
    }
}

/// Classifies the current display from the mode and page orientations.
fn layout_for(
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

/// Draws both cells of a spread when the left page is cached, and keeps the
/// previous frame until it is available.
fn spread_frame(right: Arc<RgbaImage>, left: Option<Arc<RgbaImage>>, next_failed: bool) -> Frame {
    match left {
        Some(left) => Frame::Spread { left, right },
        None if next_failed => Frame::RightOnly(right),
        None => Frame::Loading,
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
        CacheLimits, Frame, Orientation, Prefetch, Viewer, Viewport, display_size, frame_for,
        layout_for, prefetch_targets, step_mixed,
    };
    use crate::archive::Page;
    use crate::config::{DisplayMode, ScaleFilter};

    fn test_viewer(mode: DisplayMode, pages: usize) -> Viewer {
        test_viewer_scaled(mode, pages, true)
    }

    fn test_viewer_scaled(mode: DisplayMode, pages: usize, allow_upscale: bool) -> Viewer {
        let pages = (0..pages)
            .map(|entry| Page {
                name: format!("{entry}.png"),
                entry,
            })
            .collect();
        Viewer::new(
            PathBuf::from("test.zip"),
            pages,
            Prefetch::idle(),
            1,
            CacheLimits {
                entries: 8,
                bytes: usize::MAX,
            },
            Viewport::new(1200, 900, ScaleFilter::Triangle, mode, allow_upscale),
        )
    }

    fn portrait() -> Arc<RgbaImage> {
        Arc::new(RgbaImage::new(600, 900))
    }

    fn frame_for_mode(
        mode: DisplayMode,
        has_next: bool,
        current: Option<Orientation>,
        next: Option<Orientation>,
        right: Arc<RgbaImage>,
        left: Option<Arc<RgbaImage>>,
        next_failed: bool,
    ) -> Frame {
        frame_for(
            layout_for(mode, has_next, current, next),
            right,
            left,
            !has_next || next.is_some(),
            next_failed,
        )
    }

    #[test]
    fn request_display_orders_visible_then_prefetch() {
        let mut single = test_viewer(DisplayMode::Single, 10);
        single.request_display();
        assert_eq!(single.prefetch.queued_pages(), vec![0, 1]);

        single.go_to(2);
        single.request_display();
        assert_eq!(single.prefetch.queued_pages(), vec![2, 3]);

        let mut spread = test_viewer(DisplayMode::Spread, 10);
        spread.request_display();
        assert_eq!(spread.prefetch.queued_pages(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn rejected_visible_page_is_requested_again() {
        let mut viewer = test_viewer(DisplayMode::Single, 4);
        viewer.states[0].rejected = true;

        viewer.request_display();

        assert_eq!(viewer.prefetch.queued_pages(), vec![0, 1]);
    }

    #[test]
    fn failed_page_draws_a_blank_frame() {
        let mut viewer = test_viewer(DisplayMode::Single, 4);
        assert!(matches!(viewer.frame(), Frame::Loading));

        viewer.fail_page(0, "failed".to_string());
        assert!(matches!(viewer.frame(), Frame::Failed));
        assert!(!viewer.is_loading());
    }

    #[test]
    fn frame_draws_a_single_page_in_single_mode() {
        let right = portrait();

        match frame_for_mode(
            DisplayMode::Single,
            true,
            Some(Orientation::Portrait),
            Some(Orientation::Portrait),
            right.clone(),
            None,
            false,
        ) {
            Frame::Single(image) => assert!(Arc::ptr_eq(&image, &right)),
            _ => panic!("expected a single page"),
        }
    }

    #[test]
    fn frame_waits_until_both_spread_pages_are_ready() {
        let right = portrait();
        let left = portrait();
        let portrait = Some(Orientation::Portrait);

        assert!(matches!(
            frame_for_mode(
                DisplayMode::Spread,
                true,
                portrait,
                portrait,
                right.clone(),
                None,
                false
            ),
            Frame::Loading
        ));
        match frame_for_mode(
            DisplayMode::Spread,
            true,
            portrait,
            portrait,
            right.clone(),
            Some(left.clone()),
            false,
        ) {
            Frame::Spread {
                left: drawn_left,
                right: drawn_right,
            } => {
                assert!(Arc::ptr_eq(&drawn_left, &left));
                assert!(Arc::ptr_eq(&drawn_right, &right));
            }
            _ => panic!("expected a spread"),
        }
        assert!(matches!(
            frame_for_mode(
                DisplayMode::Spread,
                true,
                portrait,
                portrait,
                right.clone(),
                None,
                true
            ),
            Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
        ));
    }

    #[test]
    fn frame_uses_only_the_right_cell_on_the_last_page() {
        let right = portrait();

        assert!(matches!(
            frame_for_mode(
                DisplayMode::Spread,
                false,
                Some(Orientation::Portrait),
                None,
                right.clone(),
                None,
                false
            ),
            Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
        ));
    }

    #[test]
    fn frame_shows_a_landscape_page_alone_in_mixed_mode() {
        let right = portrait();

        match frame_for_mode(
            DisplayMode::Mixed,
            true,
            Some(Orientation::Landscape),
            Some(Orientation::Portrait),
            right.clone(),
            None,
            false,
        ) {
            Frame::Single(image) => assert!(Arc::ptr_eq(&image, &right)),
            _ => panic!("expected a single page"),
        }
    }

    #[test]
    fn frame_waits_for_a_mixed_portrait_pair() {
        let right = portrait();
        let left = portrait();
        let portrait = Some(Orientation::Portrait);

        assert!(matches!(
            frame_for_mode(
                DisplayMode::Mixed,
                true,
                portrait,
                portrait,
                right.clone(),
                None,
                false
            ),
            Frame::Loading
        ));
        match frame_for_mode(
            DisplayMode::Mixed,
            true,
            portrait,
            portrait,
            right.clone(),
            Some(left.clone()),
            false,
        ) {
            Frame::Spread {
                left: drawn_left,
                right: drawn_right,
            } => {
                assert!(Arc::ptr_eq(&drawn_left, &left));
                assert!(Arc::ptr_eq(&drawn_right, &right));
            }
            _ => panic!("expected a spread"),
        }
        assert!(matches!(
            frame_for_mode(
                DisplayMode::Mixed,
                true,
                portrait,
                portrait,
                right.clone(),
                None,
                true
            ),
            Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
        ));
    }

    #[test]
    fn frame_waits_for_an_unknown_mixed_neighbour() {
        let right = portrait();
        let portrait = Some(Orientation::Portrait);

        assert!(matches!(
            frame_for_mode(
                DisplayMode::Mixed,
                true,
                portrait,
                None,
                right.clone(),
                None,
                false
            ),
            Frame::Loading
        ));
        assert!(matches!(
            frame_for_mode(
                DisplayMode::Mixed,
                true,
                portrait,
                Some(Orientation::Landscape),
                right.clone(),
                None,
                false
            ),
            Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
        ));
        assert!(matches!(
            frame_for_mode(
                DisplayMode::Mixed,
                false,
                portrait,
                None,
                right.clone(),
                None,
                false
            ),
            Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
        ));
        assert!(matches!(
            frame_for_mode(
                DisplayMode::Mixed,
                true,
                portrait,
                None,
                right.clone(),
                None,
                true
            ),
            Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
        ));
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
        viewer.states[1].rejected = true;

        assert!(matches!(viewer.frame(), Frame::Loading));

        viewer.fail_page(1, "failed".to_string());
        assert!(matches!(viewer.frame(), Frame::RightOnly(_)));
    }

    #[test]
    fn empty_fitted_page_ends_in_errors_not_rejections() {
        let mut viewer = test_viewer(DisplayMode::Spread, 4);
        viewer.insert_decoded(0, Arc::new(RgbaImage::new(0, 0)));

        assert!(viewer.error(0).is_some());
        assert!(!viewer.states[0].rejected);
        assert!(!viewer.needs_request(0));
    }

    #[test]
    fn last_spread_page_uses_only_the_right_cell() {
        let mut viewer = test_viewer(DisplayMode::Spread, 1);
        viewer.insert_decoded(0, portrait());

        assert!(matches!(viewer.frame(), Frame::RightOnly(_)));
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
        let single = Viewport::new(1200, 900, ScaleFilter::Triangle, DisplayMode::Single, true);
        assert_eq!(display_size(single, Orientation::Portrait), (1200, 900));
        assert_eq!(display_size(single, Orientation::Landscape), (1200, 900));

        let spread = Viewport::new(1200, 900, ScaleFilter::Triangle, DisplayMode::Spread, true);
        assert_eq!(display_size(spread, Orientation::Portrait), (600, 900));
        assert_eq!(display_size(spread, Orientation::Landscape), (600, 900));

        let mixed = Viewport::new(1200, 900, ScaleFilter::Triangle, DisplayMode::Mixed, true);
        assert_eq!(display_size(mixed, Orientation::Portrait), (600, 900));
        assert_eq!(display_size(mixed, Orientation::Landscape), (1200, 900));
    }

    #[test]
    fn upscale_setting_controls_small_pages() {
        // 100x60 is smaller than the 1200x900 viewport.
        let mut enlarged = test_viewer_scaled(DisplayMode::Single, 1, true);
        enlarged.insert_decoded(0, Arc::new(RgbaImage::new(100, 60)));
        assert_eq!(
            enlarged.cache.get(0).unwrap().dimensions(),
            (1200, 720),
            "upscaling enabled must enlarge the page to the viewport"
        );

        let mut kept = test_viewer_scaled(DisplayMode::Single, 1, false);
        kept.insert_decoded(0, Arc::new(RgbaImage::new(100, 60)));
        assert_eq!(
            kept.cache.get(0).unwrap().dimensions(),
            (100, 60),
            "upscaling disabled must keep the page at its original size"
        );
    }

    #[test]
    fn square_pages_are_not_portrait() {
        assert_eq!(Orientation::from_size(100, 100), Orientation::Landscape);
        assert_eq!(Orientation::from_size(100, 101), Orientation::Portrait);
        assert_eq!(Orientation::from_size(101, 100), Orientation::Landscape);
    }

    #[test]
    fn displayed_partner_follows_the_shown_spread() {
        let mut spread = test_viewer(DisplayMode::Spread, 2);
        spread.states[0].orientation = Some(Orientation::Portrait);
        spread.states[1].orientation = Some(Orientation::Portrait);
        assert_eq!(spread.displayed_partner(), Some(1));
        assert_eq!(spread.page_range(), (1, 2));

        assert_eq!(
            test_viewer(DisplayMode::Single, 2).displayed_partner(),
            None
        );
        assert_eq!(
            test_viewer(DisplayMode::Spread, 1).displayed_partner(),
            None
        );

        let mut mixed = test_viewer(DisplayMode::Mixed, 2);
        mixed.states[0].orientation = Some(Orientation::Portrait);
        mixed.states[1].orientation = Some(Orientation::Landscape);
        assert_eq!(mixed.displayed_partner(), None);

        mixed.states[1].orientation = Some(Orientation::Portrait);
        assert_eq!(mixed.displayed_partner(), Some(1));
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
}
