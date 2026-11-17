use std::num::NonZeroU32;

use image::RgbaImage;

use crate::debug_log;
use crate::render::view;
use crate::render::viewer::{FrameState, Viewport, Visible, frame_state};

use super::App;

impl App {
    pub(super) fn viewport(&self) -> Viewport {
        let (width, height) = self
            .window
            .as_ref()
            .map(|window| {
                let size = window.inner_size();
                (size.width, size.height)
            })
            .unwrap_or((0, 0));
        Viewport::new(width, height, self.config.scale_filter, self.config.mode)
    }

    /// Updates the viewer when the window size, filter, or display mode
    /// changed. The page cache is cleared so images are decoded again for the
    /// new target. The previous frame stays on screen until the new images are
    /// ready.
    pub(super) fn sync_viewport(&mut self) -> bool {
        let viewport = self.viewport();
        if viewport.is_empty() {
            return false;
        }
        let Some(viewer) = self.viewer.as_mut() else {
            return false;
        };
        viewer.set_viewport(viewport)
    }

    pub(super) fn draw(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if let Err(error) = surface.resize(width, height) {
            debug_log::log(format_args!("surface resize failed: {error}"));
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else {
            debug_log::log(format_args!("surface buffer unavailable"));
            return;
        };

        let visible = self
            .viewer
            .as_ref()
            .map(|viewer| viewer.visible_pages())
            .unwrap_or(Visible::Waiting);
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
            visible.kind(),
            cached
        ));

        let current_failed = self
            .viewer
            .as_ref()
            .is_some_and(|viewer| viewer.errors.contains_key(&viewer.current));
        let waiting = matches!(visible, Visible::Waiting);
        let state = frame_state(!waiting, current_failed, waiting);
        if state == FrameState::Failed {
            view::blank(&mut buffer);
            self.last_frame.clear();
        } else if state == FrameState::Loading {
            debug_log::log(format_args!(
                "draw: hold last frame ({} bytes)",
                self.last_frame.len()
            ));
            if self.last_frame.len() == buffer.len() {
                buffer.copy_from_slice(&self.last_frame);
            } else {
                view::blank(&mut buffer);
            }
        } else {
            let start = std::time::Instant::now();
            let cells: Vec<Option<&RgbaImage>> = match &visible {
                Visible::Waiting => Vec::new(),
                Visible::Single(image) => vec![Some(image.as_ref())],
                Visible::Spread { left, right } => {
                    vec![Some(left.as_ref()), Some(right.as_ref())]
                }
                Visible::RightOnly(right) => vec![None, Some(right.as_ref())],
            };
            view::render_pages(
                &mut buffer,
                size.width as usize,
                size.height as usize,
                &cells,
            );

            self.last_frame.resize(buffer.len(), 0);
            self.last_frame.copy_from_slice(&buffer);
            debug_log::log(format_args!(
                "draw: rendered in {:.1}ms",
                start.elapsed().as_secs_f64() * 1000.0
            ));
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

        self.refresh();
    }
}
