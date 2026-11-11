use image::{Rgba, RgbaImage};
use mangania::view;

const BACKGROUND: u32 = 0x20_20_20;

fn red() -> RgbaImage {
    RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]))
}

fn blue() -> RgbaImage {
    RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255]))
}

#[test]
fn renders_a_single_page_centered() {
    let mut buffer = vec![0u32; 8 * 4];
    view::render(&mut buffer, 8, 4, &red());

    assert_eq!(buffer[0], BACKGROUND);
    assert_eq!(buffer[2 * 8 + 3], 0xff_0000);
    assert_eq!(buffer[3 * 8 + 7], BACKGROUND);
}

#[test]
fn renders_two_pages_side_by_side() {
    let mut buffer = vec![0u32; 8 * 4];
    view::render_spread(&mut buffer, 8, 4, Some(&blue()), Some(&red()));

    assert_eq!(buffer[0], 0x00_00ff);
    assert_eq!(buffer[7], 0xff_0000);
}

#[test]
fn centers_a_single_page_in_spread_mode() {
    let mut buffer = vec![0u32; 8 * 4];
    view::render_spread(&mut buffer, 8, 4, None, Some(&red()));

    assert_eq!(buffer[0], BACKGROUND);
    assert_eq!(buffer[2 * 8 + 3], 0xff_0000);
    assert_eq!(buffer[3 * 8 + 7], BACKGROUND);
}
