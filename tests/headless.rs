mod common;

use mangania::headless;

#[test]
fn checks_a_valid_archive() {
    let image = common::png();
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
    let image = common::png();
    let path = common::temp_zip(&[("1.png", image.as_slice())]);
    let output = path.parent().unwrap().join("out.png");

    headless::render_page(&path, 1, &output).unwrap();

    assert!(output.exists());
    common::cleanup(&path);
}
