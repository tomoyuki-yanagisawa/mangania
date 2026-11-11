mod app;
mod archive;
mod decoder;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod menu;
mod prefetch;
mod view;

use std::path::PathBuf;

fn main() {
    let mut prefetch = 2usize;
    let mut path = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg.to_str() == Some("--prefetch") {
            prefetch = args
                .next()
                .and_then(|value| value.to_str().and_then(|value| value.parse().ok()))
                .unwrap_or(2);
        } else if path.is_none() {
            path = Some(PathBuf::from(arg));
        }
    }

    if let Err(error) = app::run(path, prefetch) {
        eprintln!("mangania: {error}");
        std::process::exit(1);
    }
}
