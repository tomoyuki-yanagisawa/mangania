#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) mod menu;
#[cfg(windows)]
pub(crate) mod windows;

#[cfg(target_os = "macos")]
pub(crate) use macos::menu::MenuBar;
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) use menu::{MenuCommand, MenuState};
#[cfg(windows)]
pub(crate) use windows::menu::MenuBar;

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
