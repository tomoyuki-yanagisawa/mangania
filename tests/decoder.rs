mod common;

use image::ImageFormat;
use mangania::decoder;

#[test]
fn decodes_png_and_jpeg() {
    for format in [ImageFormat::Png, ImageFormat::Jpeg] {
        let decoded = decoder::decode(&common::encoded(format)).unwrap();
        assert_eq!(decoded.dimensions(), (4, 3));
    }
}

#[test]
fn rejects_garbage() {
    assert!(decoder::decode(b"not an image").is_err());
}
