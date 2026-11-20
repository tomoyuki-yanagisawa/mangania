#![cfg_attr(windows, windows_subsystem = "windows")]

use std::error::Error;
use std::path::PathBuf;

use mangania::app;
use mangania::cli::{self, Mode};
use mangania::headless;
use mangania::storage::disk_cache::DiskCache;

const USAGE: &str = "\
usage: mangania [--prefetch N] [--list | --check | --render-page PAGE OUTPUT] [ARCHIVE]

  --list             print indexed pages and exit
  --check            decode every page and report errors
  --render-page N O  write page N (1-based) to image file O
  --prefetch N       prefetch depth (2 pages per step except single-page mode)
  --help             show this help";

fn main() {
    #[cfg(windows)]
    if std::env::args_os().len() > 1 {
        mangania::attach_parent_console();
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
