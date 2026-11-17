use muda::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

#[cfg(target_os = "windows")]
use crate::platform::windows::menu::NativeMenu;

use crate::config::{DisplayMode, ScaleFilter};
use crate::event::UserEvent;
use crate::input::bindings::Action;

pub struct MenuBar {
    pub save: MenuItem,
    pub clear_cache: MenuItem,
    pub next: MenuItem,
    pub prev: MenuItem,
    pub first: MenuItem,
    pub last: MenuItem,
    pub single: CheckMenuItem,
    pub spread: CheckMenuItem,
    pub mixed: CheckMenuItem,
    pub fullscreen: CheckMenuItem,
    filters: Vec<(ScaleFilter, CheckMenuItem)>,
    menu: Menu,
    /// Native menu bar state, including the saved `HMENU` used to restore the
    /// menu after fullscreen.
    #[cfg(target_os = "windows")]
    native: NativeMenu,
}

impl MenuBar {
    pub fn new(
        mode: DisplayMode,
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
        let _ = file.append(&PredefinedMenuItem::separator());
        let clear_cache = MenuItem::new("キャッシュを削除", true, None);
        let _ = file.append(&clear_cache);
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
        let single = CheckMenuItem::new(
            DisplayMode::Single.label(),
            true,
            mode == DisplayMode::Single,
            None,
        );
        let spread_item = CheckMenuItem::new(
            DisplayMode::Spread.label(),
            true,
            mode == DisplayMode::Spread,
            None,
        );
        let mixed = CheckMenuItem::new(
            DisplayMode::Mixed.label(),
            true,
            mode == DisplayMode::Mixed,
            None,
        );
        let _ = view.append(&single);
        let _ = view.append(&spread_item);
        let _ = view.append(&mixed);
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
            clear_cache,
            next,
            prev,
            first,
            last,
            single,
            spread: spread_item,
            mixed,
            fullscreen,
            filters,
            menu,
            #[cfg(target_os = "windows")]
            native: NativeMenu::default(),
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
        } else if id == self.mixed.id() {
            Some(Action::MixedPage)
        } else if id == self.fullscreen.id() {
            Some(Action::ToggleFullscreen)
        } else {
            None
        }
    }

    pub fn is_clear_cache(&self, id: &MenuId) -> bool {
        id == self.clear_cache.id()
    }

    pub fn filter_for(&self, id: &MenuId) -> Option<ScaleFilter> {
        self.filters
            .iter()
            .find(|(_, item)| item.id() == id)
            .map(|(filter, _)| *filter)
    }

    pub fn set_mode_checked(&self, mode: DisplayMode) {
        self.single.set_checked(mode == DisplayMode::Single);
        self.spread.set_checked(mode == DisplayMode::Spread);
        self.mixed.set_checked(mode == DisplayMode::Mixed);
    }

    pub fn set_filter_checked(&self, current: ScaleFilter) {
        for (filter, item) in &self.filters {
            item.set_checked(*filter == current);
        }
    }

    pub fn attach(&self, window: &Window) {
        #[cfg(target_os = "macos")]
        {
            let _ = window;
            self.menu.init_for_nsapp();
        }

        #[cfg(target_os = "windows")]
        self.native.attach(&self.menu, window);
    }

    /// Shows or hides the native menu bar attached to `window`.
    ///
    /// The Win32-specific detach/restore handling lives in
    /// [`crate::platform::windows::menu::NativeMenu`].
    #[cfg(target_os = "windows")]
    pub fn set_native_menu_visible(&self, window: &Window, visible: bool) {
        self.native.set_visible(window, visible);
    }
}
