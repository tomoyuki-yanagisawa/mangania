mod common;

use std::time::{Duration, Instant};

use mangania::archive::Archive;
use mangania::prefetch::Prefetch;

#[test]
fn prefetches_without_a_winit_event_loop() {
    let image = common::png();
    let path = common::temp_zip(&[("1.png", image.as_slice())]);
    let archive = Archive::open(&path).unwrap();
    let entry = archive.pages[0];
    let mut prefetch = Prefetch::start(archive, || true);

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

    drop(prefetch);
    common::cleanup(&path);
}
