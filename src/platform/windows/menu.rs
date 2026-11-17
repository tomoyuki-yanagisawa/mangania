//! Win32 native menu bar.

use std::cell::Cell;
use std::ptr;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuItem, CreateMenu, CreatePopupMenu, DrawMenuBar, HTCLIENT,
    MA_ACTIVATEANDEAT, MENU_ITEM_FLAGS, MF_BYCOMMAND, MF_CHECKED, MF_POPUP, MF_SEPARATOR,
    MF_STRING, MF_UNCHECKED, SetMenu, WM_COMMAND, WM_MOUSEACTIVATE, WM_NCDESTROY,
};

use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use super::hwnd_of;

use crate::config::{DisplayMode, ScaleFilter};
use crate::debug_log;
use crate::event::UserEvent;
use crate::input::bindings::Action;
use crate::platform::{MenuCommand, MenuState};

const SUBCLASS_ID: usize = 1;

/// A non-null Win32 `HMENU`.
#[derive(Clone, Copy)]
struct HMenu(isize);

impl HMenu {
    /// Returns `None` for the null handle that indicates a Win32 failure.
    fn from_raw(handle: isize) -> Option<Self> {
        (handle != 0).then_some(Self(handle))
    }

    fn raw(self) -> isize {
        self.0
    }
}

/// The menu handles whose item states the application updates.
///
/// They are owned for the window/process lifetime: `DestroyWindow` destroys an
/// assigned menu, and the OS reclaims an unassigned one at process exit.
/// `set_fullscreen` temporarily unassigns the bar with `SetMenu(hwnd, NULL)`.
/// A `Drop` calling `DestroyMenu` could free a menu the window still owns, so
/// there is deliberately no destructor for this single-window app.
struct Menus {
    bar: HMenu,
    view: HMenu,
    filters: HMenu,
}

/// Whether the attached window currently shows the menu bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuVisibility {
    Visible,
    Hidden,
}

/// The window a menu bar is attached to, and whether the bar is shown.
#[derive(Debug, Clone, Copy)]
struct Attachment {
    window: HWND,
    visibility: MenuVisibility,
}

impl Attachment {
    /// Assigns or removes the bar. The attachment is only updated on success.
    fn set_visibility(&mut self, menus: &Menus, visibility: MenuVisibility) -> bool {
        let menu = match visibility {
            MenuVisibility::Visible => Some(menus.bar),
            MenuVisibility::Hidden => None,
        };
        if !set_window_menu(self.window, menu) {
            return false;
        }
        // SAFETY: `self.window` belongs to the live winit window.
        unsafe {
            DrawMenuBar(self.window);
        }
        self.visibility = visibility;
        true
    }
}

/// The native menu bar attached to a window.
pub(crate) struct MenuBar {
    /// `None` when building the menu failed; every operation then does nothing.
    menus: Option<Menus>,
    /// `None` until the menu bar is successfully assigned to a window.
    attachment: Cell<Option<Attachment>>,
    proxy: EventLoopProxy<UserEvent>,
}

impl MenuBar {
    pub(crate) fn new(
        mode: DisplayMode,
        current_filter: ScaleFilter,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        Self {
            menus: build_menus(mode, current_filter),
            attachment: Cell::new(None),
            proxy,
        }
    }

    /// Attaches the menu bar to the window and starts receiving `WM_COMMAND`.
    pub(crate) fn attach(&self, window: &Window) {
        let Some(menus) = &self.menus else {
            return;
        };
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };

        let mut attachment = Attachment {
            window: hwnd,
            visibility: MenuVisibility::Hidden,
        };
        if !attachment.set_visibility(menus, MenuVisibility::Visible) {
            debug_log::log(format_args!(
                "native menu: failed to assign the menu bar to the window"
            ));
            return;
        }
        self.attachment.set(Some(attachment));

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
        let Some(menus) = &self.menus else {
            return;
        };
        for display_mode in [DisplayMode::Single, DisplayMode::Spread, DisplayMode::Mixed] {
            check(
                menus.view,
                MenuCommand::Action(action_for_mode(display_mode)),
                display_mode == state.mode,
            );
        }
        for filter in ScaleFilter::ALL {
            check(
                menus.filters,
                MenuCommand::SetScaleFilter(filter),
                filter == state.filter,
            );
        }
        check(
            menus.view,
            MenuCommand::Action(Action::ToggleFullscreen),
            state.fullscreen,
        );
    }

    /// Windows keeps the menu bar visible in borderless fullscreen, so detach
    /// it while fullscreen and restore the same `HMENU`, including check states.
    pub(crate) fn set_fullscreen(&self, fullscreen: bool) {
        let Some(menus) = &self.menus else {
            return;
        };
        let Some(mut attachment) = self.attachment.get() else {
            return;
        };
        let visibility = if fullscreen {
            MenuVisibility::Hidden
        } else {
            MenuVisibility::Visible
        };
        if attachment.visibility == visibility {
            return;
        }
        if !attachment.set_visibility(menus, visibility) {
            debug_log::log(format_args!(
                "native menu: failed to {} the menu bar for fullscreen",
                if fullscreen { "hide" } else { "restore" }
            ));
            return;
        }
        self.attachment.set(Some(attachment));
    }
}

fn action_for_mode(mode: DisplayMode) -> Action {
    match mode {
        DisplayMode::Single => Action::SinglePage,
        DisplayMode::Spread => Action::SpreadPage,
        DisplayMode::Mixed => Action::MixedPage,
    }
}

/// Builds the menu tree once. On failure the bar is left inert, and the
/// partially created handles are abandoned until the process exits.
fn build_menus(mode: DisplayMode, current_filter: ScaleFilter) -> Option<Menus> {
    let bar = create_menu_bar()?;
    let app = create_popup("MangaNia")?;
    let file = create_popup("ファイル")?;
    let navigation = create_popup("移動")?;
    let view = create_popup("表示")?;
    let filters = create_popup("縮小フィルタ")?;

    let mut built = true;

    // The application menu keeps the existing quit entry on Windows.
    built &= append_item(app, MenuCommand::ExitApplication, "終了");

    built &= append_item(
        file,
        MenuCommand::Action(Action::SavePage),
        "表示中のページを保存",
    );
    built &= append_separator(file);
    built &= append_item(file, MenuCommand::ClearDiskCache, "キャッシュを削除");

    built &= append_item(navigation, MenuCommand::Action(Action::Next), "次のページ");
    built &= append_item(navigation, MenuCommand::Action(Action::Prev), "前のページ");
    built &= append_item(navigation, MenuCommand::Action(Action::First), "先頭へ");
    built &= append_item(navigation, MenuCommand::Action(Action::Last), "末尾へ");

    for display_mode in [DisplayMode::Single, DisplayMode::Spread, DisplayMode::Mixed] {
        let checked = display_mode == mode;
        built &= append_item_checked(
            view,
            MenuCommand::Action(action_for_mode(display_mode)),
            display_mode.label(),
            checked,
        );
    }
    built &= append_separator(view);
    built &= append_item(
        view,
        MenuCommand::Action(Action::ToggleFullscreen),
        "全画面表示",
    );

    for filter in ScaleFilter::ALL {
        built &= append_item_checked(
            filters,
            MenuCommand::SetScaleFilter(filter),
            filter.label(),
            filter == current_filter,
        );
    }
    built &= append_submenu(view, filters, "縮小フィルタ");

    built &= append_submenu(bar, app, "MangaNia");
    built &= append_submenu(bar, file, "ファイル");
    built &= append_submenu(bar, navigation, "移動");
    built &= append_submenu(bar, view, "表示");

    built.then_some(Menus { bar, view, filters })
}

fn create_menu_bar() -> Option<HMenu> {
    // SAFETY: `CreateMenu` has no preconditions.
    let menu = HMenu::from_raw(unsafe { CreateMenu() });
    if menu.is_none() {
        debug_log::log(format_args!("native menu: failed to create the menu bar"));
    }
    menu
}

fn create_popup(label: &str) -> Option<HMenu> {
    // SAFETY: `CreatePopupMenu` has no preconditions.
    let menu = HMenu::from_raw(unsafe { CreatePopupMenu() });
    if menu.is_none() {
        debug_log::log(format_args!(
            "native menu: failed to create the {label} menu"
        ));
    }
    menu
}

/// Assigns `menu` to `window`, or removes the current menu when `menu` is
/// `None`. Returns `false` when Win32 rejects the change.
fn set_window_menu(window: HWND, menu: Option<HMenu>) -> bool {
    let handle = menu.map_or(0, HMenu::raw);
    // SAFETY: `window` belongs to the live winit window. The menu is owned for
    // the process lifetime and is never destroyed here.
    unsafe { SetMenu(window, handle) != 0 }
}

fn append_item(menu: HMenu, command: MenuCommand, label: &str) -> bool {
    append_item_checked(menu, command, label, false)
}

fn append_item_checked(menu: HMenu, command: MenuCommand, label: &str, checked: bool) -> bool {
    let text = wide(label);
    let flags = item_flags(checked);
    // SAFETY: `menu` is a live menu handle and `text` is null-terminated for
    // the duration of the call.
    let appended = unsafe { AppendMenuW(menu.raw(), flags, command.id() as usize, text.as_ptr()) };
    if appended == 0 {
        debug_log::log(format_args!(
            "native menu: failed to append \"{label}\" (id {})",
            command.id()
        ));
    }
    appended != 0
}

fn append_separator(menu: HMenu) -> bool {
    // SAFETY: `menu` is a live menu handle.
    let appended = unsafe { AppendMenuW(menu.raw(), MF_SEPARATOR, 0, ptr::null()) };
    if appended == 0 {
        debug_log::log(format_args!("native menu: failed to append a separator"));
    }
    appended != 0
}

fn append_submenu(menu: HMenu, submenu: HMenu, label: &str) -> bool {
    let text = wide(label);
    // SAFETY: `menu` and `submenu` are live menu handles and `text` is
    // null-terminated for the duration of the call.
    let appended = unsafe {
        AppendMenuW(
            menu.raw(),
            MF_POPUP | MF_STRING,
            submenu.raw() as usize,
            text.as_ptr(),
        )
    };
    if appended == 0 {
        debug_log::log(format_args!(
            "native menu: failed to append the \"{label}\" submenu"
        ));
    }
    appended != 0
}

fn check(menu: HMenu, command: MenuCommand, checked: bool) {
    // SAFETY: `menu` is a live menu handle.
    unsafe {
        CheckMenuItem(
            menu.raw(),
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
        WM_COMMAND => {
            // A menu command has a zero high word and a null `lparam`. A
            // control notification can also have a zero high word, but it
            // sets `lparam` to the control's handle.
            if (wparam >> 16) == 0
                && lparam == 0
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
        WM_NCDESTROY => {
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
