mod common;

use std::time::{Duration, Instant};

use mangania::archive::Archive;
use mangania::render::prefetch::Prefetch;
use mangania::storage::disk_cache::DiskCache;

#[test]
fn prefetches_without_a_winit_event_loop() {
    let image = common::png();
    let path = common::temp_zip(&[
        ("10.png", b"not an image"),
        ("notes.txt", b"ignored"),
        ("2.png", image.as_slice()),
    ]);
    let cache_dir = path.parent().unwrap().join("cache");
    let mut archive = Archive::open(&path).unwrap();
    assert_eq!(archive.pages[0].name, "2.png");
    let page = archive.pages[0].clone();
    archive.pages.clear();
    let mut prefetch = Prefetch::start(archive, DiskCache::new(cache_dir.clone()), || true);

    // The GUI moves the page list into the viewer; prefetching must not depend
    // on the archive retaining it.
    prefetch.request(0, page.entry, page.name, true);

    let deadline = Instant::now() + Duration::from_secs(5);
    let (page, result) = loop {
        if let Some(item) = prefetch.poll().into_iter().next() {
            break item;
        }
        assert!(Instant::now() < deadline, "prefetch timed out");
        std::thread::sleep(Duration::from_millis(10));
    };

    assert_eq!(page, 0);
    assert_eq!(result.unwrap().dimensions(), (4, 3));
    assert!(std::fs::read_dir(&cache_dir).unwrap().next().is_some());

    drop(prefetch);
    common::cleanup(&path);
}

#[test]
fn urgent_batch_keeps_the_requested_order() {
    let image = common::png();
    let path = common::temp_zip(&[("1.png", image.as_slice()), ("2.png", image.as_slice())]);
    let cache_dir = path.parent().unwrap().join("cache");
    let mut archive = Archive::open(&path).unwrap();
    let pages = std::mem::take(&mut archive.pages);
    let mut prefetch = Prefetch::start(archive, DiskCache::new(cache_dir.clone()), || true);

    let jobs = pages
        .iter()
        .enumerate()
        .map(|(index, page)| (index, page.entry, page.name.clone()))
        .collect();
    prefetch.request_batch(jobs);

    // The worker is single-threaded, so the first requested page must finish
    // first. A second urgent request must not drop the first one.
    let mut order = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while order.len() < 2 {
        for (page, result) in prefetch.poll() {
            result.unwrap();
            order.push(page);
        }
        assert!(Instant::now() < deadline, "prefetch timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(order, vec![0, 1]);

    drop(prefetch);
    common::cleanup(&path);
}
