use std::sync::Arc;

use image::RgbaImage;
use mangania::render::cache::PageCache;

fn image(width: u32, height: u32) -> Arc<RgbaImage> {
    Arc::new(RgbaImage::new(width, height))
}

fn insert(cache: &mut PageCache, page: usize, size: u32, keep: &[usize], current: usize) -> bool {
    cache.insert(page, image(size, size), keep, current)
}

#[test]
fn evicts_farthest_page_from_the_current_page_first() {
    let mut cache = PageCache::new(3, usize::MAX);
    assert!(insert(&mut cache, 5, 4, &[], 10));
    assert!(insert(&mut cache, 9, 4, &[], 10));
    assert!(insert(&mut cache, 10, 4, &[], 10));
    assert!(insert(&mut cache, 11, 4, &[], 10));

    assert!(!cache.contains(5));
    assert!(cache.contains(9));
    assert!(cache.contains(10));
    assert!(cache.contains(11));
}

#[test]
fn equal_distance_tie_evicts_the_past_page_before_the_future_page() {
    let mut cache = PageCache::new(2, usize::MAX);
    assert!(insert(&mut cache, 9, 4, &[], 10));
    assert!(insert(&mut cache, 11, 4, &[], 10));
    assert!(insert(&mut cache, 10, 4, &[], 10));

    assert!(!cache.contains(9));
    assert!(cache.contains(10));
    assert!(cache.contains(11));
}

#[test]
fn navigation_changes_which_page_is_farthest() {
    let mut cache = PageCache::new(2, usize::MAX);
    assert!(insert(&mut cache, 10, 4, &[], 0));
    assert!(insert(&mut cache, 20, 4, &[], 0));
    assert!(insert(&mut cache, 0, 4, &[], 0));

    assert!(!cache.contains(20));
    assert!(cache.contains(0));
    assert!(cache.contains(10));

    assert!(insert(&mut cache, 99, 4, &[], 100));

    assert!(!cache.contains(0));
    assert!(cache.contains(10));
    assert!(cache.contains(99));
}

#[test]
fn replaces_a_page_without_growing_the_entry_count() {
    let mut cache = PageCache::new(4, usize::MAX);
    assert!(cache.insert(1, image(4, 4), &[], 0));
    assert_eq!(cache.used_bytes(), 64);

    assert!(cache.insert(1, image(8, 8), &[], 0));

    assert_eq!(cache.len(), 1);
    assert_eq!(cache.used_bytes(), 256);
    assert_eq!(cache.get(1).unwrap().dimensions(), (8, 8));
}

#[test]
fn keeps_visible_pages_over_the_entry_limit() {
    let mut cache = PageCache::new(1, usize::MAX);
    assert!(insert(&mut cache, 0, 4, &[0, 1], 0));
    assert!(insert(&mut cache, 1, 4, &[0, 1], 0));

    assert!(cache.contains(0));
    assert!(cache.contains(1));
    assert_eq!(cache.len(), 2);
}

#[test]
fn drops_oversized_prefetch_but_keeps_oversized_visible_page() {
    let mut cache = PageCache::new(10, 64);
    assert!(insert(&mut cache, 0, 4, &[], 0));
    assert!(!insert(&mut cache, 1, 8, &[], 0));

    assert!(cache.contains(0));
    assert!(!cache.contains(1));

    assert!(insert(&mut cache, 1, 8, &[1], 1));
    assert!(cache.contains(1));
}

#[test]
fn keeps_both_visible_pages_over_byte_limit() {
    let mut cache = PageCache::new(10, 64);
    assert!(insert(&mut cache, 0, 4, &[0, 1], 0));
    assert!(insert(&mut cache, 1, 4, &[0, 1], 0));

    assert!(cache.contains(0));
    assert!(cache.contains(1));
    assert_eq!(cache.used_bytes(), 128);
}

#[test]
fn clears_all_entries() {
    let mut cache = PageCache::new(10, usize::MAX);
    insert(&mut cache, 0, 4, &[], 0);

    cache.clear();

    assert!(cache.is_empty());
    assert_eq!(cache.used_bytes(), 0);
    assert_eq!(cache.len(), 0);
}
