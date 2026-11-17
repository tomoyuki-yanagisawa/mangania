use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Fullscreen, Window, WindowAttributes};

#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::config::ScaleFilter;
use crate::config::{DisplayMode, WindowGeometry};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::debug_log;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::storage::disk_cache::DiskCache;

use super::{App, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH};

impl App {
    /// Window attributes for the first window, restoring the saved size when it
    /// is usable. The position is applied after creation by
    /// [`App::restored_position`] so it matches the stored outer position on
    /// every platform.
    pub(super) fn window_attributes(&self) -> WindowAttributes {
        let mut attributes = Window::default_attributes()
            .with_title("MangaNia")
            .with_inner_size(LogicalSize::new(
                DEFAULT_WINDOW_WIDTH,
                DEFAULT_WINDOW_HEIGHT,
            ));
        if let Some((width, height)) = self.config.window.and_then(WindowGeometry::size) {
            attributes = attributes.with_inner_size(PhysicalSize::new(width, height));
        }
        attributes
    }

    /// Saved outer position when it overlaps an available monitor.
    pub(super) fn restored_position(&self, event_loop: &ActiveEventLoop) -> Option<(i32, i32)> {
        let geometry = self.config.window?;
        let position = geometry.position()?;
        let size = geometry
            .size()
            .unwrap_or((DEFAULT_WINDOW_WIDTH as u32, DEFAULT_WINDOW_HEIGHT as u32));
        position_is_visible(event_loop, position, size).then_some(position)
    }

    /// Remember the latest windowed size and position without writing config.
    pub(super) fn track_window_geometry(&mut self) {
        if self.is_fullscreen() {
            return;
        }
        let Some(window) = &self.window else {
            return;
        };
        if window.is_minimized() == Some(true) {
            return;
        }
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let position = window.outer_position().ok();
        self.window_geometry = Some(WindowGeometry {
            width: size.width,
            height: size.height,
            x: position.map(|position| position.x),
            y: position.map(|position| position.y),
        });
    }

    /// Persist the tracked windowed geometry if it changed.
    pub(super) fn save_window_geometry(&mut self) {
        self.track_window_geometry();
        let Some(geometry) = self.window_geometry else {
            return;
        };
        if self.config.window != Some(geometry) {
            self.config.window = Some(geometry);
            self.config.save();
        }
    }

    pub(super) fn set_mode(&mut self, mode: DisplayMode) {
        self.scroll = 0.0;
        self.config.mode = mode;
        self.config.save();
        self.status = None;
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if let Some(menu) = &self.menu {
            menu.set_mode_checked(mode);
        }

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
        if let Some(menu) = &self.menu {
            menu.set_filter_checked(filter);
        }
        self.refresh();
    }

    pub(super) fn set_fullscreen(&mut self, fullscreen: bool) {
        if let Some(window) = &self.window {
            window.set_fullscreen(if fullscreen {
                Some(Fullscreen::Borderless(None))
            } else {
                None
            });
            // Windows keeps the native menu bar visible in borderless
            // fullscreen, so detach it while fullscreen and restore it after.
            #[cfg(target_os = "windows")]
            if let Some(menu) = &self.menu {
                menu.set_native_menu_visible(window, !fullscreen);
            }
            window.set_cursor_visible(!fullscreen);
            window.request_redraw();
        }
        self.update_fullscreen_menu(fullscreen);
    }

    pub(super) fn update_fullscreen_menu(&self, _fullscreen: bool) {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if let Some(menu) = &self.menu {
            menu.fullscreen.set_checked(_fullscreen);
        }
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn clear_disk_cache(&mut self) {
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
    pub(super) fn handle_menu(&mut self, event: muda::MenuEvent, event_loop: &ActiveEventLoop) {
        let filter = self
            .menu
            .as_ref()
            .and_then(|menu| menu.filter_for(&event.id));
        if let Some(filter) = filter {
            self.set_scale_filter(filter);
            return;
        }
        if self
            .menu
            .as_ref()
            .is_some_and(|menu| menu.is_clear_cache(&event.id))
        {
            self.clear_disk_cache();
            return;
        }

        let action = self
            .menu
            .as_ref()
            .and_then(|menu| menu.action_for(&event.id));
        if let Some(action) = action {
            self.perform(action, event_loop);
        }
    }
}

/// Whether the window rectangle overlaps any available monitor. Positions
/// outside every monitor (for example after unplugging a display) are ignored.
fn position_is_visible(
    event_loop: &ActiveEventLoop,
    (x, y): (i32, i32),
    (width, height): (u32, u32),
) -> bool {
    let left = i64::from(x);
    let top = i64::from(y);
    let right = left + i64::from(width);
    let bottom = top + i64::from(height);
    event_loop.available_monitors().any(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        let monitor_left = i64::from(position.x);
        let monitor_top = i64::from(position.y);
        let monitor_right = monitor_left + i64::from(size.width);
        let monitor_bottom = monitor_top + i64::from(size.height);
        left < monitor_right && right > monitor_left && top < monitor_bottom && bottom > monitor_top
    })
}
