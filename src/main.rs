#![cfg_attr(windows, windows_subsystem = "windows")]

use std::error::Error;
use std::path::PathBuf;

use mangania::app;
use mangania::cli::{self, Mode};
use mangania::disk_cache::DiskCache;
use mangania::headless;

const USAGE: &str = "\
usage: mangania [--prefetch N] [--list | --check | --render-page PAGE OUTPUT] [ARCHIVE]

  --list             print indexed pages and exit
  --check            decode every page and report errors
  --render-page N O  write page N (1-based) to image file O
  --prefetch N       number of pages to prefetch ahead (overrides config.toml)
  --help             show this help";

fn main() {
    #[cfg(windows)]
    if std::env::args_os().len() > 1 {
        attach_parent_console();
    }

    let args = match cli::parse(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("mangania: {error}");
            std::process::exit(2);
        }
    };

    if let Err(error) = run(args) {
        eprintln!("mangania: {error}");
        std::process::exit(1);
    }
}

fn run(args: cli::Args) -> Result<(), Box<dyn Error>> {
    match args.mode {
        Mode::Help => {
            println!("{USAGE}");
            Ok(())
        }
        Mode::Gui => app::run(args.path, args.prefetch),
        Mode::List => headless::list(&required_path(args.path)?),
        Mode::Check => {
            let cache = DiskCache::default();
            headless::check(&required_path(args.path)?, &cache)
        }
        Mode::Render { page, output } => {
            let cache = DiskCache::default();
            headless::render_page(&required_path(args.path)?, page, &output, &cache)
        }
    }
}

fn required_path(path: Option<PathBuf>) -> Result<PathBuf, Box<dyn Error>> {
    path.ok_or_else(|| std::io::Error::other("ARCHIVE path is required").into())
}

/// Reconnect stdout/stderr/stdin to the console that launched this GUI-subsystem
/// executable, so CLI modes still work from cmd.exe / PowerShell.
#[cfg(windows)]
fn attach_parent_console() {
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
