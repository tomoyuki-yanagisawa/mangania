use std::path::{Path, PathBuf};

use crate::archive::open::Entry;
use crate::debug_log;
use crate::event::UserEvent;
use crate::export;
use crate::render::prefetch::Prefetch;
use crate::render::viewer::Viewer;
use crate::storage::disk_cache::DiskCache;

use super::App;

impl App {
    pub(super) fn open(&mut self, path: &Path) {
        debug_log::log(format_args!("open requested: {}", path.display()));
        self.open_generation = self.open_generation.wrapping_add(1);
        let generation = self.open_generation;
        let path = path.to_owned();
        let proxy = self.proxy.clone();
        self.status = Some(format!("opening {}...", path.display()));
        self.update_title();
        std::thread::spawn(move || {
            let result = Entry::open(&path);
            let _ = proxy.send_event(UserEvent::Opened { generation, result });
        });
    }

    pub(super) fn finish_open(&mut self, generation: u64, result: Result<Entry, String>) {
        if generation != self.open_generation {
            return;
        }
        self.status = None;
        match result {
            Ok(entry) => {
                let Entry {
                    path: resolved_path,
                    mut archive,
                } = entry;
                debug_log::log(format_args!(
                    "archive ready: {} page(s)",
                    archive.pages.len()
                ));
                if archive.pages.is_empty() {
                    debug_log::log(format_args!("archive has no supported images"));
                    self.error = Some("no supported images in archive".to_string());
                    self.update_title();
                    return;
                }

                let pages = std::mem::take(&mut archive.pages);
                let name = resolved_path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| resolved_path.display().to_string());
                let proxy = self.proxy.clone();
                let cache = DiskCache::default();
                let prefetch = Prefetch::start(archive, cache, move || {
                    proxy.send_event(UserEvent::ImageReady).is_ok()
                });
                self.viewer = Some(Viewer::new(
                    name,
                    resolved_path,
                    pages,
                    prefetch,
                    self.depth,
                    self.cache_limits,
                    self.viewport(),
                ));
                self.viewer_generation = self.viewer_generation.wrapping_add(1);
                self.error = None;
                self.status = None;
                self.scroll = 0.0;
                self.last_frame.clear();
                self.refresh();
            }
            Err(error) => {
                eprintln!("mangania: {error}");
                debug_log::log(format_args!("open failed: {error}"));
                self.error = Some(error);
                self.update_title();
            }
        }
    }

    pub(super) fn save_current_page(&mut self) {
        if self.saving {
            return;
        }
        let Some(viewer) = &self.viewer else {
            return;
        };
        let path = viewer.path.clone();
        let page = viewer.current;
        let config = self.config.clone();
        let generation = self.viewer_generation;
        let proxy = self.proxy.clone();
        self.saving = true;
        self.status = Some("saving...".to_string());
        self.update_title();
        std::thread::spawn(move || {
            let cache = DiskCache::default();
            let result = export::save_current_page(&path, page, &config, &cache);
            let _ = proxy.send_event(UserEvent::Saved { generation, result });
        });
    }

    pub(super) fn finish_save(&mut self, generation: u64, result: Result<PathBuf, String>) {
        self.saving = false;
        if generation != self.viewer_generation {
            return;
        }
        match result {
            Ok(path) => {
                eprintln!("mangania: saved {}", path.display());
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default();
                self.status = Some(format!("saved {name}"));
                self.error = None;
            }
            Err(error) => {
                eprintln!("mangania: save: {error}");
                self.status = Some(format!("save error: {error}"));
            }
        }
        self.update_title();
        self.request_redraw();
    }
}
