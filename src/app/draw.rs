use std::num::NonZeroU32;

use image::RgbaImage;

use crate::debug_log;
use crate::render::view;
use crate::render::viewer::{FrameState, Viewport, frame_state};

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
        Viewport::new(width, height, self.config.scale_filter, self.config.spread)
    }

    /// Updates the viewer when the window size, filter, or display mode
    /// changed. The page cache is cleared so images are decoded again for the
    /// new target.
    pub(super) fn sync_viewport(&mut self) -> bool {
        let viewport = self.viewport();
        if viewport.is_empty() {
            return false;
        }
        let Some(viewer) = self.viewer.as_mut() else {
            return false;
        };
        let changed = viewer.set_viewport(viewport);
        if changed {
            self.last_frame.clear();
        }
        changed
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

        let (current, next, waiting_for_next) = self
            .viewer
            .as_ref()
            .map_or((None, None, false), |viewer| viewer.visible_pages());
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
            "draw: {}x{} page={} current={} next={} waiting={} cached={}",
            width,
            height,
            page,
            current.is_some(),
            next.is_some(),
            waiting_for_next,
            cached
        ));

        let current_failed = self
            .viewer
            .as_ref()
            .is_some_and(|viewer| viewer.errors.contains_key(&viewer.current));
        let state = frame_state(current.is_some(), current_failed, waiting_for_next);
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
        } else if current.is_some() {
            let start = std::time::Instant::now();
            let pages: Vec<Option<&RgbaImage>> = if self.config.spread {
                vec![next.as_deref(), current.as_deref()]
            } else {
                vec![current.as_deref()]
            };
            view::render_pages(
                &mut buffer,
                size.width as usize,
                size.height as usize,
                &pages,
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
