use winit::event_loop::{EventLoop, EventLoopBuilder};

use crate::event::UserEvent;

#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) mod menu;
#[cfg(windows)]
pub(crate) mod windows;

#[cfg(target_os = "macos")]
pub(crate) use macos::menu::MenuBar;
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) use menu::{MenuCommand, MenuState, action_for_mode};
#[cfg(windows)]
pub(crate) use windows::menu::MenuBar;

/// Creates the event loop builder with each platform's menu setup applied.
pub(crate) fn event_loop_builder() -> EventLoopBuilder<UserEvent> {
    let builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    let builder = macos::menu::disable_default_menu(builder);
    builder
}

#[cfg(not(windows))]
use std::path::{Path, PathBuf};

/// Resolves a Windows shortcut to the path it points to.
#[cfg(windows)]
pub(crate) use windows::shortcut::resolve as resolve_shortcut;

/// Windows shortcuts cannot be followed on this platform.
#[cfg(not(windows))]
pub(crate) fn resolve_shortcut(path: &Path) -> Result<PathBuf, String> {
    Err(format!(
        "{} is a Windows shortcut (.lnk); drop the target file instead",
        path.display()
    ))
}
