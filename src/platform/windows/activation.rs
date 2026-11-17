//! Prevents the click that activates an inactive window from reaching the app.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, GetWindowLongPtrW, HTCLIENT, MA_ACTIVATEANDEAT,
    SetWindowLongPtrW, WM_MOUSEACTIVATE, WNDPROC,
};

use winit::window::Window;

use crate::debug_log;

static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);

/// Subclasses the window procedure so an activation click in the client area
/// activates the window but is not delivered as a normal mouse click.
pub(crate) fn install(window: &Window) {
    if ORIGINAL_WNDPROC.load(Ordering::Relaxed) != 0 {
        return;
    }
    let Some(hwnd) = super::hwnd_of(window) else {
        return;
    };

    // SAFETY: `hwnd` belongs to the live winit window. The subclass forwards
    // every other message to the original window procedure.
    let original = unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) };
    if original == 0 {
        debug_log::log(format_args!(
            "window activation: failed to read the window procedure"
        ));
        return;
    }
    ORIGINAL_WNDPROC.store(original, Ordering::Relaxed);

    let previous = unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            subclass_proc as *const () as usize as isize,
        )
    };
    if previous == 0 {
        // Restore the original in case the new procedure was installed but the
        // previous value could not be reported.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, original) };
        ORIGINAL_WNDPROC.store(0, Ordering::Relaxed);
        debug_log::log(format_args!(
            "window activation: failed to subclass the window procedure"
        ));
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_MOUSEACTIVATE && (lparam as u32 & 0xFFFF) == HTCLIENT {
        // Activate the window, but discard the click that did it.
        return MA_ACTIVATEANDEAT as LRESULT;
    }

    let original = ORIGINAL_WNDPROC.load(Ordering::Relaxed);
    if original == 0 {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let proc: WNDPROC = unsafe { std::mem::transmute(original) };
    unsafe { CallWindowProcW(proc, hwnd, msg, wparam, lparam) }
}
