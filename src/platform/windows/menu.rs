//! Win32 native menu bar.

use std::cell::Cell;
use std::ptr;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuItem, CreateMenu, CreatePopupMenu, DrawMenuBar, HTCLIENT,
    MA_ACTIVATEANDEAT, MENU_ITEM_FLAGS, MF_BYCOMMAND, MF_CHECKED, MF_POPUP, MF_SEPARATOR,
    MF_STRING, MF_UNCHECKED, SetMenu, WM_MOUSEACTIVATE,
};

use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use super::hwnd_of;

use crate::config::{DisplayMode, ScaleFilter};
use crate::debug_log;
use crate::event::UserEvent;
use crate::input::bindings::Action;
use crate::platform::menu::{NAVIGATION_ITEMS, labels};
use crate::platform::{MenuCommand, MenuState, action_for_mode};

const SUBCLASS_ID: usize = 1;

/// The native menu bar attached to a window.
pub(crate) struct MenuBar {
    menu: isize,
    view: isize,
    filters: isize,
    window: Cell<isize>,
    proxy: EventLoopProxy<UserEvent>,
}

impl MenuBar {
    pub(crate) fn new(
        mode: DisplayMode,
        current_filter: ScaleFilter,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        let menu = unsafe { CreateMenu() };
        let file = outlined(unsafe { CreatePopupMenu() });
        let navigation = outlined(unsafe { CreatePopupMenu() });
        let view = outlined(unsafe { CreatePopupMenu() });
        let filters = outlined(unsafe { CreatePopupMenu() });
        let app = outlined(unsafe { CreatePopupMenu() });

        // The application menu keeps the existing quit entry on Windows.
        append_item(app, MenuCommand::ExitApplication, labels::QUIT);

        append_item(
            file,
            MenuCommand::Action(Action::SavePage),
            labels::SAVE_PAGE,
        );
        append_separator(file);
        append_item(file, MenuCommand::ClearDiskCache, labels::CLEAR_CACHE);

        for (label, action) in NAVIGATION_ITEMS {
            append_item(navigation, MenuCommand::Action(action), label);
        }

        for display_mode in [DisplayMode::Single, DisplayMode::Spread, DisplayMode::Mixed] {
            let checked = display_mode == mode;
            append_item_checked(
                view,
                MenuCommand::Action(action_for_mode(display_mode)),
                display_mode.label(),
                checked,
            );
        }
        append_separator(view);
        append_item(
            view,
            MenuCommand::Action(Action::ToggleFullscreen),
            labels::FULLSCREEN,
        );

        for filter in ScaleFilter::ALL {
            append_item_checked(
                filters,
                MenuCommand::SetScaleFilter(filter),
                filter.label(),
                filter == current_filter,
            );
        }
        append_submenu(view, filters, labels::FILTERS);

        append_submenu(menu, app, "MangaNia");
        append_submenu(menu, file, labels::FILE);
        append_submenu(menu, navigation, labels::NAVIGATION);
        append_submenu(menu, view, labels::VIEW);

        Self {
            menu,
            view,
            filters,
            window: Cell::new(0),
            proxy,
        }
    }

    /// Attaches the menu bar to the window and starts receiving `WM_COMMAND`.
    pub(crate) fn attach(&self, window: &Window) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        self.window.set(hwnd);

        // SAFETY: `hwnd` belongs to the live winit window, and `menu` is owned
        // by `MenuBar` for the lifetime of the application.
        unsafe {
            SetMenu(hwnd, self.menu);
            DrawMenuBar(hwnd);
        }

        let proxy = Box::into_raw(Box::new(self.proxy.clone()));
        // SAFETY: the pointer stays valid until `WM_NCDESTROY`, where it is
        // reclaimed and freed.
        let installed =
            unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, proxy as usize) };
        if installed == 0 {
            // SAFETY: `installed == 0` means the callback will never run.
            drop(unsafe { Box::from_raw(proxy) });
            debug_log::log(format_args!(
                "native menu: failed to subclass the window procedure"
            ));
        }
    }

    pub(crate) fn sync_state(&self, state: MenuState) {
        for display_mode in [DisplayMode::Single, DisplayMode::Spread, DisplayMode::Mixed] {
            check(
                self.view,
                MenuCommand::Action(action_for_mode(display_mode)),
                display_mode == state.mode,
            );
        }
        for filter in ScaleFilter::ALL {
            check(
                self.filters,
                MenuCommand::SetScaleFilter(filter),
                filter == state.filter,
            );
        }
        check(
            self.view,
            MenuCommand::Action(Action::ToggleFullscreen),
            state.fullscreen,
        );
    }

    /// Windows keeps the menu bar visible in borderless fullscreen, so detach
    /// it while fullscreen and restore the same `HMENU`, including check states.
    pub(crate) fn set_fullscreen(&self, fullscreen: bool) {
        let hwnd = self.window.get();
        if hwnd == 0 {
            return;
        }
        // SAFETY: `hwnd` belongs to the live winit window and `menu` is owned
        // by `MenuBar`.
        unsafe {
            SetMenu(hwnd, if fullscreen { 0 } else { self.menu });
            DrawMenuBar(hwnd);
        }
    }
}

fn outlined(menu: isize) -> isize {
    if menu == 0 {
        debug_log::log(format_args!("native menu: failed to create a popup menu"));
    }
    menu
}

fn append_item(menu: isize, command: MenuCommand, label: &str) {
    append_item_checked(menu, command, label, false);
}

fn append_item_checked(menu: isize, command: MenuCommand, label: &str, checked: bool) {
    let label = wide(label);
    let flags = item_flags(checked);
    // SAFETY: `menu` is a live menu handle and `label` is null-terminated for
    // the duration of the call.
    unsafe {
        AppendMenuW(menu, flags, command.id() as usize, label.as_ptr());
    }
}

fn append_separator(menu: isize) {
    // SAFETY: `menu` is a live menu handle.
    unsafe {
        AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
    }
}

fn append_submenu(menu: isize, submenu: isize, label: &str) {
    let label = wide(label);
    // SAFETY: `menu` and `submenu` are live menu handles and `label` is
    // null-terminated for the duration of the call.
    unsafe {
        AppendMenuW(menu, MF_POPUP | MF_STRING, submenu as usize, label.as_ptr());
    }
}

fn check(menu: isize, command: MenuCommand, checked: bool) {
    // SAFETY: `menu` is a live menu handle.
    unsafe {
        CheckMenuItem(
            menu,
            command.id() as u32,
            MF_BYCOMMAND | item_flags(checked),
        );
    }
}

fn item_flags(checked: bool) -> MENU_ITEM_FLAGS {
    MF_STRING | if checked { MF_CHECKED } else { MF_UNCHECKED }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    match msg {
        windows_sys::Win32::UI::WindowsAndMessaging::WM_COMMAND => {
            // Menu commands have an empty high word; control notifications use
            // the high word for the notification code.
            if (wparam >> 16) == 0
                && let Some(command) = MenuCommand::from_id(wparam as u16)
            {
                // SAFETY: `refdata` points to the `EventLoopProxy` allocated in
                // `attach` and freed below on `WM_NCDESTROY`.
                let proxy = unsafe { &*(refdata as *const EventLoopProxy<UserEvent>) };
                let _ = proxy.send_event(UserEvent::Menu(command));
                return 0;
            }
        }
        WM_MOUSEACTIVATE => {
            // A click that activates the window must not reach the app as a
            // normal mouse click.
            if (lparam as u32 & 0xFFFF) == HTCLIENT {
                return MA_ACTIVATEANDEAT as LRESULT;
            }
        }
        windows_sys::Win32::UI::WindowsAndMessaging::WM_NCDESTROY => {
            // SAFETY: the window is being destroyed and `attach` installed the
            // boxed proxy for this subclass.
            unsafe {
                RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
                drop(Box::from_raw(refdata as *mut EventLoopProxy<UserEvent>));
            }
        }
        _ => {}
    }

    // SAFETY: every message that is not handled above is forwarded to the
    // previous window procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
