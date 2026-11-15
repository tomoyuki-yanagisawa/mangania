use std::sync::Arc;

use image::RgbaImage;
use mangania::cache::{Cache, ScaledCache, ScaledKey};
use mangania::config::ScaleFilter;

fn image(width: u32, height: u32) -> Arc<RgbaImage> {
    Arc::new(RgbaImage::new(width, height))
}

#[test]
fn evicts_by_page_limit_but_keeps_current() {
    let mut cache = Cache::new(2, usize::MAX);
    cache.insert(0, image(2, 2), &[2]);
    cache.insert(1, image(2, 2), &[2]);
    cache.insert(2, image(2, 2), &[2]);

    assert!(!cache.contains(0));
    assert!(cache.contains(1));
    assert!(cache.contains(2));
}

#[test]
fn evicts_by_byte_limit() {
    let mut cache = Cache::new(10, 32);
    cache.insert(0, image(2, 2), &[0]);
    cache.insert(1, image(2, 2), &[0]);
    cache.insert(2, image(2, 2), &[2]);

    assert!(!cache.contains(0));
    assert!(cache.contains(1));
    assert!(cache.contains(2));
}

#[test]
fn drops_oversized_prefetch_but_keeps_oversized_current() {
    let mut cache = Cache::new(10, 16);
    cache.insert(0, image(2, 2), &[0]);
    cache.insert(1, image(4, 4), &[0]);
    assert!(!cache.contains(1));

    cache.insert(1, image(4, 4), &[1]);
    assert!(cache.contains(1));
    assert!(!cache.contains(0));
}

#[test]
fn keeps_both_visible_pages_over_byte_limit() {
    let mut cache = Cache::new(4, 32);
    cache.insert(0, image(4, 4), &[0, 1]);
    cache.insert(1, image(4, 4), &[0, 1]);

    assert!(cache.contains(0));
    assert!(cache.contains(1));
}

#[test]
fn drops_prefetch_when_visible_pages_use_the_budget() {
    let mut cache = Cache::new(10, 32);
    assert!(cache.insert(0, image(4, 4), &[0, 1]));
    assert!(cache.insert(1, image(4, 4), &[0, 1]));
    assert!(!cache.insert(2, image(4, 4), &[0, 1]));

    assert!(cache.contains(0));
    assert!(cache.contains(1));
    assert!(!cache.contains(2));
}

#[test]
fn reports_pages_evicted_from_the_decoded_cache() {
    let mut cache = Cache::new(2, usize::MAX);
    cache.insert(0, image(2, 2), &[2]);
    cache.insert(1, image(2, 2), &[2]);
    cache.insert(2, image(2, 2), &[2]);

    assert_eq!(cache.take_evicted(), vec![0]);
    assert!(cache.take_evicted().is_empty());
}

fn scaled_key(page: usize, filter: ScaleFilter) -> ScaledKey {
    ScaledKey::new(page, 4, 4, filter)
}

#[test]
fn scaled_cache_evicts_oldest_entry_by_count() {
    let mut cache = ScaledCache::new(2, usize::MAX);
    assert!(cache.insert(scaled_key(0, ScaleFilter::Triangle), image(4, 4)));
    assert!(cache.insert(scaled_key(1, ScaleFilter::Triangle), image(4, 4)));
    assert!(cache.insert(scaled_key(2, ScaleFilter::Triangle), image(4, 4)));

    assert!(cache.get(scaled_key(0, ScaleFilter::Triangle)).is_none());
    assert!(cache.get(scaled_key(1, ScaleFilter::Triangle)).is_some());
    assert!(cache.get(scaled_key(2, ScaleFilter::Triangle)).is_some());
    assert_eq!(cache.len(), 2);
}

#[test]
fn scaled_cache_respects_the_byte_budget() {
    let mut cache = ScaledCache::new(10, 128);
    assert!(cache.insert(scaled_key(0, ScaleFilter::Triangle), image(4, 4)));
    assert!(cache.insert(scaled_key(1, ScaleFilter::Triangle), image(4, 4)));
    assert_eq!(cache.used_bytes(), 128);
    assert!(!cache.insert(scaled_key(2, ScaleFilter::Triangle), image(8, 8)));
    assert_eq!(cache.len(), 2);
}

#[test]
fn scaled_cache_distinguishes_filter_and_size() {
    let mut cache = ScaledCache::new(10, usize::MAX);
    assert!(cache.insert(scaled_key(0, ScaleFilter::Triangle), image(4, 4)));
    assert!(cache.insert(scaled_key(0, ScaleFilter::Lanczos3), image(4, 4)));
    assert!(cache.insert(ScaledKey::new(0, 8, 8, ScaleFilter::Triangle), image(8, 8)));
    assert_eq!(cache.len(), 3);
}

#[test]
fn removes_scaled_entries_for_an_evicted_page() {
    let mut cache = ScaledCache::new(10, usize::MAX);
    cache.insert(scaled_key(0, ScaleFilter::Triangle), image(4, 4));
    cache.insert(scaled_key(0, ScaleFilter::Lanczos3), image(4, 4));
    cache.insert(scaled_key(1, ScaleFilter::Triangle), image(4, 4));

    cache.remove_page(0);

    assert!(cache.get(scaled_key(0, ScaleFilter::Triangle)).is_none());
    assert!(cache.get(scaled_key(0, ScaleFilter::Lanczos3)).is_none());
    assert!(cache.get(scaled_key(1, ScaleFilter::Triangle)).is_some());
    assert_eq!(cache.used_bytes(), 64);
}

#[test]
fn clears_the_scaled_cache() {
    let mut cache = ScaledCache::new(10, usize::MAX);
    cache.insert(scaled_key(0, ScaleFilter::Triangle), image(4, 4));

    cache.clear();

    assert!(cache.is_empty());
    assert_eq!(cache.used_bytes(), 0);
    assert_eq!(cache.len(), 0);
}
