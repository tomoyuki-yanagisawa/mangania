use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::RgbaImage;

use crate::config::ScaleFilter;
use crate::debug_log;

thread_local! {
    /// Reused so repeated page resizes do not reallocate internal buffers.
    static RESIZER: RefCell<Resizer> = RefCell::new(Resizer::new());
}

/// Resizes an image to a display target and logs the cost.
pub(crate) fn resized(
    image: &RgbaImage,
    target: (u32, u32),
    filter: ScaleFilter,
) -> Arc<RgbaImage> {
    let start = Instant::now();
    let mut output = RgbaImage::new(target.0, target.1);
    let options = ResizeOptions::new()
        .resize_alg(resize_algorithm(filter))
        // Pages are drawn without alpha blending, so the premultiply and
        // unpremultiply passes are unnecessary.
        .use_alpha(false);
    if let Err(error) =
        RESIZER.with(|resizer| resizer.borrow_mut().resize(image, &mut output, &options))
    {
        // RGBA8 with a non-zero target is always supported, so this should not
        // happen. Leave the zero-filled target and log it instead of panicking.
        debug_log::log(format_args!("resize: fast_image_resize failed: {error}"));
    }
    debug_log::log(format_args!(
        "resize: {}x{} -> {}x{} filter={} in {:.1}ms",
        image.width(),
        image.height(),
        target.0,
        target.1,
        filter.as_str(),
        start.elapsed().as_secs_f64() * 1000.0
    ));
    Arc::new(output)
}

fn resize_algorithm(filter: ScaleFilter) -> ResizeAlg {
    match filter {
        ScaleFilter::Nearest => ResizeAlg::Nearest,
        ScaleFilter::Triangle => ResizeAlg::Convolution(FilterType::Bilinear),
        ScaleFilter::Hamming => ResizeAlg::Convolution(FilterType::Hamming),
        ScaleFilter::CatmullRom => ResizeAlg::Convolution(FilterType::CatmullRom),
        ScaleFilter::Mitchell => ResizeAlg::Convolution(FilterType::Mitchell),
        ScaleFilter::Gaussian => ResizeAlg::Convolution(FilterType::Gaussian),
        ScaleFilter::Lanczos3 => ResizeAlg::Convolution(FilterType::Lanczos3),
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};

    use super::resized;
    use crate::config::ScaleFilter;

    #[test]
    fn resizes_with_every_filter() {
        let image = RgbaImage::from_pixel(7, 5, Rgba([10, 20, 30, 255]));

        for filter in ScaleFilter::ALL {
            let output = resized(&image, (3, 2), filter);
            assert_eq!(output.dimensions(), (3, 2), "{}", filter.as_str());
            assert!(
                output.pixels().all(|pixel| pixel.0 == [10, 20, 30, 255]),
                "{}",
                filter.as_str()
            );
        }
    }
}
