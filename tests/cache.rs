use std::sync::Arc;

use image::RgbaImage;
use mangania::cache::Cache;

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
