use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use mangania::decoder;

fn encoded(format: ImageFormat) -> Vec<u8> {
    let image = RgbaImage::from_pixel(4, 3, Rgba([10, 20, 30, 255]));
    let mut bytes = Vec::new();
    DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut bytes), format)
        .unwrap();
    bytes
}

#[test]
fn decodes_png_and_jpeg() {
    for format in [ImageFormat::Png, ImageFormat::Jpeg] {
        let decoded = decoder::decode(&encoded(format)).unwrap();
        assert_eq!(decoded.dimensions(), (4, 3));
    }
}

#[test]
fn rejects_garbage() {
    assert!(decoder::decode(b"not an image").is_err());
}
