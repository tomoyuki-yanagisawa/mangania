use std::path::Path;

use image::RgbaImage;

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
