use std::cell::Cell;

use muda::Menu;
use winit::window::Window;

/// Win32 state for the native menu bar attached to a window.
#[derive(Default)]
pub(crate) struct NativeMenu {
    /// Menu detached from the window by [`NativeMenu::set_visible`].
    /// `0` means the menu is currently attached (or was never attached).
    saved_hmenu: Cell<isize>,
}

impl NativeMenu {
    /// Attaches `menu` to the window's `HWND`.
    pub(crate) fn attach(&self, menu: &Menu, window: &Window) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        // SAFETY: the HWND comes from the live winit window.
        unsafe {
            if let Err(error) = menu.init_for_hwnd(hwnd) {
                eprintln!("mangania: create menu: {error}");
            }
        }
    }

    /// Shows or hides the native menu bar attached to `window`.
    ///
    /// Win32 has no dedicated hide call, so the menu bar is detached with
    /// `SetMenu(hwnd, NULL)`. The `HMENU` returned by `GetMenu` is saved first
    /// and restored afterwards, which keeps the exact same menu (including the
    /// check state of the view items) across repeated fullscreen toggles.
    pub(crate) fn set_visible(&self, window: &Window, visible: bool) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{DrawMenuBar, GetMenu, SetMenu};

        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        // SAFETY: `hwnd` belongs to the live winit window. `saved_hmenu` is the
        // menu that `attach` installed on the same window, and it is only kept
        // while the window is alive.
        unsafe {
            if visible {
                let saved = self.saved_hmenu.replace(0);
                if saved != 0 {
                    SetMenu(hwnd, saved);
                    DrawMenuBar(hwnd);
                }
            } else {
                let current = GetMenu(hwnd);
                if current == 0 {
                    // Already hidden; keep the handle saved by the first call.
                    return;
                }
                if self.saved_hmenu.get() == 0 {
                    self.saved_hmenu.set(current);
                }
                SetMenu(hwnd, 0);
                DrawMenuBar(hwnd);
            }
        }
    }
}

/// Returns the `HWND` behind a live winit window.
fn hwnd_of(window: &Window) -> Option<isize> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}
