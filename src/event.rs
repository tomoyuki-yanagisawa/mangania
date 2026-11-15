pub enum UserEvent {
    ImageReady,
    Opened(u64, Result<crate::open::Entry, String>),
    Saved(u64, Result<std::path::PathBuf, String>),
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    Menu(muda::MenuEvent),
}
