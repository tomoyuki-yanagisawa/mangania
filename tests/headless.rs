mod common;

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use mangania::headless;

fn png() -> Vec<u8> {
    let image = RgbaImage::from_pixel(4, 3, Rgba([10, 20, 30, 255]));
    let mut bytes = Vec::new();
    DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

#[test]
fn checks_a_valid_archive() {
    let image = png();
    let path = common::temp_zip(&[("1.png", image.as_slice())]);

    headless::check(&path).unwrap();

    common::cleanup(&path);
}

#[test]
fn check_fails_on_a_bad_image() {
    let path = common::temp_zip(&[("1.png", b"not an image".as_slice())]);

    assert!(headless::check(&path).is_err());

    common::cleanup(&path);
}

#[test]
fn renders_a_page_to_a_file() {
    let image = png();
    let path = common::temp_zip(&[("1.png", image.as_slice())]);
    let output = path.parent().unwrap().join("out.png");

    headless::render_page(&path, 1, &output).unwrap();

    assert!(output.exists());
    common::cleanup(&path);
}
