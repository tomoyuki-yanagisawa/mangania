#![allow(dead_code)]

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use mangania::storage::disk_cache::DiskCache;

static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn temp_zip(entries: &[(&str, &[u8])]) -> PathBuf {
    let path = temp_dir().join("book.zip");
    let file = std::fs::File::create(&path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    for &(name, data) in entries {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap();
    path
}

/// A disk cache next to the temp archive, so parallel test binaries stay isolated.
pub fn cache_for(archive_path: &Path) -> DiskCache {
    DiskCache::new(archive_path.parent().unwrap().join("cache"))
}

pub fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
    let path = temp_dir().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

pub fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mangania-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn cleanup(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}

pub fn encoded(format: ImageFormat) -> Vec<u8> {
    let image = RgbaImage::from_pixel(4, 3, Rgba([10, 20, 30, 255]));
    let mut bytes = Vec::new();
    DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut bytes), format)
        .unwrap();
    bytes
}

pub fn png() -> Vec<u8> {
    encoded(ImageFormat::Png)
}
