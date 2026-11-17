//! Title bar and taskbar icons generated from `assets/icon.svg` at build time.

use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Icon, WindowAttributes};

use crate::debug_log;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/window_icon.rs"));
}

/// Attaches the generated icons to a window that is about to be created.
pub(super) fn apply(attributes: WindowAttributes) -> WindowAttributes {
    let attributes = match load(
        generated::TITLEBAR_ICON_RGBA,
        generated::TITLEBAR_ICON_SIZE,
        "title bar",
    ) {
        Some(icon) => attributes.with_window_icon(Some(icon)),
        None => attributes,
    };
    match load(
        generated::TASKBAR_ICON_RGBA,
        generated::TASKBAR_ICON_SIZE,
        "taskbar",
    ) {
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
