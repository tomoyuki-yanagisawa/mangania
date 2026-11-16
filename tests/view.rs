use image::{Rgba, RgbaImage};
use mangania::config::ScaleFilter;
use mangania::render::cache::ScaledCache;
use mangania::render::view;

const BACKGROUND: u32 = 0x20_20_20;

fn red() -> RgbaImage {
    RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]))
}

fn blue() -> RgbaImage {
    RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255]))
}

fn page(index: usize, image: &RgbaImage) -> view::RenderPage<'_> {
    view::RenderPage {
        index,
        image,
        original_size: image.dimensions(),
    }
}

fn page_with_size(
    index: usize,
    image: &RgbaImage,
    original_size: (u32, u32),
) -> view::RenderPage<'_> {
    view::RenderPage {
        index,
        image,
        original_size,
    }
}

#[test]
fn renders_a_single_page_centered() {
    let mut buffer = vec![0u32; 8 * 4];
    view::render_pages(&mut buffer, 8, 4, &[Some(&red())]);

    assert_eq!(buffer[0], BACKGROUND);
    assert_eq!(buffer[2 * 8 + 3], 0xff_0000);
    assert_eq!(buffer[3 * 8 + 7], BACKGROUND);
}

#[test]
fn renders_two_pages_side_by_side() {
    let mut buffer = vec![0u32; 8 * 4];
    view::render_pages(&mut buffer, 8, 4, &[Some(&blue()), Some(&red())]);

    assert_eq!(buffer[0], 0x00_00ff);
    assert_eq!(buffer[7], 0xff_0000);
}

#[test]
fn centers_a_single_page_in_spread_mode() {
    let mut buffer = vec![0u32; 8 * 4];
    view::render_pages(&mut buffer, 8, 4, &[None, Some(&red())]);

    assert_eq!(buffer[0], BACKGROUND);
    assert_eq!(buffer[2 * 8 + 3], 0xff_0000);
    assert_eq!(buffer[3 * 8 + 7], BACKGROUND);
}

#[test]
fn fits_previews_inside_the_viewport() {
    assert_eq!(view::fit(2, 1, 8, 8), (8, 4));
    assert_eq!(view::fit(1, 2, 8, 8), (4, 8));
    assert_eq!(view::fit(8, 8, 4, 2), (2, 2));
    assert_eq!(view::fit(0, 100, 8, 8), (0, 0));
    assert_eq!(view::fit(100, 100, 0, 8), (0, 0));
}

#[test]
fn reuses_scaled_pages_by_page_and_filter() {
    let image = red();
    let mut buffer = vec![0u32; 8 * 4];
    let mut cache = ScaledCache::new(4, usize::MAX);

    view::render_pages_cached(
        &mut buffer,
        8,
        4,
        &[Some(page(0, &image))],
        ScaleFilter::Triangle,
        0,
        &mut cache,
    );
    assert_eq!(cache.len(), 1);
    assert_eq!(buffer[0], BACKGROUND);
    assert_eq!(buffer[2 * 8 + 3], 0xff_0000);

    view::render_pages_cached(
        &mut buffer,
        8,
        4,
        &[Some(page(0, &image))],
        ScaleFilter::Triangle,
        0,
        &mut cache,
    );
    assert_eq!(cache.len(), 1);

    view::render_pages_cached(
        &mut buffer,
        8,
        4,
        &[Some(page(0, &image))],
        ScaleFilter::Lanczos3,
        0,
        &mut cache,
    );
    assert_eq!(cache.len(), 2);

    view::render_pages_cached(
        &mut buffer,
        8,
        4,
        &[Some(page(1, &image))],
        ScaleFilter::Triangle,
        0,
        &mut cache,
    );
    assert_eq!(cache.len(), 3);
}

#[test]
fn retains_visible_spread_at_each_target_size_over_cache_budget() {
    let left = blue();
    let right = red();
    let pages = [Some(page(0, &left)), Some(page(1, &right))];
    let mut cache = ScaledCache::new(0, 0);
    let mut buffer = vec![0; 8 * 4];
    view::render_pages_cached(
        &mut buffer,
        8,
        4,
        &pages,
        ScaleFilter::Triangle,
        0,
        &mut cache,
    );
    assert_eq!(cache.len(), 2);
    assert_eq!(buffer[0], 0x00_00ff);
    assert_eq!(buffer[7], 0xff_0000);

    let mut larger = vec![0; 12 * 6];
    view::render_pages_cached(
        &mut larger,
        12,
        6,
        &pages,
        ScaleFilter::Triangle,
        0,
        &mut cache,
    );
    assert_eq!(cache.len(), 4);
    assert_eq!(larger[0], 0x00_00ff);
    assert_eq!(larger[11], 0xff_0000);
}

#[test]
fn spread_layout_uses_original_dimensions_not_preview_dimensions() {
    // Both previews are fitted to the window, so their dimensions no longer
    // share a common scale. The spread must still lay them out by 1:1 and 1:2
    // original aspect ratios.
    let left = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 255]));
    let right = RgbaImage::from_pixel(1, 2, Rgba([0, 0, 255, 255]));
    let left_preview = RgbaImage::from_pixel(8, 8, Rgba([255, 0, 0, 255]));
    let right_preview = RgbaImage::from_pixel(4, 8, Rgba([0, 0, 255, 255]));
    let mut buffer = vec![0u32; 8 * 8];
    let mut cache = ScaledCache::new(4, usize::MAX);

    view::render_pages_cached(
        &mut buffer,
        8,
        8,
        &[
            Some(page_with_size(0, &left_preview, left.dimensions())),
            Some(page_with_size(1, &right_preview, right.dimensions())),
        ],
        ScaleFilter::Triangle,
        0,
        &mut cache,
    );

    assert_eq!(buffer[0], BACKGROUND);
    assert_eq!(buffer[4], 0x00_00ff);
    assert_eq!(buffer[5 * 8 + 4], 0x00_00ff);
    assert_eq!(buffer[2 * 8], 0xff_0000);
    assert_eq!(buffer[5 * 8 + 3], 0xff_0000);
}

#[test]
fn does_not_cache_an_image_that_already_matches_the_target() {
    let image = RgbaImage::from_pixel(8, 4, Rgba([255, 0, 0, 255]));
    let mut buffer = vec![0u32; 8 * 4];
    let mut cache = ScaledCache::new(4, usize::MAX);

    view::render_pages_cached(
        &mut buffer,
        8,
        4,
        &[Some(page(0, &image))],
        ScaleFilter::Triangle,
        0,
        &mut cache,
    );

    assert!(cache.is_empty());
    assert_eq!(buffer[2 * 8 + 3], 0xff_0000);
}
