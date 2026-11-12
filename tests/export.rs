mod common;

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use mangania::config::Config;
use mangania::export;

fn png() -> Vec<u8> {
    let image = RgbaImage::from_pixel(4, 3, Rgba([10, 20, 30, 255]));
    let mut bytes = Vec::new();
    DynamicImage::ImageRgba8(image)
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

#[test]
fn expands_placeholders() {
    assert_eq!(
        export::expand_name("{book}_{page:03}_{name}", "book", 7, "001"),
        "book_007_001"
    );
    assert_eq!(export::expand_name("{page:000}", "book", 7, "001"), "007");
    assert_eq!(export::expand_name("{page}", "book", 7, "001"), "7");
    assert_eq!(
        export::expand_name("{unknown}", "book", 7, "001"),
        "{unknown}"
    );
}

#[test]
fn sanitizes_unsafe_names() {
    let name = export::sanitize_filename("a/b:c*?.jpg");
    assert!(!name.contains(['/', ':', '*', '?']));
    assert_eq!(export::sanitize_filename("CON"), "_CON");
    assert_eq!(export::sanitize_filename("..."), "page");
}

#[test]
fn saves_original_entry_bytes() {
    let image = png();
    let archive_path = common::temp_zip(&[("001.png", image.as_slice())]);
    let out_dir = archive_path.parent().unwrap().join("out");
    let config = Config {
        export_dir: Some(out_dir.clone()),
        export_name: "{book}_{page:03}_{name}".to_string(),
        ..Config::default()
    };

    let output = export::save_current_page(&archive_path, 0, &config).unwrap();

    assert_eq!(output, out_dir.join("book_001_001.png"));
    assert_eq!(std::fs::read(&output).unwrap(), image);
    common::cleanup(&archive_path);
}
