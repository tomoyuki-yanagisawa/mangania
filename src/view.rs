use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;

use crate::cache::{ScaledCache, ScaledKey};
use crate::config::ScaleFilter;
use crate::debug_log;

const BACKGROUND: u32 = 0x20_20_20;

/// Page index, display-sized image, and original dimensions for layout.
pub type RenderPage<'a> = (usize, &'a RgbaImage, (u32, u32));

/// Fills the window with the background color.
pub fn blank(buffer: &mut [u32]) {
    buffer.fill(BACKGROUND);
}

/// Fits one page into the window.
pub fn render(buffer: &mut [u32], width: usize, height: usize, image: &RgbaImage) {
    render_pages(buffer, width, height, &[Some(image)]);
}

/// Fits two pages side by side. `left` is drawn first, then `right`.
pub fn render_spread(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    left: Option<&RgbaImage>,
    right: Option<&RgbaImage>,
) {
    render_pages(buffer, width, height, &[left, right]);
}

/// Fits the given pages side by side, preserving their relative sizes.
///
/// This path does not cache resized pages and uses the default filter. The GUI
/// uses [`render_pages_cached`] instead.
pub fn render_pages(buffer: &mut [u32], width: usize, height: usize, pages: &[Option<&RgbaImage>]) {
    let pages: Vec<_> = pages
        .iter()
        .map(|page| page.map(|image| (0, image, image.dimensions())))
        .collect();
    render_impl(
        buffer,
        width,
        height,
        &pages,
        ScaleFilter::Triangle,
        0,
        None,
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
    let resized = Arc::new(image::imageops::resize(
        image,
        target.0,
        target.1,
        filter.to_filter_type(),
    ));
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
    render_impl(buffer, width, height, pages, filter, current, Some(cache));
}

fn render_impl(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    pages: &[Option<RenderPage<'_>>],
    filter: ScaleFilter,
    current: usize,
    mut cache: Option<&mut ScaledCache>,
) {
    blank(buffer);
    if width == 0 || height == 0 {
        return;
    }

    let total_width: u32 = pages
        .iter()
        .flatten()
        .map(|(_, _, original)| original.0)
        .sum();
    let max_height = pages
        .iter()
        .flatten()
        .map(|(_, _, original)| original.1)
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
    let keep: Vec<usize> = pages.iter().flatten().map(|(page, _, _)| *page).collect();
    for (page, image, original) in pages.iter().flatten() {
        let (target_width, target_height) = scaled(original.0, original.1, scale);
        let y = start_y + (draw_height as isize - target_height as isize) / 2;
        blit_scaled(
            buffer,
            width,
            height,
            *page,
            image,
            (x, y),
            (target_width, target_height),
            filter,
            current,
            cache.as_deref_mut(),
            &keep,
        );
        x += target_width as isize;
    }
}

fn scaled(width: u32, height: u32, scale: f64) -> (u32, u32) {
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

#[allow(clippy::too_many_arguments)]
fn blit_scaled(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    page: usize,
    image: &RgbaImage,
    position: (isize, isize),
    target: (u32, u32),
    filter: ScaleFilter,
    current: usize,
    cache: Option<&mut ScaledCache>,
    keep: &[usize],
) {
    if target == image.dimensions() {
        blit(buffer, width, height, image, position.0, position.1);
        return;
    }

    let resized = if let Some(cache) = cache {
        let key = ScaledKey::new(page, target.0, target.1, filter);
        if let Some(resized) = cache.get(key) {
            debug_log::log(format_args!(
                "view: resize cache hit page={} size={}x{} filter={}",
                page + 1,
                target.0,
                target.1,
                filter.as_str()
            ));
            resized
        } else {
            let resized = resized(image, target, filter);
            cache.insert(key, Arc::clone(&resized), keep, current);
            resized
        }
    } else {
        resized(image, target, filter)
    };
    blit(buffer, width, height, &resized, position.0, position.1);
}

fn blit(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    image: &RgbaImage,
    offset_x: isize,
    offset_y: isize,
) {
    let x_start = (-offset_x).max(0) as u32;
    let y_start = (-offset_y).max(0) as u32;
    let x_end = (width as isize - offset_x)
        .min(image.width() as isize)
        .max(0) as u32;
    let y_end = (height as isize - offset_y)
        .min(image.height() as isize)
        .max(0) as u32;

    for y in y_start..y_end {
        let dest_y = (offset_y + y as isize) as usize;
        for x in x_start..x_end {
            let dest_x = (offset_x + x as isize) as usize;
            let [red, green, blue, _alpha] = image.get_pixel(x, y).0;
            buffer[dest_y * width + dest_x] =
                ((red as u32) << 16) | ((green as u32) << 8) | blue as u32;
        }
    }
}
