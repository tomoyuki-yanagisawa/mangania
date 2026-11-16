mod common;

use std::path::Path;

use mangania::open::{self, Entry, Format};

fn open_error(path: &Path) -> String {
    match Entry::open(path) {
        Ok(_) => panic!("expected open to fail for {}", path.display()),
        Err(error) => error,
    }
}

#[test]
fn detects_and_opens_a_zip() {
    let path = common::temp_zip(&[("1.jpg", b"one".as_slice()), ("2.png", b"two".as_slice())]);

    assert_eq!(open::detect(&path), Format::Zip);

    let mut entry = Entry::open(&path).unwrap();
    assert_eq!(entry.path, path);
    assert_eq!(entry.page_count(), 2);
    assert_eq!(entry.page_name(0), Some("1.jpg"));
    assert_eq!(entry.read_page(1).unwrap(), b"two");

    common::cleanup(&path);
}

#[test]
fn opens_a_zip_with_leading_data() {
    let zip = common::temp_zip(&[("1.jpg", b"one".as_slice())]);
    let mut bytes = b"MZ\x90\x00self-extracting stub\n".to_vec();
    bytes.extend(std::fs::read(&zip).unwrap());
    let path = zip.parent().unwrap().join("book.cbz");
    std::fs::write(&path, &bytes).unwrap();

    assert_eq!(open::detect(&path), Format::Unknown);

    let mut entry = Entry::open(&path).unwrap();
    assert_eq!(entry.page_name(0), Some("1.jpg"));
    assert_eq!(entry.read_page(0).unwrap(), b"one");

    common::cleanup(&zip);
}

#[test]
fn reports_shortcuts_instead_of_zip_errors() {
    let path = common::temp_file("book.lnk", b"not a real shortcut");

    assert_eq!(open::detect(&path), Format::ShellLink);

    let error = open_error(&path);
    assert!(
        error.to_ascii_lowercase().contains("shortcut"),
        "unexpected error: {error}"
    );

    common::cleanup(&path);
}

#[cfg(windows)]
#[test]
fn resolves_windows_shortcut_to_zip() {
    let zip = common::temp_zip(&[("1.jpg", b"one".as_slice())]);
    let shortcut = zip.with_extension("lnk");
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); \
         $s.TargetPath='{}'; $s.Save()",
        shortcut.display(),
        zip.display()
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .unwrap();
    assert!(status.success());

    let mut entry = Entry::open(&shortcut).unwrap();
    assert_eq!(
        std::fs::canonicalize(&entry.path).unwrap(),
        std::fs::canonicalize(&zip).unwrap()
    );
    assert_eq!(entry.page_name(0), Some("1.jpg"));
    assert_eq!(entry.read_page(0).unwrap(), b"one");

    common::cleanup(&zip);
}

#[test]
fn reports_rar_as_unsupported() {
    let path = common::temp_file("book.cbr", b"Rar!\x1A\x07\x00rest of the archive");

    assert_eq!(open::detect(&path), Format::Rar);

    let error = open_error(&path);
    assert!(error.contains("RAR/CBR"), "unexpected error: {error}");
    assert!(
        error.to_ascii_lowercase().contains("not supported"),
        "unexpected error: {error}"
    );

    common::cleanup(&path);
}

#[test]
fn reports_7z_as_unsupported() {
    let path = common::temp_file("book.7z", b"7z\xBC\xAF\x27\x1Crest of the archive");

    assert_eq!(open::detect(&path), Format::SevenZip);

    let error = open_error(&path);
    assert!(error.contains("7z"), "unexpected error: {error}");
    assert!(
        error.to_ascii_lowercase().contains("not supported"),
        "unexpected error: {error}"
    );

    common::cleanup(&path);
}

#[test]
fn reports_html_instead_of_eocd_errors() {
    let path = common::temp_file(
        "book.cbz",
        b"<!DOCTYPE html>\n<html><body>not found</body></html>",
    );

    assert_eq!(open::detect(&path), Format::Html);

    let error = open_error(&path);
    assert!(error.contains("HTML"), "unexpected error: {error}");
    assert!(!error.contains("EOCD"), "unexpected error: {error}");

    common::cleanup(&path);
}

#[test]
fn reports_empty_files() {
    let path = common::temp_file("empty.cbz", b"");

    assert_eq!(open::detect(&path), Format::Empty);

    let error = open_error(&path);
    assert!(
        error.to_ascii_lowercase().contains("empty"),
        "unexpected error: {error}"
    );

    common::cleanup(&path);
}

#[test]
fn reports_truncated_zip_files() {
    let path = common::temp_file("cut.cbz", b"PK\x03\x04");

    assert_eq!(open::detect(&path), Format::Truncated);

    let error = open_error(&path);
    assert!(
        error.to_ascii_lowercase().contains("truncated"),
        "unexpected error: {error}"
    );

    common::cleanup(&path);
}

#[test]
fn reports_unknown_files() {
    let path = common::temp_file("book.cbz", b"this is not an archive");

    assert_eq!(open::detect(&path), Format::Unknown);

    let error = open_error(&path);
    assert!(
        error.to_ascii_lowercase().contains("unsupported")
            || error.to_ascii_lowercase().contains("unrecognized"),
        "unexpected error: {error}"
    );

    common::cleanup(&path);
}
