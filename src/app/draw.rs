use crate::debug_log;
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
        Viewport::new(
            width,
            height,
            self.config.scale_filter,
            self.config.mode,
            self.config.allow_upscale,
        )
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
            "draw: page={} display={} cached={}",
            page,
            frame.kind().as_str(),
            cached
        ));

        if let Some(window) = self.window.as_mut() {
            window.draw(frame);
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
        // again, which looks like a flicker on some systems. Rebuild the queue
        // even when the frame does not change so the next pages stay queued.
        viewer.request_display();
        if viewer.frame().kind() != before {
            self.refresh();
        }
    }
}
