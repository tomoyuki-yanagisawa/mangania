use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::Window as WinitWindow;

use crate::debug_log;
use crate::render::image::{blank, render_pages};
use crate::render::viewer::Frame;

type Surface = softbuffer::Surface<Arc<WinitWindow>, Arc<WinitWindow>>;
type Buffer<'a> = softbuffer::Buffer<'a, Arc<WinitWindow>, Arc<WinitWindow>>;

/// The native window and its software drawing surface.
pub(super) struct Window {
    window: Arc<WinitWindow>,
    surface: Surface,
    last_frame: Vec<u32>,
}

impl Window {
    pub(super) fn new(event_loop: &ActiveEventLoop) -> Result<Self, String> {
        let attributes = WinitWindow::default_attributes()
            .with_title("MangaNia")
            .with_inner_size(LogicalSize::new(1200.0, 900.0));
        #[cfg(target_os = "windows")]
        let attributes = crate::platform::apply_window_icon(attributes);
        let window = event_loop
            .create_window(attributes)
            .map_err(|error| format!("create window: {error}"))?;
        let window = Arc::new(window);
        let context = softbuffer::Context::new(window.clone())
            .map_err(|error| format!("create display context: {error}"))?;
        let surface = softbuffer::Surface::new(&context, window.clone())
            .map_err(|error| format!("create surface: {error}"))?;
        Ok(Self {
            window,
            surface,
            last_frame: Vec::new(),
        })
    }

    pub(super) fn winit(&self) -> &WinitWindow {
        &self.window
    }

    pub(super) fn size(&self) -> PhysicalSize<u32> {
        self.window.inner_size()
    }

    pub(super) fn buffer_mut(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Option<Buffer<'_>> {
        if let Err(error) = self.surface.resize(width, height) {
            debug_log::log(format_args!("surface resize failed: {error}"));
            return None;
        }
        match self.surface.buffer_mut() {
            Ok(buffer) => Some(buffer),
            Err(error) => {
                debug_log::log(format_args!("surface buffer unavailable: {error}"));
                None
            }
        }
    }

    pub(super) fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub(super) fn clear_frame(&mut self) {
        self.last_frame.clear();
    }

    pub(super) fn draw(&mut self, frame: Frame) {
        let size = self.size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        let mut last_frame = std::mem::take(&mut self.last_frame);
        let Some(mut buffer) = self.buffer_mut(width, height) else {
            self.last_frame = last_frame;
            return;
        };

        match frame {
            Frame::Failed => {
                blank(&mut buffer);
                last_frame.clear();
            }
            Frame::Loading => {
                debug_log::log(format_args!(
                    "draw: hold last frame ({} bytes)",
                    last_frame.len()
                ));
                if last_frame.len() == buffer.len() {
                    buffer.copy_from_slice(&last_frame);
                } else {
                    blank(&mut buffer);
                }
            }
            Frame::Single(image) => {
                draw_cells(
                    &mut buffer,
                    &mut last_frame,
                    size.width,
                    size.height,
                    &[Some(image.as_ref())],
                );
            }
            Frame::Spread { left, right } => {
                draw_cells(
                    &mut buffer,
                    &mut last_frame,
                    size.width,
                    size.height,
                    &[Some(left.as_ref()), Some(right.as_ref())],
                );
            }
            Frame::RightOnly(right) => {
                draw_cells(
                    &mut buffer,
                    &mut last_frame,
                    size.width,
                    size.height,
                    &[None, Some(right.as_ref())],
                );
            }
        }
        if let Err(error) = buffer.present() {
            debug_log::log(format_args!("surface present failed: {error}"));
        }
        self.last_frame = last_frame;
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
    let start = Instant::now();
    render_pages(buffer, width as usize, height as usize, cells);

    last_frame.resize(buffer.len(), 0);
    last_frame.copy_from_slice(buffer);
    debug_log::log(format_args!(
        "draw: rendered in {:.1}ms",
        start.elapsed().as_secs_f64() * 1000.0
    ));
}
