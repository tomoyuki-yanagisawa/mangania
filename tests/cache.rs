use std::sync::Arc;

use image::RgbaImage;
use mangania::config::ScaleFilter;
use mangania::render::cache::{ScaledCache, ScaledKey};

fn image(width: u32, height: u32) -> Arc<RgbaImage> {
    Arc::new(RgbaImage::new(width, height))
}

fn scaled_key(page: usize, size: u32, filter: ScaleFilter) -> ScaledKey {
    ScaledKey::new(page, size, size, filter)
}

fn insert(cache: &mut ScaledCache, page: usize, size: u32, keep: &[usize], current: usize) -> bool {
    cache.insert(
        scaled_key(page, size, ScaleFilter::Triangle),
        image(size, size),
        keep,
        current,
    )
}

#[test]
fn evicts_farthest_page_from_the_current_page_first() {
    let mut cache = ScaledCache::new(3, usize::MAX);
    assert!(insert(&mut cache, 5, 4, &[], 10));
    assert!(insert(&mut cache, 9, 4, &[], 10));
    assert!(insert(&mut cache, 10, 4, &[], 10));
    assert!(insert(&mut cache, 11, 4, &[], 10));

    assert!(!cache.contains_page(5, ScaleFilter::Triangle));
    assert!(cache.contains_page(9, ScaleFilter::Triangle));
    assert!(cache.contains_page(10, ScaleFilter::Triangle));
    assert!(cache.contains_page(11, ScaleFilter::Triangle));
}

#[test]
fn equal_distance_tie_evicts_the_past_page_before_the_future_page() {
    let mut cache = ScaledCache::new(2, usize::MAX);
    assert!(insert(&mut cache, 9, 4, &[], 10));
    assert!(insert(&mut cache, 11, 4, &[], 10));
    assert!(insert(&mut cache, 10, 4, &[], 10));

    assert!(!cache.contains_page(9, ScaleFilter::Triangle));
    assert!(cache.contains_page(10, ScaleFilter::Triangle));
    assert!(cache.contains_page(11, ScaleFilter::Triangle));
}

#[test]
fn navigation_changes_which_page_is_farthest() {
    let mut cache = ScaledCache::new(2, usize::MAX);
    assert!(insert(&mut cache, 10, 4, &[], 0));
    assert!(insert(&mut cache, 20, 4, &[], 0));
    assert!(insert(&mut cache, 0, 4, &[], 0));

    assert!(!cache.contains_page(20, ScaleFilter::Triangle));
    assert!(cache.contains_page(0, ScaleFilter::Triangle));
    assert!(cache.contains_page(10, ScaleFilter::Triangle));

    assert!(insert(&mut cache, 99, 4, &[], 100));

    assert!(!cache.contains_page(0, ScaleFilter::Triangle));
    assert!(cache.contains_page(10, ScaleFilter::Triangle));
    assert!(cache.contains_page(99, ScaleFilter::Triangle));
}

#[test]
fn equal_distance_tie_prefers_the_older_entry() {
    let mut cache = ScaledCache::new(2, usize::MAX);
    let small = ScaledKey::new(11, 4, 4, ScaleFilter::Triangle);
    let large = ScaledKey::new(11, 8, 8, ScaleFilter::Triangle);
    assert!(cache.insert(small, image(4, 4), &[], 10));
    assert!(cache.insert(large, image(8, 8), &[], 10));
    assert!(insert(&mut cache, 10, 4, &[], 10));

    assert!(cache.get(small).is_none());
    assert!(cache.get(large).is_some());
    assert!(cache.contains_page(10, ScaleFilter::Triangle));
}

#[test]
fn keeps_visible_pages_over_the_entry_limit() {
    let mut cache = ScaledCache::new(1, usize::MAX);
    assert!(insert(&mut cache, 0, 4, &[0, 1], 0));
    assert!(insert(&mut cache, 1, 4, &[0, 1], 0));

    assert!(cache.contains_page(0, ScaleFilter::Triangle));
    assert!(cache.contains_page(1, ScaleFilter::Triangle));
    assert_eq!(cache.len(), 2);
}

#[test]
fn drops_oversized_prefetch_but_keeps_oversized_visible_page() {
    let mut cache = ScaledCache::new(10, 64);
    assert!(insert(&mut cache, 0, 4, &[], 0));
    assert!(!insert(&mut cache, 1, 8, &[], 0));

    assert!(cache.contains_page(0, ScaleFilter::Triangle));
    assert!(!cache.contains_page(1, ScaleFilter::Triangle));

    assert!(insert(&mut cache, 1, 8, &[1], 1));
    assert!(cache.contains_page(1, ScaleFilter::Triangle));
}

#[test]
fn keeps_both_visible_previews_over_byte_limit() {
    let mut cache = ScaledCache::new(10, 64);
    assert!(insert(&mut cache, 0, 4, &[0, 1], 0));
    assert!(insert(&mut cache, 1, 4, &[0, 1], 0));

    assert!(cache.contains_page(0, ScaleFilter::Triangle));
    assert!(cache.contains_page(1, ScaleFilter::Triangle));
    assert_eq!(cache.used_bytes(), 128);
}

#[test]
fn keeps_window_preview_and_exact_spread_target() {
    let mut cache = ScaledCache::new(10, usize::MAX);
    assert!(cache.insert(
        ScaledKey::new(0, 8, 8, ScaleFilter::Triangle),
        image(8, 8),
        &[0],
        0
    ));
    assert!(cache.insert(
        ScaledKey::new(0, 4, 4, ScaleFilter::Triangle),
        image(4, 4),
        &[0],
        0
    ));

    assert!(cache.contains_page(0, ScaleFilter::Triangle));
    let largest = cache.get_page(0, ScaleFilter::Triangle).unwrap();
    assert_eq!(largest.dimensions(), (8, 8));
    assert_eq!(cache.len(), 2);
}

#[test]
fn separates_pages_and_filters() {
    let mut cache = ScaledCache::new(10, usize::MAX);
    assert!(cache.insert(scaled_key(0, 4, ScaleFilter::Triangle), image(4, 4), &[], 0));
    assert!(cache.insert(scaled_key(0, 4, ScaleFilter::Lanczos3), image(4, 4), &[], 0));

    assert!(cache.contains_page(0, ScaleFilter::Triangle));
    assert!(cache.contains_page(0, ScaleFilter::Lanczos3));
    assert!(cache.get_page(0, ScaleFilter::Gaussian).is_none());
    assert_eq!(cache.len(), 2);
}

#[test]
fn clears_all_resized_entries() {
    let mut cache = ScaledCache::new(10, usize::MAX);
    insert(&mut cache, 0, 4, &[], 0);

    cache.clear();

    assert!(cache.is_empty());
    assert_eq!(cache.used_bytes(), 0);
    assert_eq!(cache.len(), 0);
}
