use std::num::NonZeroU32;

use image::RgbaImage;

use crate::debug_log;
use crate::render::view;
use crate::render::viewer::{Frame, Viewport};

use super::App;

impl App {
    pub(super) fn viewport(&self) -> Viewport {
        let (width, height) = self
            .window
            .as_ref()
            .map(|window| {
                let size = window.size();
                (size.width, size.height)
            })
            .unwrap_or((0, 0));
        Viewport::new(width, height, self.config.scale_filter, self.config.mode)
    }

    /// Updates the viewer when the window size, filter, or display mode
    /// changed. The page cache is cleared so images are decoded again for the
    /// new target. The previous frame stays on screen until the new images are
    /// ready.
    pub(super) fn sync_viewport(&mut self) {
        let viewport = self.viewport();
        if viewport.is_empty() {
            return;
        }
        let Some(viewer) = self.viewer.as_mut() else {
            return;
        };
        viewer.set_viewport(viewport);
    }

    pub(super) fn draw(&mut self) {
        let Some(window) = self.window.as_mut() else {
            return;
        };
        let size = window.size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        let Some(mut buffer) = window.buffer_mut(width, height) else {
            return;
        };

        let frame = self
            .viewer
            .as_ref()
            .map(|viewer| viewer.frame())
            .unwrap_or(Frame::Loading);
        let page = self
            .viewer
            .as_ref()
            .map(|viewer| viewer.current + 1)
            .unwrap_or(0);
        let cached = self
            .viewer
            .as_ref()
            .is_some_and(|viewer| viewer.has_page(viewer.current));
        debug_log::log(format_args!(
            "draw: {}x{} page={} display={} cached={}",
            width,
            height,
            page,
            frame.kind().as_str(),
            cached
        ));

        match frame {
            Frame::Failed => {
                view::blank(&mut buffer);
                self.last_frame.clear();
            }
            Frame::Loading => {
                debug_log::log(format_args!(
                    "draw: hold last frame ({} bytes)",
                    self.last_frame.len()
                ));
                if self.last_frame.len() == buffer.len() {
                    buffer.copy_from_slice(&self.last_frame);
                } else {
                    view::blank(&mut buffer);
                }
            }
            Frame::Single(image) => {
                draw_cells(
                    &mut buffer,
                    &mut self.last_frame,
                    size.width,
                    size.height,
                    &[Some(image.as_ref())],
                );
            }
            Frame::Spread { left, right } => {
                draw_cells(
                    &mut buffer,
                    &mut self.last_frame,
                    size.width,
                    size.height,
                    &[Some(left.as_ref()), Some(right.as_ref())],
                );
            }
            Frame::RightOnly(right) => {
                draw_cells(
                    &mut buffer,
                    &mut self.last_frame,
                    size.width,
                    size.height,
                    &[None, Some(right.as_ref())],
                );
            }
        }
        if let Err(error) = buffer.present() {
            debug_log::log(format_args!("surface present failed: {error}"));
        }
    }

    pub(super) fn handle_images_ready(&mut self) {
        let Some(viewer) = self.viewer.as_mut() else {
            return;
        };

        let results = viewer.prefetch.poll();
        if results.is_empty() {
            return;
        }

        let before = viewer.frame().kind();
        for (page, result) in results {
            match result {
                Ok(image) => {
                    debug_log::log(format_args!(
                        "page {} ready: {}x{} ({} bytes)",
                        page + 1,
                        image.width(),
                        image.height(),
                        image.as_raw().len()
                    ));
                    viewer.insert_decoded(page, image);
                }
                Err(message) => {
                    eprintln!("mangania: page {}: {message}", page + 1);
                    debug_log::log(format_args!("page {} failed: {message}", page + 1));
                    viewer.errors.insert(page, message);
                }
            }
        }

        // Prefetched pages only change the screen when they complete the
        // current display. Redrawing for them would present the same image
        // again, which looks like a flicker on some systems.
        if viewer.frame().kind() != before {
            self.refresh();
        }
    }
}

/// Draws the page cells and keeps the result to hold while loading.
fn draw_cells(
    buffer: &mut [u32],
    last_frame: &mut Vec<u32>,
    width: u32,
    height: u32,
    cells: &[Option<&RgbaImage>],
) {
    let start = std::time::Instant::now();
    view::render_pages(buffer, width as usize, height as usize, cells);

    last_frame.resize(buffer.len(), 0);
    last_frame.copy_from_slice(buffer);
    debug_log::log(format_args!(
        "draw: rendered in {:.1}ms",
        start.elapsed().as_secs_f64() * 1000.0
    ));
}
