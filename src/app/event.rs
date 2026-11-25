pub(crate) enum UserEvent {
    ImageReady,
    Opened {
        generation: u64,
        result: Result<crate::archive::open::Entry, String>,
    },
    Saved {
        generation: u64,
        result: Result<std::path::PathBuf, String>,
    },
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    Menu(crate::platform::MenuCommand),
}
