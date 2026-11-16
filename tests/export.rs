mod common;

use std::path::Path;

use image::ImageFormat;
use mangania::config::Config;
use mangania::export;
use mangania::storage::disk_cache::DiskCache;

fn cache_for(archive_path: &Path) -> DiskCache {
    DiskCache::new(archive_path.parent().unwrap().join("cache"))
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
    let image = common::png();
    let archive_path = common::temp_zip(&[("001.png", image.as_slice())]);
    let out_dir = archive_path.parent().unwrap().join("out");
    let config = Config {
        export_dir: Some(out_dir.clone()),
        export_name: "{book}_{page:03}_{name}".to_string(),
        ..Config::default()
    };

    let cache = cache_for(&archive_path);
    let output = export::save_current_page(&archive_path, 0, &config, &cache).unwrap();

    assert_eq!(output, out_dir.join("book_001_001.png"));
    assert_eq!(std::fs::read(&output).unwrap(), image);
    common::cleanup(&archive_path);
}

#[test]
fn saves_original_bmp_bytes_and_extension() {
    let image = common::encoded(ImageFormat::Bmp);
    let archive_path = common::temp_zip(&[("001.bmp", image.as_slice())]);
    let out_dir = archive_path.parent().unwrap().join("out");
    let config = Config {
        export_dir: Some(out_dir.clone()),
        export_name: "{book}_{page:03}_{name}".to_string(),
        ..Config::default()
    };

    let cache = cache_for(&archive_path);
    let output = export::save_current_page(&archive_path, 0, &config, &cache).unwrap();

    assert_eq!(output, out_dir.join("book_001_001.bmp"));
    assert_eq!(std::fs::read(&output).unwrap(), image);
    common::cleanup(&archive_path);
}

#[test]
fn saves_sorted_page_with_matching_name_and_bytes() {
    let archive_path = common::temp_zip(&[
        ("10.jpg", b"ten"),
        ("notes.txt", b"ignored"),
        ("2.png", b"two"),
    ]);
    let out_dir = archive_path.parent().unwrap().join("out");
    let config = Config {
        export_dir: Some(out_dir.clone()),
        export_name: "{page}_{name}".to_string(),
        ..Config::default()
    };

    let cache = cache_for(&archive_path);
    let output = export::save_current_page(&archive_path, 0, &config, &cache).unwrap();
    assert_eq!(output, out_dir.join("1_2.png"));
    assert_eq!(std::fs::read(output).unwrap(), b"two");
    common::cleanup(&archive_path);
}
