use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Instant;

use crate::archive::{Archive, Page};
use crate::debug_log;
use crate::render::decode::decode_page;
use crate::render::image::{PreparedPage, prepare};
use crate::render::viewport::Viewport;
use crate::storage::disk_cache::DiskCache;

struct Job {
    page: usize,
    entry: usize,
    name: String,
    viewport: Viewport,
}

struct Queue {
    jobs: VecDeque<Job>,
    /// The worker owns the disk cache, so clearing it is a queued command
    /// instead of a concurrent call from the UI thread.
    clear_cache: bool,
    closed: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}

/// Why a page job failed.
#[derive(Debug)]
pub enum PageError {
    /// The disk cache could not be read or written, or a cache file was
    /// corrupt and has been discarded. Retrying once can succeed.
    Cache(String),
    /// The page cannot be decoded or fitted. Retrying will not help.
    Page(String),
}

impl std::fmt::Display for PageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cache(message) | Self::Page(message) => f.write_str(message),
        }
    }
}

/// A finished page job. `viewport` is the display target the image was
/// prepared for; results for another viewport are stale and must be dropped.
pub struct Completed {
    pub page: usize,
    pub viewport: Viewport,
    pub result: Result<PreparedPage, PageError>,
}

pub struct Prefetch {
    shared: Arc<Shared>,
    results: Receiver<Completed>,
    cleared: Receiver<Result<(), String>>,
    in_flight: HashSet<usize>,
}

impl Prefetch {
    /// Spawns one queue worker. Up to `batch_size` pages are extracted
    /// sequentially, then decoded and resized to the display target in
    /// parallel with scoped threads.
    pub fn start(
        mut archive: Archive,
        cache: DiskCache,
        batch_size: usize,
        notify: impl Fn() -> bool + Send + 'static,
    ) -> Self {
        let batch_size = batch_size.max(1);
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                jobs: VecDeque::new(),
                clear_cache: false,
                closed: false,
            }),
            ready: Condvar::new(),
        });
        let (sender, results) = mpsc::sync_channel(batch_size);
        let (cleared_sender, cleared) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        thread::spawn(move || {
            debug_log::log(format_args!("worker: started batch_size={batch_size}"));
            loop {
                let first = {
                    let mut queue = worker_shared.queue.lock().unwrap();
                    loop {
                        if queue.closed {
                            debug_log::log(format_args!("worker: closed"));
                            return;
                        }
                        if queue.clear_cache {
                            queue.clear_cache = false;
                            drop(queue);
                            let result = cache.clear();
                            match &result {
                                Ok(()) => {
                                    debug_log::log(format_args!("worker: disk cache cleared"))
                                }
                                Err(error) => debug_log::log(format_args!(
                                    "worker: disk cache clear failed: {error}"
                                )),
                            }
                            let _ = cleared_sender.send(result);
                            if !notify() {
                                debug_log::log(format_args!("worker: notify failed"));
                                return;
                            }
                            break None;
                        }
                        if let Some(job) = queue.jobs.pop_front() {
                            break Some(job);
                        }
                        queue = worker_shared.ready.wait(queue).unwrap();
                    }
                };
                let Some(first) = first else {
                    continue;
                };

                let mut jobs = vec![first];
                {
                    let mut queue = worker_shared.queue.lock().unwrap();
                    while jobs.len() < batch_size {
                        let Some(job) = queue.jobs.pop_front() else {
                            break;
                        };
                        jobs.push(job);
                    }
                }

                let mut extracted = Vec::with_capacity(jobs.len());
                for job in jobs {
                    let started = Instant::now();
                    debug_log::log(format_args!(
                        "job start: page={} entry={} batch={}",
                        job.page + 1,
                        job.entry,
                        extracted.len() + 1
                    ));
                    let page = Page {
                        name: job.name.clone(),
                        entry: job.entry,
                    };
                    let file = cache.page_path(&mut archive, &page);
                    extracted.push((job, file, started));
                }

                let mut keep_going = true;
                thread::scope(|scope| {
                    let mut handles = Vec::with_capacity(extracted.len());
                    for (job, file, started) in extracted {
                        let cache = &cache;
                        let page = job.page;
                        let viewport = job.viewport;
                        handles.push((
                            page,
                            viewport,
                            started,
                            scope.spawn(move || {
                                let result = match file {
                                    Ok(file) => {
                                        let cached = file.cached;
                                        match std::panic::catch_unwind(
                                            std::panic::AssertUnwindSafe(|| {
                                                let image = match decode_page(cache, &file) {
                                                    Ok(image) => image,
                                                    Err(error) if cached => {
                                                        // The broken cache file was removed,
                                                        // so a retry re-extracts it.
                                                        return Err(PageError::Cache(error));
                                                    }
                                                    Err(error) => {
                                                        return Err(PageError::Page(error));
                                                    }
                                                };
                                                debug_log::log(format_args!(
                                                    "job decode: page={} {}x{} in {:.1}ms",
                                                    page + 1,
                                                    image.width(),
                                                    image.height(),
                                                    started.elapsed().as_secs_f64() * 1000.0
                                                ));
                                                prepare(image, viewport).map_err(PageError::Page)
                                            }),
                                        ) {
                                            Ok(result) => result,
                                            Err(payload) => Err(PageError::Page(format!(
                                                "decode panicked: {}",
                                                panic_message(payload)
                                            ))),
                                        }
                                    }
                                    Err(error) => Err(PageError::Cache(error)),
                                };
                                Completed {
                                    page,
                                    viewport,
                                    result,
                                }
                            }),
                        ));
                    }

                    for (page, viewport, started, handle) in handles {
                        let completed = match handle.join() {
                            Ok(completed) => completed,
                            Err(payload) => Completed {
                                page,
                                viewport,
                                result: Err(PageError::Page(format!(
                                    "decode panicked: {}",
                                    panic_message(payload)
                                ))),
                            },
                        };
                        if let Err(error) = &completed.result {
                            debug_log::log(format_args!("job failed: page={} {error}", page + 1));
                        }
                        if sender.send(completed).is_err() {
                            debug_log::log(format_args!("worker: result receiver dropped"));
                            keep_going = false;
                            break;
                        }
                        debug_log::log(format_args!(
                            "job done: page={} in {:.1}ms",
                            page + 1,
                            started.elapsed().as_secs_f64() * 1000.0
                        ));
                        if !notify() {
                            debug_log::log(format_args!("worker: notify failed"));
                            keep_going = false;
                            break;
                        }
                    }
                });

                if !keep_going {
                    return;
                }
            }
        });

        Self {
            shared,
            results,
            cleared,
            in_flight: HashSet::new(),
        }
    }

    /// Replaces the queued jobs with `jobs` in decode order. The caller puts
    /// visible pages first, followed by prefetch pages. Running jobs are kept;
    /// their results still arrive. Cached pages are simply not enqueued.
    pub fn set_jobs(&mut self, viewport: Viewport, jobs: Vec<(usize, usize, String)>) {
        let mut queue = self.shared.queue.lock().unwrap();
        if queue.closed {
            return;
        }
        // Queued jobs are only page metadata. Rebuilding them is cheap and
        // drops pages that are no longer near the current position.
        while let Some(job) = queue.jobs.pop_front() {
            self.in_flight.remove(&job.page);
        }
        for (page, entry, name) in jobs {
            if self.in_flight.contains(&page) {
                continue;
            }
            debug_log::log(format_args!("request: page={} entry={}", page + 1, entry));
            self.in_flight.insert(page);
            queue.jobs.push_back(Job {
                page,
                entry,
                name,
                viewport,
            });
        }
        drop(queue);
        self.shared.ready.notify_one();
    }

    /// Asks the worker to delete the disk cache. The result arrives through
    /// [`Prefetch::poll_cleared`].
    pub fn clear_cache(&mut self) {
        let mut queue = self.shared.queue.lock().unwrap();
        queue.clear_cache = true;
        drop(queue);
        self.shared.ready.notify_one();
    }

    pub fn poll(&mut self) -> Vec<Completed> {
        let mut finished = Vec::new();
        while let Ok(completed) = self.results.try_recv() {
            self.in_flight.remove(&completed.page);
            finished.push(completed);
        }
        finished
    }

    pub fn poll_cleared(&mut self) -> Vec<Result<(), String>> {
        let mut cleared = Vec::new();
        while let Ok(result) = self.cleared.try_recv() {
            cleared.push(result);
        }
        cleared
    }

    /// Drops queued jobs that have not started yet. Running jobs cannot be cancelled.
    pub fn clear_pending(&mut self) {
        let mut queue = self.shared.queue.lock().unwrap();
        while let Some(job) = queue.jobs.pop_front() {
            self.in_flight.remove(&job.page);
        }
    }
}

impl Drop for Prefetch {
    fn drop(&mut self) {
        debug_log::log(format_args!("worker: stopping"));
        self.shared.queue.lock().unwrap().closed = true;
        self.shared.ready.notify_all();
    }
}

#[cfg(test)]
impl Prefetch {
    /// A prefetch that accepts jobs but never runs them, for tests that do not
    /// exercise decoding.
    pub(crate) fn idle() -> Self {
        let (_sender, results) = mpsc::sync_channel(0);
        let (_cleared_sender, cleared) = mpsc::channel();
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue {
                    jobs: VecDeque::new(),
                    clear_cache: false,
                    closed: false,
                }),
                ready: Condvar::new(),
            }),
            results,
            cleared,
            in_flight: HashSet::new(),
        }
    }

    /// Pages currently queued, in worker order. For tests that inspect ordering.
    pub(crate) fn queued_pages(&self) -> Vec<usize> {
        self.shared
            .queue
            .lock()
            .unwrap()
            .jobs
            .iter()
            .map(|job| job.page)
            .collect()
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_string()
    }
}
