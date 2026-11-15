mod common;

use mangania::archive::Archive;

#[test]
fn indexes_images_in_natural_order_and_skips_unsafe_entries() {
    let path = common::temp_zip(&[
        ("10.jpg", b"ten".as_slice()),
        ("2.jpg", b"two".as_slice()),
        ("dir/1.png", b"one".as_slice()),
        ("3.bmp", b"three".as_slice()),
        ("../evil.jpg", b"bad".as_slice()),
        ("dir/.hidden.jpg", b"hidden".as_slice()),
        ("__MACOSX/1.jpg", b"mac".as_slice()),
        ("notes.txt", b"txt".as_slice()),
    ]);

    let mut archive = Archive::open(&path).unwrap();
    let names: Vec<&str> = archive.names.iter().map(String::as_str).collect();
    assert_eq!(names, ["2.jpg", "3.bmp", "10.jpg", "dir/1.png"]);
    assert_eq!(archive.pages.len(), 4);

    let first = archive.pages[0];
    assert_eq!(archive.read_index(first).unwrap(), b"two");

    common::cleanup(&path);
}

#[test]
fn reports_shortcuts_instead_of_zip_errors() {
    let path = common::temp_zip(&[("1.jpg", b"one".as_slice())]);
    let shortcut = path.with_extension("lnk");
    std::fs::rename(&path, &shortcut).unwrap();

    let error = match Archive::open(&shortcut) {
        Ok(_) => panic!("expected shortcut error"),
        Err(error) => error,
    };
    assert!(
        error.to_ascii_lowercase().contains("shortcut"),
        "unexpected error: {error}"
    );

    common::cleanup(&shortcut);
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

    let mut archive = Archive::open(&shortcut).unwrap();
    assert_eq!(archive.names, ["1.jpg"]);
    let page = archive.pages[0];
    assert_eq!(archive.read_index(page).unwrap(), b"one");

    common::cleanup(&zip);
}
