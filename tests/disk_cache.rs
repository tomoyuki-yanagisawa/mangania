mod common;

use std::path::Path;

use mangania::archive::Archive;
use mangania::storage::disk_cache::DiskCache;

fn cache_for(archive_path: &Path) -> DiskCache {
    DiskCache::new(archive_path.parent().unwrap().join("cache"))
}

fn source(
    cache: &DiskCache,
    archive: &mut Archive,
    page: usize,
) -> Result<std::path::PathBuf, String> {
    let page = archive.pages[page].clone();
    cache.source(archive, page.entry, &page.name)
}

fn decode(
    cache: &DiskCache,
    archive: &mut Archive,
    page: usize,
) -> Result<image::RgbaImage, String> {
    let page = archive.pages[page].clone();
    cache.decode(archive, page.entry, &page.name)
}

fn cache_files(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    files
}

#[test]
fn extracts_on_miss_and_reuses_on_hit() {
    let image = common::png();
    let path = common::temp_zip(&[("1.png", image.as_slice())]);
    let cache = cache_for(&path);
    let mut archive = Archive::open(&path).unwrap();

    let first = source(&cache, &mut archive, 0).unwrap();
    assert!(first.starts_with(cache.directory()));
    assert_eq!(std::fs::read(&first).unwrap(), image);
    assert_eq!(
        decode(&cache, &mut archive, 0).unwrap().dimensions(),
        (4, 3)
    );

    let second = source(&cache, &mut archive, 0).unwrap();
    assert_eq!(first, second);
    let files = cache_files(cache.directory());
    assert_eq!(files.len(), 1);
    assert!(!files[0].to_string_lossy().contains(".tmp"));

    common::cleanup(&path);
}

#[test]
fn archive_change_selects_a_different_cache_file() {
    let path = common::temp_zip(&[("1.png", common::png().as_slice())]);
    let cache = cache_for(&path);
    let mut archive = Archive::open(&path).unwrap();
    let first = source(&cache, &mut archive, 0).unwrap();
    drop(archive);

    let replacement = common::temp_zip(&[("1.png", b"different image bytes".as_slice())]);
    std::fs::copy(&replacement, &path).unwrap();
    common::cleanup(&replacement);

    let mut archive = Archive::open(&path).unwrap();
    let second = source(&cache, &mut archive, 0).unwrap();
    assert_ne!(first, second);
    assert_eq!(std::fs::read(second).unwrap(), b"different image bytes");

    common::cleanup(&path);
}

#[test]
fn clear_removes_all_cached_files() {
    let image = common::png();
    let path = common::temp_zip(&[("1.png", image.as_slice())]);
    let cache = cache_for(&path);
    let mut archive = Archive::open(&path).unwrap();

    let cached = source(&cache, &mut archive, 0).unwrap();
    assert!(cached.exists());
    cache.clear().unwrap();
    assert!(!cache.directory().exists());

    let cached = source(&cache, &mut archive, 0).unwrap();
    assert_eq!(std::fs::read(cached).unwrap(), image);

    common::cleanup(&path);
}

#[test]
fn cache_directory_failure_is_an_error() {
    let path = common::temp_zip(&[("1.png", common::png().as_slice())]);
    let blocker = path.parent().unwrap().join("cache-blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let cache = DiskCache::new(blocker.join("cache"));
    let mut archive = Archive::open(&path).unwrap();

    let error = source(&cache, &mut archive, 0).unwrap_err();
    assert!(error.contains("disk cache"), "unexpected error: {error}");

    common::cleanup(&path);
}

#[test]
fn corrupt_cached_file_is_removed_and_rebuilt_on_next_read() {
    let image = common::png();
    let path = common::temp_zip(&[("1.png", image.as_slice())]);
    let cache = cache_for(&path);
    let mut archive = Archive::open(&path).unwrap();

    let cached = source(&cache, &mut archive, 0).unwrap();
    let mut bytes = std::fs::read(&cached).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(&cached, bytes).unwrap();

    let error = decode(&cache, &mut archive, 0).unwrap_err();
    assert!(error.contains("unreadable"), "unexpected error: {error}");
    assert!(!cached.exists());

    let decoded = decode(&cache, &mut archive, 0).unwrap();
    assert_eq!(decoded.dimensions(), (4, 3));
    assert_eq!(std::fs::read(&cached).unwrap(), image);

    common::cleanup(&path);
}

#[test]
fn truncated_cached_file_is_replaced() {
    let image = common::png();
    let path = common::temp_zip(&[("1.png", image.as_slice())]);
    let cache = cache_for(&path);
    let mut archive = Archive::open(&path).unwrap();

    let cached = source(&cache, &mut archive, 0).unwrap();
    std::fs::write(&cached, b"x").unwrap();

    let source = source(&cache, &mut archive, 0).unwrap();
    assert_eq!(source, cached);
    assert_eq!(std::fs::read(&source).unwrap(), image);

    common::cleanup(&path);
}

#[test]
fn undecodable_entry_reports_an_error() {
    let path = common::temp_zip(&[("1.png", b"not an image".as_slice())]);
    let cache = cache_for(&path);
    let mut archive = Archive::open(&path).unwrap();

    let error = decode(&cache, &mut archive, 0).unwrap_err();
    assert!(error.contains("unreadable"), "unexpected error: {error}");

    common::cleanup(&path);
}
