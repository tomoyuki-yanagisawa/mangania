pub enum UserEvent {
    ImageReady,
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    Menu(muda::MenuEvent),
}
