use winit::window::Fullscreen;

#[cfg(any(target_os = "windows", target_os = "macos"))]
use winit::event_loop::ActiveEventLoop;

#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::config::ScaleFilter;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::debug_log;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::disk_cache::DiskCache;

use super::App;

impl App {
    pub(super) fn set_spread(&mut self, spread: bool) {
        self.scroll = 0.0;
        self.config.spread = spread;
        self.config.save();
        self.status = None;
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if let Some(menu) = &self.menu {
            menu.single.set_checked(!spread);
            menu.spread.set_checked(spread);
        }

        if let Some(viewer) = self.viewer.as_mut() {
            if spread {
                viewer.align_spread();
            }
            viewer.prefetch.clear_pending();
        }
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
