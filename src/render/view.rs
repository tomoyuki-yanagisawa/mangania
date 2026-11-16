use image::RgbaImage;

const BACKGROUND: u32 = 0x20_20_20;

/// Fills the window with the background color.
pub fn blank(buffer: &mut [u32]) {
    buffer.fill(BACKGROUND);
}

/// Draws one page centered, or two pages in left/right cells.
///
/// Spread pages meet at the middle gutter: the left page is aligned to the
/// right edge of the left cell, and the right page to the left edge of the
/// right cell. Missing pages leave their cell blank. Images are expected to
/// be display-sized already; this function only copies pixels.
pub fn render_pages(buffer: &mut [u32], width: usize, height: usize, pages: &[Option<&RgbaImage>]) {
    blank(buffer);
    if width == 0 || height == 0 {
        return;
    }

    if let [page] = pages {
        if let Some(image) = page {
            place(buffer, width, height, image, Center::Full);
        }
        return;
    }

    let gutter = width / 2;
    if let Some(image) = pages.first().copied().flatten() {
        place(buffer, width, height, image, Center::LeftCell { gutter });
    }
    if let Some(image) = pages.get(1).copied().flatten() {
        place(buffer, width, height, image, Center::RightCell { gutter });
    }
}

enum Center {
    Full,
    LeftCell { gutter: usize },
    RightCell { gutter: usize },
}

fn place(buffer: &mut [u32], width: usize, height: usize, image: &RgbaImage, center: Center) {
    let x = match center {
        Center::Full => (width as isize - image.width() as isize) / 2,
        Center::LeftCell { gutter } => gutter as isize - image.width() as isize,
        Center::RightCell { gutter } => gutter as isize,
    };
    let y = (height as isize - image.height() as isize) / 2;
    blit(buffer, width, height, image, x, y);
}

/// Returns the display size of an image fitted into the viewport while
/// preserving its aspect ratio.
pub fn fit(width: u32, height: u32, max_width: usize, max_height: usize) -> (u32, u32) {
    if width == 0 || height == 0 || max_width == 0 || max_height == 0 {
        return (0, 0);
    }
    let scale = (max_width as f64 / width as f64).min(max_height as f64 / height as f64);
    scaled(width, height, scale)
}

fn scaled(width: u32, height: u32, scale: f64) -> (u32, u32) {
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

fn blit(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    image: &RgbaImage,
    offset_x: isize,
    offset_y: isize,
) {
    let image_width = image.width() as usize;
    let image_height = image.height() as usize;
    if width == 0 || height == 0 || image_width == 0 || image_height == 0 {
        return;
    }

    let x_start = (-offset_x).max(0) as usize;
    let y_start = (-offset_y).max(0) as usize;
    let x_end = (width as isize - offset_x).min(image_width as isize).max(0) as usize;
    let y_end = (height as isize - offset_y)
        .min(image_height as isize)
        .max(0) as usize;
    if x_start >= x_end || y_start >= y_end {
        return;
    }

    let dest_x = (offset_x + x_start as isize) as usize;
    let row_len = x_end - x_start;
    let source = image.as_raw();
    for y in y_start..y_end {
        let dest_y = (offset_y + y as isize) as usize;
        let source_start = (y * image_width + x_start) * 4;
        let source_row = &source[source_start..source_start + row_len * 4];
        let dest_row = &mut buffer[dest_y * width + dest_x..][..row_len];
        let (pixels, _) = source_row.as_chunks::<4>();
        for (dest, pixel) in dest_row.iter_mut().zip(pixels) {
            *dest = (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]);
        }
    }
}
