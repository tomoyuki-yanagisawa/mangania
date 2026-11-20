//! Image decoding for archive entries and plain files.

use std::path::Path;

use image::RgbaImage;

use crate::debug_log;
use crate::storage::disk_cache::{DiskCache, PageFile};

pub fn decode(bytes: &[u8]) -> Result<RgbaImage, String> {
    image::load_from_memory(bytes)
        .map(|image| image.to_rgba8())
        .map_err(|error| error.to_string())
}

/// Decodes an image file without loading its bytes into an intermediate buffer.
pub fn decode_file(path: &Path) -> Result<RgbaImage, String> {
    image::ImageReader::open(path)
        .map_err(|error| format!("open {}: {error}", path.display()))?
        .with_guessed_format()
        .map_err(|error| format!("probe {}: {error}", path.display()))?
        .decode()
        .map(|image| image.to_rgba8())
        .map_err(|error| format!("decode {}: {error}", path.display()))
}

/// Decodes a page file. A broken disk-cache file is removed so the next read
/// re-extracts it; a directory image is left alone.
pub fn decode_page(cache: &DiskCache, file: &PageFile) -> Result<RgbaImage, String> {
    match decode_file(&file.path) {
        Ok(image) => Ok(image),
        Err(error) => {
            // `decode_file` already includes the path in its message.
            let message = if file.cached {
                cache.discard(&file.path);
                format!("cached image is unreadable: {error}")
            } else {
                error
            };
            debug_log::log(format_args!("decode: {message}"));
            Err(message)
        }
    }
}
