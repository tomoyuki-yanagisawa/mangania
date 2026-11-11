mod common;

use mangania::archive::Archive;

#[test]
fn indexes_images_in_natural_order_and_skips_unsafe_entries() {
    let path = common::temp_zip(&[
        ("10.jpg", b"ten".as_slice()),
        ("2.jpg", b"two".as_slice()),
        ("dir/1.png", b"one".as_slice()),
        ("../evil.jpg", b"bad".as_slice()),
        ("dir/.hidden.jpg", b"hidden".as_slice()),
        ("__MACOSX/1.jpg", b"mac".as_slice()),
        ("notes.txt", b"txt".as_slice()),
    ]);

    let mut archive = Archive::open(&path).unwrap();
    let names: Vec<&str> = archive.names.iter().map(String::as_str).collect();
    assert_eq!(names, ["2.jpg", "10.jpg", "dir/1.png"]);
    assert_eq!(archive.pages.len(), 3);

    let first = archive.pages[0];
    assert_eq!(archive.read_index(first).unwrap(), b"two");

    common::cleanup(&path);
}
