//! Display target and page orientation.

use crate::config::{DisplayMode, ScaleFilter};

/// The window size and display settings a page is prepared for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) filter: ScaleFilter,
    pub(crate) mode: DisplayMode,
    pub(crate) allow_upscale: bool,
}

impl Viewport {
    pub fn new(
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

/// A page is portrait when it is taller than it is wide. Square pages are
/// treated as landscape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    pub(crate) fn from_size(width: u32, height: u32) -> Self {
        if height > width {
            Self::Portrait
        } else {
            Self::Landscape
        }
    }
}

/// Size of the display image for a page. Single pages use the whole window.
/// Spread pages use one half of the window. Mixed mode chooses by the page's
/// own orientation, never by its neighbour.
pub(crate) fn display_size(viewport: Viewport, orientation: Orientation) -> (u32, u32) {
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

#[cfg(test)]
mod tests {
    use super::{Orientation, Viewport, display_size};
    use crate::config::{DisplayMode, ScaleFilter};

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
    fn square_pages_are_not_portrait() {
        assert_eq!(Orientation::from_size(100, 100), Orientation::Landscape);
        assert_eq!(Orientation::from_size(100, 101), Orientation::Portrait);
        assert_eq!(Orientation::from_size(101, 100), Orientation::Landscape);
    }
}
