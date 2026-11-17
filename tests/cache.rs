use std::sync::Arc;

use image::RgbaImage;
use mangania::render::cache::PageCache;

fn image(width: u32, height: u32) -> Arc<RgbaImage> {
    Arc::new(RgbaImage::new(width, height))
}

fn insert(cache: &mut PageCache, page: usize, size: u32, current: usize, step: usize) -> bool {
    cache.insert(page, image(size, size), current, step)
}

#[test]
fn evicts_far_past_before_far_future() {
    let mut cache = PageCache::new(2, usize::MAX);
    assert!(insert(&mut cache, 0, 4, 5, 1));
    assert!(insert(&mut cache, 8, 4, 5, 1));
    assert!(insert(&mut cache, 9, 4, 5, 1));

    assert!(!cache.contains(0));
    assert!(cache.contains(8));
    assert!(cache.contains(9));
}

#[test]
fn keeps_one_step_neighbours_over_the_limit() {
    let mut cache = PageCache::new(1, usize::MAX);
    assert!(insert(&mut cache, 4, 4, 5, 1));
    assert!(insert(&mut cache, 5, 4, 5, 1));
    assert!(insert(&mut cache, 6, 4, 5, 1));

    assert!(cache.contains(4));
    assert!(cache.contains(5));
    assert!(cache.contains(6));
    assert_eq!(cache.len(), 3);
}

#[test]
fn evicts_far_future_before_one_step_back() {
    let mut cache = PageCache::new(2, usize::MAX);
    assert!(insert(&mut cache, 4, 4, 5, 1));
    assert!(insert(&mut cache, 6, 4, 5, 1));
    assert!(!insert(&mut cache, 9, 4, 5, 1));

    assert!(cache.contains(4));
    assert!(cache.contains(6));
    assert!(!cache.contains(9));
}

#[test]
fn evicts_the_farthest_future_first() {
    let mut cache = PageCache::new(2, usize::MAX);
    assert!(insert(&mut cache, 6, 4, 5, 1));
    assert!(insert(&mut cache, 7, 4, 5, 1));
    assert!(!insert(&mut cache, 8, 4, 5, 1));

    assert!(cache.contains(6));
    assert!(cache.contains(7));
    assert!(!cache.contains(8));
}

#[test]
fn spread_protects_one_display_in_each_direction() {
    let mut cache = PageCache::new(1, usize::MAX);
    for page in 8..=13 {
        assert!(insert(&mut cache, page, 4, 10, 2));
    }

    assert_eq!(cache.len(), 6);
}

#[test]
fn replaces_a_page_without_growing_the_entry_count() {
    let mut cache = PageCache::new(4, usize::MAX);
    assert!(cache.insert(1, image(4, 4), 0, 1));
    assert_eq!(cache.used_bytes(), 64);

    assert!(cache.insert(1, image(8, 8), 0, 1));

    assert_eq!(cache.len(), 1);
    assert_eq!(cache.used_bytes(), 256);
    assert_eq!(cache.get(1).unwrap().dimensions(), (8, 8));
}

#[test]
fn keeps_visible_pages_over_the_entry_limit() {
    let mut cache = PageCache::new(1, usize::MAX);
    assert!(insert(&mut cache, 0, 4, 0, 1));
    assert!(insert(&mut cache, 1, 4, 0, 1));

    assert!(cache.contains(0));
    assert!(cache.contains(1));
    assert_eq!(cache.len(), 2);
}

#[test]
fn drops_oversized_prefetch_but_keeps_oversized_visible_page() {
    let mut cache = PageCache::new(10, 64);
    assert!(insert(&mut cache, 0, 4, 0, 1));
    assert!(!insert(&mut cache, 10, 8, 0, 1));

    assert!(cache.contains(0));
    assert!(!cache.contains(10));

    assert!(insert(&mut cache, 1, 8, 0, 1));
    assert!(cache.contains(1));
}

#[test]
fn keeps_both_visible_pages_over_byte_limit() {
    let mut cache = PageCache::new(10, 64);
    assert!(insert(&mut cache, 0, 4, 0, 1));
    assert!(insert(&mut cache, 1, 4, 0, 1));

    assert!(cache.contains(0));
    assert!(cache.contains(1));
    assert_eq!(cache.used_bytes(), 128);
}

#[test]
fn clears_all_entries() {
    let mut cache = PageCache::new(10, usize::MAX);
    insert(&mut cache, 0, 4, 0, 1);

    cache.clear();

    assert!(cache.is_empty());
    assert_eq!(cache.used_bytes(), 0);
    assert_eq!(cache.len(), 0);
}
