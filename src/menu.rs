use muda::{CheckMenuItem, Menu, PredefinedMenuItem, Submenu};
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::prefetch::UserEvent;

pub struct MenuBar {
    pub single: CheckMenuItem,
    pub spread: CheckMenuItem,
    pub fullscreen: CheckMenuItem,
    menu: Menu,
}

impl MenuBar {
    pub fn new(spread: bool, proxy: EventLoopProxy<UserEvent>) -> Self {
        let menu = Menu::new();

        let app = Submenu::new("MangaNia", true);
        let _ = app.append(&PredefinedMenuItem::quit(None));
        let _ = menu.append(&app);

        let view = Submenu::new("表示", true);
        let single = CheckMenuItem::new("1ページ表示", true, !spread, None);
        let spread_item = CheckMenuItem::new("見開き表示", true, spread, None);
        let _ = view.append(&single);
        let _ = view.append(&spread_item);
        let _ = view.append(&PredefinedMenuItem::separator());
        let fullscreen = CheckMenuItem::new("全画面表示", true, false, None);
        let _ = view.append(&fullscreen);
        let _ = menu.append(&view);

        muda::MenuEvent::set_event_handler(Some(move |event| {
            let _ = proxy.send_event(UserEvent::Menu(event));
        }));

        Self {
            single,
            spread: spread_item,
            fullscreen,
            menu,
        }
    }

    pub fn attach(&self, _window: &Window) {
        #[cfg(target_os = "macos")]
        self.menu.init_for_nsapp();

        #[cfg(target_os = "windows")]
        {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

            if let Ok(handle) = _window.window_handle() {
                if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                    // SAFETY: the HWND comes from the live winit window.
                    unsafe {
                        if let Err(error) = self.menu.init_for_hwnd(handle.hwnd.get()) {
                            eprintln!("mangania: create menu: {error}");
                        }
                    }
                }
            }
        }
    }
}
