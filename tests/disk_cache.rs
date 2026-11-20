mod common;

use std::path::{Path, PathBuf};

use mangania::archive::Archive;
use mangania::render::decode::decode_page;
use mangania::storage::disk_cache::{DiskCache, PageFile};

fn page_file(cache: &DiskCache, archive: &mut Archive, page: usize) -> PageFile {
    let page = archive.pages[page].clone();
    cache.page_path(archive, &page).unwrap()
}

fn source(cache: &DiskCache, archive: &mut Archive, page: usize) -> Result<PathBuf, String> {
    let page = archive.pages[page].clone();
    cache.source(archive, page.entry, &page.name)
}

fn decode(
    cache: &DiskCache,
    archive: &mut Archive,
    page: usize,
) -> Result<image::RgbaImage, String> {
    let page = archive.pages[page].clone();
    let file = cache.page_path(archive, &page)?;
    decode_page(cache, &file)
}

fn cache_files(directory: &Path) -> Vec<PathBuf> {
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
    let cache = common::cache_for(&path);
    let mut archive = Archive::open(&path).unwrap();

    let first = page_file(&cache, &mut archive, 0);
    assert!(first.cached);
    assert!(first.path.starts_with(cache.directory()));
    assert_eq!(std::fs::read(&first.path).unwrap(), image);
    assert_eq!(
        decode(&cache, &mut archive, 0).unwrap().dimensions(),
        (4, 3)
    );

    let second = page_file(&cache, &mut archive, 0);
    assert!(second.cached);
    assert_eq!(first.path, second.path);
    let files = cache_files(cache.directory());
    assert_eq!(files.len(), 1);
    assert!(!files[0].to_string_lossy().contains(".tmp"));

    common::cleanup(&path);
}

#[test]
fn archive_change_selects_a_different_cache_file() {
    let path = common::temp_zip(&[("1.png", common::png().as_slice())]);
    let cache = common::cache_for(&path);
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
    let cache = common::cache_for(&path);
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
    let cache = common::cache_for(&path);
    let mut archive = Archive::open(&path).unwrap();

    let cached = page_file(&cache, &mut archive, 0);
    assert!(cached.cached);
    let mut bytes = std::fs::read(&cached.path).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(&cached.path, bytes).unwrap();

    let error = decode_page(&cache, &cached).unwrap_err();
    assert!(error.contains("unreadable"), "unexpected error: {error}");
    assert!(!cached.path.exists());

    let decoded = decode(&cache, &mut archive, 0).unwrap();
    assert_eq!(decoded.dimensions(), (4, 3));
    assert_eq!(std::fs::read(&cached.path).unwrap(), image);

    common::cleanup(&path);
}

#[test]
fn truncated_cached_file_is_replaced() {
    let image = common::png();
    let path = common::temp_zip(&[("1.png", image.as_slice())]);
    let cache = common::cache_for(&path);
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
    let cache = common::cache_for(&path);
    let mut archive = Archive::open(&path).unwrap();

    let error = decode(&cache, &mut archive, 0).unwrap_err();
    assert!(error.contains("unreadable"), "unexpected error: {error}");

    common::cleanup(&path);
}

#[test]
fn directory_pages_are_read_in_place_and_kept_when_undecodable() {
    let directory = common::temp_dir();
    let image = directory.join("1.png");
    std::fs::write(&image, b"not an image").unwrap();
    let cache = DiskCache::new(directory.join("cache"));
    let mut archive = Archive::open_directory(&directory).unwrap();

    let file = page_file(&cache, &mut archive, 0);
    assert!(!file.cached);
    assert_eq!(file.path, image);
    assert!(!cache.directory().exists());

    let error = decode_page(&cache, &file).unwrap_err();
    assert!(!error.contains("unreadable"), "unexpected error: {error}");
    assert!(image.exists());

    std::fs::remove_dir_all(&directory).unwrap();
}
