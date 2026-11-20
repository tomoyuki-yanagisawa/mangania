use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Fullscreen, Window as WinitWindow};

use crate::config::DisplayMode;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::config::ScaleFilter;
use crate::debug_log;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::platform::{MenuCommand, MenuState};
use crate::render::image::{blank, render_pages};
use crate::render::viewer::Frame;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::storage::disk_cache::DiskCache;

use super::App;

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

impl App {
    pub(super) fn set_mode(&mut self, mode: DisplayMode) {
        self.scroll = 0.0;
        self.config.mode = mode;
        self.config.save();
        self.status = None;
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        self.sync_menu();

        if let Some(viewer) = self.viewer.as_mut() {
            if mode == DisplayMode::Spread {
                viewer.align_spread();
            }
            viewer.prefetch.clear_pending();
        }
        // Display images are sized for the mode, so switching it invalidates
        // the page cache.
        self.sync_viewport();
        self.refresh();
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn set_scale_filter(&mut self, filter: ScaleFilter) {
        self.config.scale_filter = filter;
        self.config.save();
        self.sync_viewport();
        self.sync_menu();
        self.refresh();
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn set_allow_upscale(&mut self, allow_upscale: bool) {
        self.config.allow_upscale = allow_upscale;
        self.config.save();
        self.sync_viewport();
        self.sync_menu();
        self.refresh();
    }

    pub(super) fn set_fullscreen(&mut self, fullscreen: bool) {
        if let Some(window) = &self.window {
            window.winit().set_fullscreen(if fullscreen {
                Some(Fullscreen::Borderless(None))
            } else {
                None
            });
            // The platform menu decides whether it must be hidden in
            // fullscreen.
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            if let Some(menu) = &self.menu {
                menu.set_fullscreen(fullscreen);
            }
            window.winit().set_cursor_visible(!fullscreen);
            window.request_redraw();
        }
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        self.sync_menu();
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub(super) fn sync_menu(&self) {
        if let Some(menu) = &self.menu {
            menu.sync_state(MenuState {
                mode: self.config.mode,
                filter: self.config.scale_filter,
                fullscreen: self.is_fullscreen(),
                allow_upscale: self.config.allow_upscale,
            });
        }
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn clear_disk_cache(&mut self) {
        self.warmer = None;
        let cache = DiskCache::default();
        match cache.clear() {
            Ok(()) => {
                debug_log::log(format_args!("disk cache: cleared by menu"));
                self.status = Some("disk cache cleared".to_string());
            }
            Err(error) => {
                eprintln!("mangania: {error}");
                debug_log::log(format_args!("disk cache: clear failed: {error}"));
                self.status = Some(format!("cache clear error: {error}"));
            }
        }
        self.update_title();
        self.request_redraw();
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub(super) fn handle_menu(&mut self, command: MenuCommand, event_loop: &ActiveEventLoop) {
        match command {
            MenuCommand::Action(action) => self.perform(action, event_loop),
            MenuCommand::SetScaleFilter(filter) => self.set_scale_filter(filter),
            MenuCommand::ClearDiskCache => self.clear_disk_cache(),
            MenuCommand::ExitApplication => event_loop.exit(),
            MenuCommand::ToggleAllowUpscale => {
                let allow_upscale = !self.config.allow_upscale;
                self.set_allow_upscale(allow_upscale);
            }
        }
    }
}
