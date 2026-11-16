mod common;

use std::time::{Duration, Instant};

use mangania::archive::Archive;
use mangania::disk_cache::DiskCache;
use mangania::prefetch::Prefetch;

#[test]
fn prefetches_without_a_winit_event_loop() {
    let image = common::png();
    let path = common::temp_zip(&[
        ("10.png", b"not an image"),
        ("notes.txt", b"ignored"),
        ("2.png", image.as_slice()),
    ]);
    let cache_dir = path.parent().unwrap().join("cache");
    let archive = Archive::open(&path).unwrap();
    assert_eq!(archive.pages[0].name, "2.png");
    let entry = archive.pages[0].entry;
    let mut prefetch = Prefetch::start(archive, DiskCache::new(cache_dir.clone()), || true);

    prefetch.request(0, entry, true);

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
