/// Reconnect stdout/stderr/stdin to the console that launched this GUI-subsystem
/// executable, so CLI modes still work from cmd.exe / PowerShell.
pub fn attach_parent_console() {
    use std::ptr;

    use windows_sys::Win32::Foundation::{
        GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
        STD_OUTPUT_HANDLE, SetStdHandle,
    };

    fn usable(handle: HANDLE) -> bool {
        handle != 0 && handle != INVALID_HANDLE_VALUE
    }

    fn open_console(name: &str, access: u32) -> HANDLE {
        let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            CreateFileW(
                name.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                0,
                0,
            )
        }
    }

    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);

        let stdout = GetStdHandle(STD_OUTPUT_HANDLE);
        let stderr = GetStdHandle(STD_ERROR_HANDLE);
        if !usable(stdout) || !usable(stderr) {
            let console = open_console("CONOUT$", GENERIC_WRITE);
            if console != INVALID_HANDLE_VALUE {
                if !usable(stdout) {
                    SetStdHandle(STD_OUTPUT_HANDLE, console);
                }
                if !usable(stderr) {
                    SetStdHandle(STD_ERROR_HANDLE, console);
                }
            }
        }

        if !usable(GetStdHandle(STD_INPUT_HANDLE)) {
            let console = open_console("CONIN$", GENERIC_READ);
            if console != INVALID_HANDLE_VALUE {
                SetStdHandle(STD_INPUT_HANDLE, console);
            }
        }
    }
}
