use winit::window::Fullscreen;

#[cfg(any(target_os = "windows", target_os = "macos"))]
use winit::event_loop::ActiveEventLoop;

use crate::config::DisplayMode;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::config::ScaleFilter;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::debug_log;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::platform::{MenuCommand, MenuState};
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::storage::disk_cache::DiskCache;

use super::App;

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

    pub(super) fn set_fullscreen(&mut self, fullscreen: bool) {
        if let Some(window) = &self.window {
            window.set_fullscreen(if fullscreen {
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
            window.set_cursor_visible(!fullscreen);
            window.request_redraw();
        }
        self.update_fullscreen_menu(fullscreen);
    }

    pub(super) fn update_fullscreen_menu(&self, _fullscreen: bool) {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        self.sync_menu_with(_fullscreen);
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn sync_menu(&self) {
        let fullscreen = self.is_fullscreen();
        self.sync_menu_with(fullscreen);
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn sync_menu_with(&self, fullscreen: bool) {
        if let Some(menu) = &self.menu {
            menu.sync_state(MenuState {
                mode: self.config.mode,
                filter: self.config.scale_filter,
                fullscreen,
            });
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
    pub(super) fn handle_menu(&mut self, command: MenuCommand, event_loop: &ActiveEventLoop) {
        match command {
            MenuCommand::Action(action) => self.perform(action, event_loop),
            MenuCommand::SetScaleFilter(filter) => self.set_scale_filter(filter),
            MenuCommand::ClearDiskCache => self.clear_disk_cache(),
            MenuCommand::ExitApplication => event_loop.exit(),
        }
    }
}
