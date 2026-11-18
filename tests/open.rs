mod common;

use std::path::Path;

use mangania::archive::open::{self, Entry, Format};

fn open_error(path: &Path) -> String {
    match Entry::open(path) {
        Ok(_) => panic!("expected open to fail for {}", path.display()),
        Err(error) => error,
    }
}

fn page_bytes(entry: &mut Entry, page: usize) -> Vec<u8> {
    let index = entry.archive.pages[page].entry;
    let mut bytes = Vec::new();
    entry.archive.write_index(index, &mut bytes).unwrap();
    bytes
}

#[test]
fn detects_and_opens_a_zip() {
    let path = common::temp_zip(&[("1.jpg", b"one".as_slice()), ("2.png", b"two".as_slice())]);

    assert_eq!(open::detect(&path), Format::Zip);

    let mut entry = Entry::open(&path).unwrap();
    assert_eq!(entry.path, path);
    assert_eq!(entry.page_count(), 2);
    assert_eq!(entry.page(0).unwrap().name, "1.jpg");
    assert_eq!(page_bytes(&mut entry, 1), b"two");

    common::cleanup(&path);
}

#[test]
fn opens_a_zip_with_leading_data() {
    let zip = common::temp_zip(&[("1.jpg", b"one".as_slice())]);
    let mut bytes = b"MZ\x90\x00self-extracting stub\n".to_vec();
    bytes.extend(std::fs::read(&zip).unwrap());
    let path = zip.parent().unwrap().join("book.cbz");
    std::fs::write(&path, &bytes).unwrap();

    assert_eq!(open::detect(&path), Format::Broken);

    let mut entry = Entry::open(&path).unwrap();
    assert_eq!(entry.page(0).unwrap().name, "1.jpg");
    assert_eq!(page_bytes(&mut entry, 0), b"one");

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
    assert_eq!(entry.page(0).unwrap().name, "1.jpg");
    assert_eq!(page_bytes(&mut entry, 0), b"one");

    common::cleanup(&zip);
}

#[test]
fn reports_rar_as_unsupported() {
    let path = common::temp_file("book.cbr", b"Rar!\x1A\x07\x00rest of the archive");

    assert_eq!(open::detect(&path), Format::Unsupported);

    let error = open_error(&path);
    assert!(
        error.to_ascii_lowercase().contains("unsupported"),
        "unexpected error: {error}"
    );

    common::cleanup(&path);
}

#[test]
fn reports_7z_as_unsupported() {
    let path = common::temp_file("book.7z", b"7z\xBC\xAF\x27\x1Crest of the archive");

    assert_eq!(open::detect(&path), Format::Unsupported);

    let error = open_error(&path);
    assert!(
        error.to_ascii_lowercase().contains("unsupported"),
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

    assert_eq!(open::detect(&path), Format::Unsupported);

    let error = open_error(&path);
    assert!(
        error.to_ascii_lowercase().contains("unsupported"),
        "unexpected error: {error}"
    );
    assert!(!error.contains("EOCD"), "unexpected error: {error}");

    common::cleanup(&path);
}

#[test]
fn reports_empty_files() {
    let path = common::temp_file("empty.cbz", b"");

    assert_eq!(open::detect(&path), Format::Broken);

    let error = open_error(&path);
    assert!(
        error.to_ascii_lowercase().contains("broken"),
        "unexpected error: {error}"
    );

    common::cleanup(&path);
}

#[test]
fn reports_truncated_zip_files() {
    let path = common::temp_file("cut.cbz", b"PK\x03\x04");

    assert_eq!(open::detect(&path), Format::Broken);

    let error = open_error(&path);
    assert!(
        error.to_ascii_lowercase().contains("broken"),
        "unexpected error: {error}"
    );

    common::cleanup(&path);
}

#[test]
fn reports_unknown_files() {
    let path = common::temp_file("book.cbz", b"this is not an archive");

    assert_eq!(open::detect(&path), Format::Broken);

    let error = open_error(&path);
    assert!(
        error.to_ascii_lowercase().contains("broken"),
        "unexpected error: {error}"
    );

    common::cleanup(&path);
}

#[test]
fn opens_only_direct_image_files_in_a_directory() {
    let directory = common::temp_dir();
    std::fs::write(directory.join("10.jpg"), b"ten").unwrap();
    std::fs::write(directory.join("2.jpg"), b"two").unwrap();
    std::fs::write(directory.join(".hidden.jpg"), b"hidden").unwrap();
    std::fs::write(directory.join("notes.txt"), b"notes").unwrap();
    std::fs::create_dir(directory.join("sub")).unwrap();
    std::fs::write(directory.join("sub").join("1.png"), b"sub").unwrap();

    assert_eq!(open::detect(&directory), Format::Directory);

    let entry = Entry::open(&directory).unwrap();
    assert_eq!(entry.path, directory);
    let names: Vec<&str> = entry
        .pages()
        .iter()
        .map(|page| page.name.as_str())
        .collect();
    assert_eq!(names, ["2.jpg", "10.jpg"]);

    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn reports_a_directory_without_images() {
    let directory = common::temp_dir();
    std::fs::write(directory.join("notes.txt"), b"notes").unwrap();

    assert_eq!(open::detect(&directory), Format::Directory);

    let error = open_error(&directory);
    assert!(
        error.contains("no supported images"),
        "unexpected error: {error}"
    );

    std::fs::remove_dir_all(&directory).unwrap();
}

#[cfg(unix)]
#[test]
fn ignores_symlinks_in_a_directory() {
    use std::os::unix::fs::symlink;

    let directory = common::temp_dir();
    let image = directory.join("1.jpg");
    std::fs::write(&image, b"one").unwrap();
    symlink(&image, directory.join("2.jpg")).unwrap();

    let entry = Entry::open(&directory).unwrap();
    let names: Vec<&str> = entry
        .pages()
        .iter()
        .map(|page| page.name.as_str())
        .collect();
    assert_eq!(names, ["1.jpg"]);

    std::fs::remove_dir_all(&directory).unwrap();
}
