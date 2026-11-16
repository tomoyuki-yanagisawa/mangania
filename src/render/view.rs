use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbaImage;

use crate::config::ScaleFilter;
use crate::debug_log;
use crate::render::cache::{ScaledCache, ScaledKey};

thread_local! {
    /// Reused so repeated page resizes do not reallocate internal buffers.
    static RESIZER: RefCell<Resizer> = RefCell::new(Resizer::new());
}

const BACKGROUND: u32 = 0x20_20_20;

/// A page ready to draw.
#[derive(Clone, Copy)]
pub struct RenderPage<'a> {
    /// Page index in the archive.
    pub index: usize,
    /// Display-sized image.
    pub image: &'a RgbaImage,
    /// Original page dimensions, used to lay out spreads.
    pub original_size: (u32, u32),
}

/// Fills the window with the background color.
pub fn blank(buffer: &mut [u32]) {
    buffer.fill(BACKGROUND);
}

/// Fits the given pages side by side, preserving their relative sizes.
///
/// Uses a temporary cache and the default filter. The GUI retains its cache
/// between calls via [`render_pages_cached`].
pub fn render_pages(buffer: &mut [u32], width: usize, height: usize, pages: &[Option<&RgbaImage>]) {
    let pages: Vec<_> = pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            page.map(|image| RenderPage {
                index,
                image,
                original_size: image.dimensions(),
            })
        })
        .collect();
    render_pages_cached(
        buffer,
        width,
        height,
        &pages,
        ScaleFilter::Triangle,
        0,
        &mut ScaledCache::new(0, 0),
    );
}

/// Returns the display size of an image fitted into the viewport while
/// preserving its aspect ratio.
pub fn fit(width: u32, height: u32, max_width: usize, max_height: usize) -> (u32, u32) {
    if width == 0 || height == 0 || max_width == 0 || max_height == 0 {
        return (0, 0);
    }
    let scale = (max_width as f64 / width as f64).min(max_height as f64 / height as f64);
    scaled(width, height, scale)
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
        .use_alpha(false);
    let result = RESIZER.with(|resizer| resizer.borrow_mut().resize(image, &mut output, &options));
    let resized = match result {
        Ok(()) => Arc::new(output),
        Err(error) => {
            debug_log::log(format_args!(
                "view: fast resize failed ({error}); using the image crate"
            ));
            Arc::new(image::imageops::resize(
                image,
                target.0,
                target.1,
                filter.to_filter_type(),
            ))
        }
    };
    debug_log::log(format_args!(
        "view: resize {}x{} -> {}x{} filter={} in {:.1}ms",
        image.width(),
        image.height(),
        target.0,
        target.1,
        filter.as_str(),
        start.elapsed().as_secs_f64() * 1000.0
    ));
    resized
}

fn resize_algorithm(filter: ScaleFilter) -> ResizeAlg {
    match filter {
        ScaleFilter::Nearest => ResizeAlg::Nearest,
        ScaleFilter::Triangle => ResizeAlg::Convolution(FilterType::Bilinear),
        ScaleFilter::CatmullRom => ResizeAlg::Convolution(FilterType::CatmullRom),
        ScaleFilter::Gaussian => ResizeAlg::Convolution(FilterType::Gaussian),
        ScaleFilter::Lanczos3 => ResizeAlg::Convolution(FilterType::Lanczos3),
    }
}

/// Fits the given pages side by side and reuses resized pages from `cache`.
///
/// `pages` holds the page index, its display-sized image, and the original
/// dimensions used for layout. Entries for the pages being drawn are kept even
/// when the cache is full.
pub fn render_pages_cached(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    pages: &[Option<RenderPage<'_>>],
    filter: ScaleFilter,
    current: usize,
    cache: &mut ScaledCache,
) {
    blank(buffer);
    if width == 0 || height == 0 {
        return;
    }

    let total_width: u32 = pages
        .iter()
        .flatten()
        .map(|page| page.original_size.0)
        .sum();
    let max_height = pages
        .iter()
        .flatten()
        .map(|page| page.original_size.1)
        .max()
        .unwrap_or(0);
    if total_width == 0 || max_height == 0 {
        debug_log::log(format_args!(
            "view: empty page (width={total_width} height={max_height})"
        ));
        return;
    }

    let scale = (width as f64 / total_width as f64).min(height as f64 / max_height as f64);
    let (draw_width, draw_height) = scaled(total_width, max_height, scale);
    let start_x = (width as isize - draw_width as isize) / 2;
    let start_y = (height as isize - draw_height as isize) / 2;

    let mut x = start_x;
    let keep: Vec<usize> = pages.iter().flatten().map(|page| page.index).collect();
    for page in pages.iter().flatten() {
        let (target_width, target_height) =
            scaled(page.original_size.0, page.original_size.1, scale);
        let y = start_y + (draw_height as isize - target_height as isize) / 2;
        let target = (target_width, target_height);
        if target == page.image.dimensions() {
            blit(buffer, width, height, page.image, x, y);
        } else {
            let key = ScaledKey::new(page.index, target_width, target_height, filter);
            let display = if let Some(resized) = cache.get(key) {
                debug_log::log(format_args!(
                    "view: resize cache hit page={} size={}x{} filter={}",
                    page.index + 1,
                    target_width,
                    target_height,
                    filter.as_str()
                ));
                resized
            } else {
                let resized = resized(page.image, target, filter);
                cache.insert(key, Arc::clone(&resized), &keep, current);
                resized
            };
            blit(buffer, width, height, &display, x, y);
        }
        x += target_width as isize;
    }
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

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::resized;
    use crate::config::ScaleFilter;

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
