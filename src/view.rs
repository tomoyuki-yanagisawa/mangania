use image::RgbaImage;
use image::imageops::FilterType;

const BACKGROUND: u32 = 0x20_20_20;

/// Fills the window with the background color.
pub fn blank(buffer: &mut [u32]) {
    buffer.fill(BACKGROUND);
}

/// Fits one page into the window.
pub fn render(buffer: &mut [u32], width: usize, height: usize, image: &RgbaImage) {
    blank(buffer);
    if width == 0 || height == 0 || image.width() == 0 || image.height() == 0 {
        return;
    }

    let (target_width, target_height) = fit(image, width, height);
    let x = (width as isize - target_width as isize) / 2;
    let y = (height as isize - target_height as isize) / 2;
    blit_scaled(
        buffer,
        width,
        height,
        image,
        (x, y),
        (target_width, target_height),
    );
}

/// Fits two pages side by side. `left` is drawn first, then `right`.
pub fn render_spread(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    left: Option<&RgbaImage>,
    right: Option<&RgbaImage>,
) {
    blank(buffer);
    if width == 0 || height == 0 {
        return;
    }

    let total_width =
        left.map_or(0, |image| image.width()) + right.map_or(0, |image| image.width());
    let max_height = left
        .map_or(0, |image| image.height())
        .max(right.map_or(0, |image| image.height()));
    if total_width == 0 || max_height == 0 {
        return;
    }

    let scale = (width as f64 / total_width as f64).min(height as f64 / max_height as f64);
    let (draw_width, draw_height) = scaled(total_width, max_height, scale);
    let start_x = (width as isize - draw_width as isize) / 2;
    let start_y = (height as isize - draw_height as isize) / 2;

    let mut x = start_x;
    for image in [left, right].into_iter().flatten() {
        let (target_width, target_height) = scaled(image.width(), image.height(), scale);
        let y = start_y + (draw_height as isize - target_height as isize) / 2;
        blit_scaled(
            buffer,
            width,
            height,
            image,
            (x, y),
            (target_width, target_height),
        );
        x += target_width as isize;
    }
}

fn fit(image: &RgbaImage, width: usize, height: usize) -> (u32, u32) {
    let (image_width, image_height) = image.dimensions();
    let scale = (width as f64 / image_width as f64).min(height as f64 / image_height as f64);
    scaled(image_width, image_height, scale)
}

fn scaled(width: u32, height: u32, scale: f64) -> (u32, u32) {
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

fn blit_scaled(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    image: &RgbaImage,
    position: (isize, isize),
    target: (u32, u32),
) {
    if target == image.dimensions() {
        blit(buffer, width, height, image, position.0, position.1);
        return;
    }

    let resized = image::imageops::resize(image, target.0, target.1, FilterType::Triangle);
    blit(buffer, width, height, &resized, position.0, position.1);
}

fn blit(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    image: &RgbaImage,
    offset_x: isize,
    offset_y: isize,
) {
    let x_start = (-offset_x).max(0) as u32;
    let y_start = (-offset_y).max(0) as u32;
    let x_end = (width as isize - offset_x)
        .min(image.width() as isize)
        .max(0) as u32;
    let y_end = (height as isize - offset_y)
        .min(image.height() as isize)
        .max(0) as u32;

    for y in y_start..y_end {
        let dest_y = (offset_y + y as isize) as usize;
        for x in x_start..x_end {
            let dest_x = (offset_x + x as isize) as usize;
            let [red, green, blue, _alpha] = image.get_pixel(x, y).0;
            buffer[dest_y * width + dest_x] =
                ((red as u32) << 16) | ((green as u32) << 8) | blue as u32;
        }
    }
}
