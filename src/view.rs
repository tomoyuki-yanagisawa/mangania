use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;

use crate::cache::{ScaledCache, ScaledKey};
use crate::config::ScaleFilter;
use crate::debug_log;

const BACKGROUND: u32 = 0x20_20_20;

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
        .map(|page| page.map(|image| (0, image)))
        .collect();
    render_impl(buffer, width, height, &pages, ScaleFilter::Triangle, None);
}

/// Fits the given pages side by side and reuses resized pages from `cache`.
///
/// `pages` holds the page index paired with its decoded image. The index is
/// part of the cache key so a stale resize cannot be reused after reopening.
pub fn render_pages_cached(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    pages: &[Option<(usize, &RgbaImage)>],
    filter: ScaleFilter,
    cache: &mut ScaledCache,
) {
    render_impl(buffer, width, height, pages, filter, Some(cache));
}

fn render_impl(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    pages: &[Option<(usize, &RgbaImage)>],
    filter: ScaleFilter,
    mut cache: Option<&mut ScaledCache>,
) {
    blank(buffer);
    if width == 0 || height == 0 {
        return;
    }

    let total_width: u32 = pages.iter().flatten().map(|(_, image)| image.width()).sum();
    let max_height = pages
        .iter()
        .flatten()
        .map(|(_, image)| image.height())
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
    for (page, image) in pages.iter().flatten() {
        let (target_width, target_height) = scaled(image.width(), image.height(), scale);
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
            cache.as_deref_mut(),
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
    cache: Option<&mut ScaledCache>,
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
            cache.insert(key, Arc::clone(&resized));
            resized
        }
    } else {
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
