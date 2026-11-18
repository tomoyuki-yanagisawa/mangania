mod common;

use image::ImageFormat;
use mangania::decode;

#[test]
fn decodes_png_jpeg_and_bmp() {
    for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Bmp] {
        let decoded = decode::decode(&common::encoded(format)).unwrap();
        assert_eq!(decoded.dimensions(), (4, 3));
    }
}

#[test]
fn rejects_garbage() {
    assert!(decode::decode(b"not an image").is_err());
}
