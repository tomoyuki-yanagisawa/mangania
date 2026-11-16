pub enum UserEvent {
    ImageReady,
    Opened {
        generation: u64,
        result: Result<crate::open::Entry, String>,
    },
    Saved {
        generation: u64,
        result: Result<std::path::PathBuf, String>,
    },
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    Menu(muda::MenuEvent),
}
