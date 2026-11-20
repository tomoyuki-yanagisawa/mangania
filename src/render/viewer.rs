use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use image::RgbaImage;

use crate::archive::Page;
use crate::config::DisplayMode;
use crate::debug_log;
use crate::input::navigation;
use crate::render::cache::PageCache;
use crate::render::image::PreparedPage;
use crate::render::prefetch::Prefetch;
use crate::render::viewport::{Orientation, Viewport};

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

/// Per-page state that survives display-cache clears.
#[derive(Default, Clone)]
struct PageState {
    orientation: Option<Orientation>,
    error: Option<String>,
    /// The decoded page was not kept in the display cache. Prefetching skips
    /// it until it becomes visible; evicting a cached page does not set it.
    rejected: bool,
    /// A transient cache failure was already retried once.
    retried: bool,
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
        let states = vec![PageState::default(); pages.len()];
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
            state.retried = false;
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
        self.prefetch.set_jobs(self.viewport, jobs);
    }

    fn needs_request(&self, page: usize) -> bool {
        page < self.pages.len() && !self.has_page(page) && self.error(page).is_none()
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

    /// Stores a page the worker prepared for `viewport`. Results prepared for
    /// another viewport are stale: the page is requested again for the new
    /// target, so keeping them would show an image at the wrong size.
    pub(crate) fn insert_prepared(
        &mut self,
        page: usize,
        viewport: Viewport,
        prepared: PreparedPage,
    ) -> bool {
        if viewport != self.viewport {
            return false;
        }
        self.states[page].orientation = Some(prepared.orientation);
        self.states[page].retried = false;
        let step = if self.viewport.mode == DisplayMode::Single {
            1
        } else {
            2
        };
        let retained = self.cache.insert(page, prepared.image, self.current, step);
        self.states[page].rejected = !retained;
        debug_log::log(format_args!(
            "viewer: display page={} cached={retained}",
            page + 1
        ));
        true
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

    pub(crate) fn fail_page(&mut self, page: usize, viewport: Viewport, error: String) -> bool {
        if viewport != self.viewport {
            return false;
        }
        if let Some(state) = self.states.get_mut(page) {
            state.error = Some(error);
            state.rejected = false;
        }
        true
    }

    /// Records a transient cache failure. The page is requested again when it
    /// is visible; a second failure becomes a normal error so a broken entry
    /// cannot loop.
    pub(crate) fn retry_page(&mut self, page: usize, viewport: Viewport, message: String) -> bool {
        if viewport != self.viewport {
            return false;
        }
        let Some(state) = self.states.get_mut(page) else {
            return false;
        };
        state.rejected = true;
        if state.retried {
            state.error = Some(message);
        } else {
            state.retried = true;
            debug_log::log(format_args!(
                "viewer: page={} cache failure, retrying: {message}",
                page + 1
            ));
        }
        true
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
mod tests;
