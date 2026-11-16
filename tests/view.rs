use image::{Rgba, RgbaImage};
use mangania::render::view;

const BACKGROUND: u32 = 0x20_20_20;

fn solid(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
    RgbaImage::from_pixel(width, height, Rgba(color))
}

fn red() -> RgbaImage {
    solid(2, 2, [255, 0, 0, 255])
}

#[test]
fn renders_a_single_page_centered() {
    let mut buffer = vec![0u32; 8 * 4];
    view::render_pages(&mut buffer, 8, 4, &[Some(&red())]);

    assert_eq!(buffer[0], BACKGROUND);
    assert_eq!(buffer[11], 0xff_0000);
    assert_eq!(buffer[2 * 8 + 4], 0xff_0000);
    assert_eq!(buffer[3 * 8 + 7], BACKGROUND);
}

#[test]
fn renders_two_pages_in_half_width_cells() {
    let left = solid(4, 4, [0, 0, 255, 255]);
    let right = solid(4, 4, [255, 0, 0, 255]);
    let mut buffer = vec![0u32; 8 * 4];

    view::render_pages(&mut buffer, 8, 4, &[Some(&left), Some(&right)]);

    assert_eq!(buffer[0], 0x00_00ff);
    assert_eq!(buffer[3], 0x00_00ff);
    assert_eq!(buffer[4], 0xff_0000);
    assert_eq!(buffer[7], 0xff_0000);
}

#[test]
fn aligns_spread_pages_to_the_middle_gutter() {
    // Both images are narrower than their half-width cell, so they must meet
    // at the gutter instead of being centered in each cell.
    let left = solid(2, 4, [0, 0, 255, 255]);
    let right = solid(2, 4, [255, 0, 0, 255]);
    let mut buffer = vec![0u32; 8 * 4];

    view::render_pages(&mut buffer, 8, 4, &[Some(&left), Some(&right)]);

    assert_eq!(buffer[0], BACKGROUND);
    assert_eq!(buffer[1], BACKGROUND);
    assert_eq!(buffer[2], 0x00_00ff);
    assert_eq!(buffer[3], 0x00_00ff);
    assert_eq!(buffer[4], 0xff_0000);
    assert_eq!(buffer[5], 0xff_0000);
    assert_eq!(buffer[6], BACKGROUND);
}

#[test]
fn leaves_a_missing_left_cell_blank() {
    let right = solid(2, 4, [255, 0, 0, 255]);
    let mut buffer = vec![0u32; 8 * 4];

    view::render_pages(&mut buffer, 8, 4, &[None, Some(&right)]);

    assert_eq!(buffer[0], BACKGROUND);
    assert_eq!(buffer[4], 0xff_0000);
}

#[test]
fn leaves_a_missing_right_cell_blank() {
    let left = solid(2, 4, [0, 0, 255, 255]);
    let mut buffer = vec![0u32; 8 * 4];

    view::render_pages(&mut buffer, 8, 4, &[Some(&left), None]);

    assert_eq!(buffer[0], BACKGROUND);
    assert_eq!(buffer[2], 0x00_00ff);
    assert_eq!(buffer[6], BACKGROUND);
}

#[test]
fn fits_display_images_inside_the_viewport() {
    assert_eq!(view::fit(2, 1, 8, 8), (8, 4));
    assert_eq!(view::fit(1, 2, 8, 8), (4, 8));
    assert_eq!(view::fit(8, 8, 4, 2), (2, 2));
    assert_eq!(view::fit(0, 100, 8, 8), (0, 0));
    assert_eq!(view::fit(100, 100, 0, 8), (0, 0));
}
