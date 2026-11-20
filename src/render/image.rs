use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbaImage;

use crate::config::ScaleFilter;
use crate::debug_log;
use crate::render::viewport::{Orientation, Viewport, display_size};

const BACKGROUND: u32 = 0x20_20_20;

thread_local! {
    /// Reused so repeated page resizes do not reallocate internal buffers.
    static RESIZER: RefCell<Resizer> = RefCell::new(Resizer::new());
}

/// Fills the window with the background color.
pub fn blank(buffer: &mut [u32]) {
    buffer.fill(BACKGROUND);
}

/// Draws one page centered, or two pages in left/right cells.
///
/// Spread pages meet at the middle gutter: the left page is aligned to the
/// right edge of the left cell, and the right page to the left edge of the
/// right cell. Missing pages leave their cell blank. Images are expected to
/// be display-sized already; this function only copies pixels.
pub fn render_pages(buffer: &mut [u32], width: usize, height: usize, pages: &[Option<&RgbaImage>]) {
    blank(buffer);
    if width == 0 || height == 0 {
        return;
    }

    if let [page] = pages {
        if let Some(image) = page {
            place(buffer, width, height, image, Center::Full);
        }
        return;
    }

    let gutter = width / 2;
    if let Some(image) = pages.first().copied().flatten() {
        place(buffer, width, height, image, Center::LeftCell { gutter });
    }
    if let Some(image) = pages.get(1).copied().flatten() {
        place(buffer, width, height, image, Center::RightCell { gutter });
    }
}

enum Center {
    Full,
    LeftCell { gutter: usize },
    RightCell { gutter: usize },
}

fn place(buffer: &mut [u32], width: usize, height: usize, image: &RgbaImage, center: Center) {
    let x = match center {
        Center::Full => (width as isize - image.width() as isize) / 2,
        Center::LeftCell { gutter } => gutter as isize - image.width() as isize,
        Center::RightCell { gutter } => gutter as isize,
    };
    let y = (height as isize - image.height() as isize) / 2;
    blit(buffer, width, height, image, x, y);
}

/// Returns the display size of an image fitted into the viewport while
/// preserving its aspect ratio.
///
/// When `allow_upscale` is false, the scale is clamped to at most 1.0 so
/// smaller images keep their original size. Shrinking always works.
pub fn fit(
    width: u32,
    height: u32,
    max_width: usize,
    max_height: usize,
    allow_upscale: bool,
) -> (u32, u32) {
    if width == 0 || height == 0 || max_width == 0 || max_height == 0 {
        return (0, 0);
    }
    let mut scale = (max_width as f64 / width as f64).min(max_height as f64 / height as f64);
    if !allow_upscale {
        scale = scale.min(1.0);
    }
    scaled(width, height, scale)
}

fn scaled(width: u32, height: u32, scale: f64) -> (u32, u32) {
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

fn blit(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    image: &RgbaImage,
    offset_x: isize,
    offset_y: isize,
) {
    let image_width = image.width() as usize;
    let image_height = image.height() as usize;
    if width == 0 || height == 0 || image_width == 0 || image_height == 0 {
        return;
    }

    let x_start = (-offset_x).max(0) as usize;
    let y_start = (-offset_y).max(0) as usize;
    let x_end = (width as isize - offset_x).min(image_width as isize).max(0) as usize;
    let y_end = (height as isize - offset_y)
        .min(image_height as isize)
        .max(0) as usize;
    if x_start >= x_end || y_start >= y_end {
        return;
    }

    let dest_x = (offset_x + x_start as isize) as usize;
    let row_len = x_end - x_start;
    let source = image.as_raw();
    for y in y_start..y_end {
        let dest_y = (offset_y + y as isize) as usize;
        let source_start = (y * image_width + x_start) * 4;
        let source_row = &source[source_start..source_start + row_len * 4];
        let dest_row = &mut buffer[dest_y * width + dest_x..][..row_len];
        let (pixels, _) = source_row.as_chunks::<4>();
        for (dest, pixel) in dest_row.iter_mut().zip(pixels) {
            *dest = (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]);
        }
    }
}

/// A decoded page resized for the current display target.
pub struct PreparedPage {
    pub image: Arc<RgbaImage>,
    pub orientation: Orientation,
}

/// Resizes a decoded page once to its display size.
///
/// The display size depends only on the page's own orientation, so pairing it
/// with a different neighbour never requires resizing it again. The decoded
/// original is dropped.
pub(crate) fn prepare(image: RgbaImage, viewport: Viewport) -> Result<PreparedPage, String> {
    let (width, height) = image.dimensions();
    let orientation = Orientation::from_size(width, height);
    let (cell_width, cell_height) = display_size(viewport, orientation);
    let target = fit(
        width,
        height,
        cell_width as usize,
        cell_height as usize,
        viewport.allow_upscale,
    );
    if target.0 == 0 || target.1 == 0 {
        return Err(format!("cannot fit {width}x{height} page for display"));
    }
    let image = if target == image.dimensions() {
        Arc::new(image)
    } else {
        resized(&image, target, viewport.filter)
    };
    Ok(PreparedPage { image, orientation })
}

/// Resizes an image to a display target and logs the cost.
pub(crate) fn resized(
    image: &RgbaImage,
    target: (u32, u32),
    filter: ScaleFilter,
) -> Arc<RgbaImage> {
    let start = Instant::now();
    let mut output = RgbaImage::new(target.0, target.1);
    let options = ResizeOptions::new()
        .resize_alg(resize_algorithm(filter))
        // Pages are drawn without alpha blending, so the premultiply and
        // unpremultiply passes are unnecessary.
        .use_alpha(false);
    if let Err(error) =
        RESIZER.with(|resizer| resizer.borrow_mut().resize(image, &mut output, &options))
    {
        // RGBA8 with a non-zero target is always supported, so this should not
        // happen. Leave the zero-filled target and log it instead of panicking.
        debug_log::log(format_args!("resize: fast_image_resize failed: {error}"));
    }
    debug_log::log(format_args!(
        "resize: {}x{} -> {}x{} filter={} in {:.1}ms",
        image.width(),
        image.height(),
        target.0,
        target.1,
        filter.as_str(),
        start.elapsed().as_secs_f64() * 1000.0
    ));
    Arc::new(output)
}

fn resize_algorithm(filter: ScaleFilter) -> ResizeAlg {
    match filter {
        ScaleFilter::Nearest => ResizeAlg::Nearest,
        ScaleFilter::Triangle => ResizeAlg::Convolution(FilterType::Bilinear),
        ScaleFilter::Hamming => ResizeAlg::Convolution(FilterType::Hamming),
        ScaleFilter::CatmullRom => ResizeAlg::Convolution(FilterType::CatmullRom),
        ScaleFilter::Mitchell => ResizeAlg::Convolution(FilterType::Mitchell),
        ScaleFilter::Gaussian => ResizeAlg::Convolution(FilterType::Gaussian),
        ScaleFilter::Lanczos3 => ResizeAlg::Convolution(FilterType::Lanczos3),
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::{prepare, resized};
    use crate::config::{DisplayMode, ScaleFilter};
    use crate::render::viewport::{Orientation, Viewport};

    fn viewport(mode: DisplayMode, allow_upscale: bool) -> Viewport {
        Viewport::new(1200, 900, ScaleFilter::Triangle, mode, allow_upscale)
    }

    #[test]
    fn prepare_uses_the_display_size_for_the_orientation() {
        let prepared = prepare(
            RgbaImage::new(600, 900),
            viewport(DisplayMode::Spread, true),
        )
        .expect("prepare portrait");
        assert_eq!(prepared.orientation, Orientation::Portrait);
        assert_eq!(prepared.image.dimensions(), (600, 900));

        let prepared = prepare(
            RgbaImage::new(1200, 600),
            viewport(DisplayMode::Spread, true),
        )
        .expect("prepare landscape");
        assert_eq!(prepared.orientation, Orientation::Landscape);
        assert_eq!(prepared.image.dimensions(), (600, 300));
    }

    #[test]
    fn prepare_enlarges_only_when_allowed() {
        let enlarged = prepare(RgbaImage::new(100, 60), viewport(DisplayMode::Single, true))
            .expect("prepare enlarged");
        assert_eq!(enlarged.image.dimensions(), (1200, 720));

        let kept = prepare(
            RgbaImage::new(100, 60),
            viewport(DisplayMode::Single, false),
        )
        .expect("prepare kept");
        assert_eq!(kept.image.dimensions(), (100, 60));
    }

    #[test]
    fn prepare_rejects_a_zero_sized_page() {
        assert!(prepare(RgbaImage::new(0, 0), viewport(DisplayMode::Single, true)).is_err());
    }

    #[test]
    fn resizes_with_every_filter() {
        let image = RgbaImage::from_pixel(7, 5, Rgba([10, 20, 30, 255]));

        for filter in ScaleFilter::ALL {
            let output = resized(&image, (3, 2), filter);
            assert_eq!(output.dimensions(), (3, 2), "{}", filter.as_str());
            assert!(
                output.pixels().all(|pixel| pixel.0 == [10, 20, 30, 255]),
                "{}",
                filter.as_str()
            );
        }
    }
}
