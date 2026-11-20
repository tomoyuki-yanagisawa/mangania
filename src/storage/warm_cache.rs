//! Background extraction of ZIP pages into the disk cache.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crate::archive::Archive;
use crate::debug_log;
use crate::storage::disk_cache::DiskCache;

/// Warms the disk cache in the background. The worker owns a second archive
/// handle, so page views do not wait for the whole archive to be extracted.
pub(crate) struct CacheWarmer {
    stop: Arc<AtomicBool>,
}

impl CacheWarmer {
    /// Starts warming `archive`. Directories are already read in place, so
    /// they need no cache and return `None`.
    pub(crate) fn start(archive: &Archive) -> Option<Self> {
        if archive.directory_root().is_some() {
            return None;
        }
        let mut archive = match archive.reopen() {
            Ok(archive) => archive,
            Err(error) => {
                debug_log::log(format_args!("warm cache: reopen failed: {error}"));
                return None;
            }
        };
        let pages = archive.pages.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let spawned = thread::Builder::new()
            .name("mangania-warm-cache".to_string())
            .spawn(move || {
                let cache = DiskCache::default();
                let total = pages.len();
                let mut failures = 0;
                debug_log::log(format_args!("warm cache: start pages={total}"));
                for page in pages {
                    if worker_stop.load(Ordering::Relaxed) {
                        debug_log::log(format_args!("warm cache: stopped"));
                        return;
                    }
                    match cache.source(&mut archive, page.entry, &page.name) {
                        Ok(_) => {
                            failures = 0;
                        }
                        Err(error) => {
                            failures += 1;
                            debug_log::log(format_args!("warm cache: page={} {error}", page.entry));
                            if failures >= 3 {
                                debug_log::log(format_args!(
                                    "warm cache: stopping after {failures} failures"
                                ));
                                return;
                            }
                        }
                    }
                }
                debug_log::log(format_args!("warm cache: done pages={total}"));
            });
        if let Err(error) = spawned {
            debug_log::log(format_args!("warm cache: spawn failed: {error}"));
            return None;
        }
        Some(Self { stop })
    }

    /// Stops after the current page finishes.
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for CacheWarmer {
    fn drop(&mut self) {
        self.stop();
    }
}
