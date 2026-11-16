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
    let names: Vec<&str> = archive
        .pages
        .iter()
        .map(|page| page.name.as_str())
        .collect();
    assert_eq!(names, ["2.jpg", "3.bmp", "10.jpg", "dir/1.png"]);
    assert_eq!(archive.pages.len(), 4);

    let entries: Vec<_> = archive.pages.iter().map(|page| page.entry).collect();
    assert_eq!(entries, [1, 3, 0, 2]);
    for (entry, expected) in entries.into_iter().zip([
        b"two".as_slice(),
        b"three".as_slice(),
        b"ten".as_slice(),
        b"one".as_slice(),
    ]) {
        let mut bytes = Vec::new();
        archive.write_index(entry, &mut bytes).unwrap();
        assert_eq!(bytes, expected);
    }

    common::cleanup(&path);
}
