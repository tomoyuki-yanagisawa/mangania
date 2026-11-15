#[cfg(target_os = "windows")]
use std::cell::Cell;

use muda::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::bindings::Action;
use crate::config::ScaleFilter;
use crate::event::UserEvent;

pub struct MenuBar {
    pub save: MenuItem,
    pub next: MenuItem,
    pub prev: MenuItem,
    pub first: MenuItem,
    pub last: MenuItem,
    pub single: CheckMenuItem,
    pub spread: CheckMenuItem,
    pub fullscreen: CheckMenuItem,
    filters: Vec<(ScaleFilter, CheckMenuItem)>,
    menu: Menu,
    /// Menu detached from the window by [`MenuBar::set_native_menu_visible`].
    /// `0` means the menu is currently attached (or was never attached).
    #[cfg(target_os = "windows")]
    saved_hmenu: Cell<isize>,
}

impl MenuBar {
    pub fn new(
        spread: bool,
        current_filter: ScaleFilter,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        let menu = Menu::new();

        let app = Submenu::new("MangaNia", true);
        let _ = app.append(&PredefinedMenuItem::quit(None));
        let _ = menu.append(&app);

        let file = Submenu::new("ファイル", true);
        let save = MenuItem::new("表示中のページを保存", true, None);
        let _ = file.append(&save);
        let _ = menu.append(&file);

        let navigation = Submenu::new("移動", true);
        let next = MenuItem::new("次のページ", true, None);
        let prev = MenuItem::new("前のページ", true, None);
        let first = MenuItem::new("先頭へ", true, None);
        let last = MenuItem::new("末尾へ", true, None);
        let _ = navigation.append(&next);
        let _ = navigation.append(&prev);
        let _ = navigation.append(&first);
        let _ = navigation.append(&last);
        let _ = menu.append(&navigation);

        let view = Submenu::new("表示", true);
        let single = CheckMenuItem::new("1ページ表示", true, !spread, None);
        let spread_item = CheckMenuItem::new("見開き表示", true, spread, None);
        let _ = view.append(&single);
        let _ = view.append(&spread_item);
        let _ = view.append(&PredefinedMenuItem::separator());
        let fullscreen = CheckMenuItem::new("全画面表示", true, false, None);
        let _ = view.append(&fullscreen);

        let filter = Submenu::new("縮小フィルタ", true);
        let mut filters = Vec::with_capacity(ScaleFilter::ALL.len());
        for scale_filter in ScaleFilter::ALL {
            let item = CheckMenuItem::new(
                scale_filter.label(),
                true,
                scale_filter == current_filter,
                None,
            );
            let _ = filter.append(&item);
            filters.push((scale_filter, item));
        }
        let _ = view.append(&filter);
        let _ = menu.append(&view);

        muda::MenuEvent::set_event_handler(Some(move |event| {
            let _ = proxy.send_event(UserEvent::Menu(event));
        }));

        Self {
            save,
            next,
            prev,
            first,
            last,
            single,
            spread: spread_item,
            fullscreen,
            filters,
            menu,
            #[cfg(target_os = "windows")]
            saved_hmenu: Cell::new(0),
        }
    }

    pub fn action_for(&self, id: &MenuId) -> Option<Action> {
        if id == self.save.id() {
            Some(Action::SavePage)
        } else if id == self.next.id() {
            Some(Action::Next)
        } else if id == self.prev.id() {
            Some(Action::Prev)
        } else if id == self.first.id() {
            Some(Action::First)
        } else if id == self.last.id() {
            Some(Action::Last)
        } else if id == self.single.id() {
            Some(Action::SinglePage)
        } else if id == self.spread.id() {
            Some(Action::SpreadPage)
        } else if id == self.fullscreen.id() {
            Some(Action::ToggleFullscreen)
        } else {
            None
        }
    }

    pub fn filter_for(&self, id: &MenuId) -> Option<ScaleFilter> {
        self.filters
            .iter()
            .find(|(_, item)| item.id() == id)
            .map(|(filter, _)| *filter)
    }

    pub fn set_filter_checked(&self, current: ScaleFilter) {
        for (filter, item) in &self.filters {
            item.set_checked(*filter == current);
        }
    }

    pub fn attach(&self, _window: &Window) {
        #[cfg(target_os = "macos")]
        self.menu.init_for_nsapp();

        #[cfg(target_os = "windows")]
        if let Some(hwnd) = hwnd_of(_window) {
            // SAFETY: the HWND comes from the live winit window.
            unsafe {
                if let Err(error) = self.menu.init_for_hwnd(hwnd) {
                    eprintln!("mangania: create menu: {error}");
                }
            }
        }
    }

    /// Shows or hides the native menu bar attached to `window`.
    ///
    /// Win32 has no dedicated hide call, so the menu bar is detached with
    /// `SetMenu(hwnd, NULL)`. The `HMENU` returned by `GetMenu` is saved first
    /// and restored afterwards, which keeps the exact same menu (including the
    /// check state of the view items) across repeated fullscreen toggles.
    #[cfg(target_os = "windows")]
    pub fn set_native_menu_visible(&self, window: &Window, visible: bool) {
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
#[cfg(target_os = "windows")]
fn hwnd_of(window: &Window) -> Option<isize> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}
