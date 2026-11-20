pub(crate) mod console;
pub(crate) mod icon;
pub(crate) mod menu_bar;
pub(crate) mod shortcut;

use winit::window::Window;

/// Returns the `HWND` behind a live winit window.
pub(crate) fn hwnd_of(window: &Window) -> Option<isize> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}
