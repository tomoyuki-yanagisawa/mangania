//! Title bar and taskbar icons generated from `assets/icon.svg` at build time.

use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Icon, WindowAttributes};

use crate::debug_log;

// Keep in sync with the sizes at the top of build.rs.
const TITLEBAR_ICON_SIZE: u32 = 32;
const TASKBAR_ICON_SIZE: u32 = 256;
const TITLEBAR_ICON_RGBA: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/mangania-titlebar.rgba"));
const TASKBAR_ICON_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mangania-taskbar.rgba"));

/// Attaches the generated icons to a window that is about to be created.
pub(crate) fn apply(attributes: WindowAttributes) -> WindowAttributes {
    let attributes = match load(TITLEBAR_ICON_RGBA, TITLEBAR_ICON_SIZE, "title bar") {
        Some(icon) => attributes.with_window_icon(Some(icon)),
        None => attributes,
    };
    match load(TASKBAR_ICON_RGBA, TASKBAR_ICON_SIZE, "taskbar") {
        Some(icon) => attributes.with_taskbar_icon(Some(icon)),
        None => attributes,
    }
}

fn load(rgba: &[u8], size: u32, role: &str) -> Option<Icon> {
    match Icon::from_rgba(rgba.to_vec(), size, size) {
        Ok(icon) => Some(icon),
        Err(error) => {
            debug_log::log(format_args!("window icon ({role}): {error}"));
            None
        }
    }
}
