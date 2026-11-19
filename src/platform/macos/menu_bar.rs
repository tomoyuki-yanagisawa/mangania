//! macOS native menu bar.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSMenu, NSMenuItem,
};
use objc2_foundation::{MainThreadMarker, NSObject, NSString, ns_string};

use winit::event_loop::{EventLoopBuilder, EventLoopProxy};
use winit::platform::macos::EventLoopBuilderExtMacOS;

use crate::config::{DisplayMode, ScaleFilter};
use crate::event::UserEvent;
use crate::input::bindings::Action;
use crate::menu::{NAVIGATION_ITEMS, labels};
use crate::platform::{MenuCommand, MenuState, action_for_mode};

/// Lets the app install its own system menu instead of winit's default menu.
pub(crate) fn disable_default_menu(
    mut builder: EventLoopBuilder<UserEvent>,
) -> EventLoopBuilder<UserEvent> {
    builder.with_default_menu(false);
    builder
}

struct MenuTargetIvars {
    proxy: EventLoopProxy<UserEvent>,
}

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = MenuTargetIvars]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(menuCommand:))]
        fn menu_command(&self, sender: &NSMenuItem) {
            let Some(command) = MenuCommand::from_id(sender.tag() as u16) else {
                return;
            };
            let _ = self.ivars().proxy.send_event(UserEvent::Menu(command));
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker, proxy: EventLoopProxy<UserEvent>) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(MenuTargetIvars { proxy });
        // SAFETY: `NSObject`'s `init` has no additional requirements.
        unsafe { msg_send![super(this), init] }
    }

    fn any_ref(&self) -> &AnyObject {
        self.as_ref()
    }
}

/// The application-wide menu bar.
pub(crate) struct MenuBar {
    menu: Retained<NSMenu>,
    _target: Retained<MenuTarget>,
    single: Retained<NSMenuItem>,
    spread: Retained<NSMenuItem>,
    mixed: Retained<NSMenuItem>,
    fullscreen: Retained<NSMenuItem>,
    allow_upscale: Retained<NSMenuItem>,
    filters: Vec<(ScaleFilter, Retained<NSMenuItem>)>,
}

impl MenuBar {
    pub(crate) fn new(
        mode: DisplayMode,
        current_filter: ScaleFilter,
        allow_upscale: bool,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        let mtm = MainThreadMarker::new().expect("menu must be created on the main thread");
        let target = MenuTarget::new(mtm, proxy);
        let menu = NSMenu::new(mtm);

        let app = NSMenu::new(mtm);
        let app_item = NSMenuItem::new(mtm);
        app_item.setSubmenu(Some(&app));
        let quit = item(mtm, &target, labels::QUIT, MenuCommand::ExitApplication);
        quit.setKeyEquivalent(ns_string!("q"));
        app.addItem(&quit);
        menu.addItem(&app_item);

        let file = submenu(mtm, &menu, labels::FILE);
        file.addItem(&item(
            mtm,
            &target,
            labels::SAVE_PAGE,
            MenuCommand::Action(Action::SavePage),
        ));
        file.addItem(&NSMenuItem::separatorItem(mtm));
        file.addItem(&item(
            mtm,
            &target,
            labels::CLEAR_CACHE,
            MenuCommand::ClearDiskCache,
        ));

        let navigation = submenu(mtm, &menu, labels::NAVIGATION);
        for (label, action) in NAVIGATION_ITEMS {
            navigation.addItem(&item(mtm, &target, label, MenuCommand::Action(action)));
        }

        let view = submenu(mtm, &menu, labels::VIEW);
        let single = check_item(mtm, &target, DisplayMode::Single, mode);
        let spread = check_item(mtm, &target, DisplayMode::Spread, mode);
        let mixed = check_item(mtm, &target, DisplayMode::Mixed, mode);
        view.addItem(&single);
        view.addItem(&spread);
        view.addItem(&mixed);
        view.addItem(&NSMenuItem::separatorItem(mtm));

        let filter_menu = submenu(mtm, &view, labels::FILTERS);
        let mut filters = Vec::with_capacity(ScaleFilter::ALL.len());
        for filter in ScaleFilter::ALL {
            let filter_item = item(
                mtm,
                &target,
                filter.label(),
                MenuCommand::SetScaleFilter(filter),
            );
            set_checked(&filter_item, filter == current_filter);
            filter_menu.addItem(&filter_item);
            filters.push((filter, filter_item));
        }
        let allow_upscale_item = check_item_for(
            mtm,
            &target,
            labels::ALLOW_UPSCALE,
            MenuCommand::ToggleAllowUpscale,
            allow_upscale,
        );
        view.addItem(&allow_upscale_item);
        view.addItem(&NSMenuItem::separatorItem(mtm));
        let fullscreen = check_item_for(
            mtm,
            &target,
            labels::FULLSCREEN,
            MenuCommand::Action(Action::ToggleFullscreen),
            false,
        );
        view.addItem(&fullscreen);

        Self {
            menu,
            _target: target,
            single,
            spread,
            mixed,
            fullscreen,
            allow_upscale: allow_upscale_item,
            filters,
        }
    }

    pub(crate) fn attach(&self, _window: &winit::window::Window) {
        let mtm = MainThreadMarker::new().expect("menu must be attached on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setMainMenu(Some(&self.menu));
    }

    pub(crate) fn sync_state(&self, state: MenuState) {
        set_checked(&self.single, state.mode == DisplayMode::Single);
        set_checked(&self.spread, state.mode == DisplayMode::Spread);
        set_checked(&self.mixed, state.mode == DisplayMode::Mixed);
        set_checked(&self.fullscreen, state.fullscreen);
        set_checked(&self.allow_upscale, state.allow_upscale);
        for (filter, item) in &self.filters {
            set_checked(item, *filter == state.filter);
        }
    }

    /// macOS shows and hides the system menu bar itself in fullscreen.
    pub(crate) fn set_fullscreen(&self, _fullscreen: bool) {}
}

fn submenu(mtm: MainThreadMarker, parent: &NSMenu, title: &str) -> Retained<NSMenu> {
    let title = NSString::from_str(title);
    let submenu = NSMenu::initWithTitle(mtm.alloc(), &title);
    let item = NSMenuItem::new(mtm);
    item.setTitle(&title);
    item.setSubmenu(Some(&submenu));
    parent.addItem(&item);
    submenu
}

fn item(
    mtm: MainThreadMarker,
    target: &MenuTarget,
    title: &str,
    command: MenuCommand,
) -> Retained<NSMenuItem> {
    let title = NSString::from_str(title);
    // SAFETY: the item is created on the main thread, the selector exists on
    // `MenuTarget`, and the target outlives every item.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &title,
            Some(sel!(menuCommand:)),
            ns_string!(""),
        )
    };
    item.setTag(command.id() as isize);
    // SAFETY: `target` is retained by `MenuBar` for the lifetime of the menu.
    unsafe { item.setTarget(Some(target.any_ref())) };
    item
}

fn check_item(
    mtm: MainThreadMarker,
    target: &MenuTarget,
    display_mode: DisplayMode,
    current: DisplayMode,
) -> Retained<NSMenuItem> {
    check_item_for(
        mtm,
        target,
        display_mode.label(),
        MenuCommand::Action(action_for_mode(display_mode)),
        display_mode == current,
    )
}

fn check_item_for(
    mtm: MainThreadMarker,
    target: &MenuTarget,
    title: &str,
    command: MenuCommand,
    checked: bool,
) -> Retained<NSMenuItem> {
    let item = item(mtm, target, title, command);
    set_checked(&item, checked);
    item
}

fn set_checked(item: &NSMenuItem, checked: bool) {
    item.setState(if checked {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}
