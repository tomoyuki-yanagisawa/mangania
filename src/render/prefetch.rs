use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Instant;

use image::RgbaImage;

use crate::archive::Archive;
use crate::debug_log;
use crate::storage::disk_cache::DiskCache;

struct Job {
    page: usize,
    entry: usize,
    name: String,
}

struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}

pub struct Prefetch {
    shared: Arc<Shared>,
    results: Receiver<(usize, Result<Arc<RgbaImage>, String>)>,
    in_flight: HashSet<usize>,
}

impl Prefetch {
    /// Takes ownership of the archive so ZIP reads and decodes never block the UI thread.
    pub fn start(
        archive: Archive,
        cache: DiskCache,
        notify: impl Fn() -> bool + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                jobs: VecDeque::new(),
                closed: false,
            }),
            ready: Condvar::new(),
        });
        let (sender, results) = mpsc::sync_channel(1);
        let worker_shared = Arc::clone(&shared);
        thread::spawn(move || {
            debug_log::log(format_args!("worker: started"));
            let mut archive = archive;
            let cache = cache;
            loop {
                let job = {
                    let mut queue = worker_shared.queue.lock().unwrap();
                    loop {
                        if queue.closed {
                            debug_log::log(format_args!("worker: closed"));
                            return;
                        }
                        if let Some(job) = queue.jobs.pop_front() {
                            break job;
                        }
                        queue = worker_shared.ready.wait(queue).unwrap();
                    }
                };

                let started = Instant::now();
                debug_log::log(format_args!(
                    "job start: page={} entry={}",
                    job.page + 1,
                    job.entry
                ));
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let image = cache.decode(&mut archive, job.entry, &job.name)?;
                    debug_log::log(format_args!(
                        "job decode: page={} {}x{} in {:.1}ms",
                        job.page + 1,
                        image.width(),
                        image.height(),
                        started.elapsed().as_secs_f64() * 1000.0
                    ));
                    Ok(Arc::new(image))
                }));

                let result = match result {
                    Ok(result) => result,
                    Err(payload) => {
                        let message = panic_message(payload);
                        debug_log::log(format_args!("job panic: page={} {message}", job.page + 1));
                        Err(format!("decode panicked: {message}"))
                    }
                };

                if let Err(error) = &result {
                    debug_log::log(format_args!("job failed: page={} {error}", job.page + 1));
                }
                if sender.send((job.page, result)).is_err() {
                    debug_log::log(format_args!("worker: result receiver dropped"));
                    return;
                }
                debug_log::log(format_args!(
                    "job done: page={} in {:.1}ms",
                    job.page + 1,
                    started.elapsed().as_secs_f64() * 1000.0
                ));
                if !notify() {
                    debug_log::log(format_args!("worker: notify failed"));
                    return;
                }
            }
        });

        Self {
            shared,
            results,
            in_flight: HashSet::new(),
        }
    }

    /// `urgent` jobs jump ahead of queued prefetches and discard obsolete ones.
    pub fn request(&mut self, page: usize, entry: usize, name: String, urgent: bool) {
        if urgent {
            self.request_batch(vec![(page, entry, name)]);
            return;
        }
        debug_log::log(format_args!("request: page={} entry={}", page + 1, entry));
        let mut queue = self.shared.queue.lock().unwrap();
        if queue.closed {
            return;
        }
        if !self.in_flight.insert(page) {
            return;
        }
        queue.jobs.push_back(Job { page, entry, name });
        drop(queue);
        self.shared.ready.notify_one();
    }

    /// Queues several urgent jobs at once. Queued (not yet running) jobs are
    /// discarded once, so the batch starts next without dropping its own
    /// members. The first item runs first.
    pub fn request_batch(&mut self, jobs: Vec<(usize, usize, String)>) {
        if jobs.is_empty() {
            return;
        }
        let mut queue = self.shared.queue.lock().unwrap();
        if queue.closed {
            return;
        }
        // Drop queued (not yet running) jobs so the current pages start next.
        while let Some(job) = queue.jobs.pop_front() {
            self.in_flight.remove(&job.page);
        }
        for (page, entry, name) in jobs.into_iter().rev() {
            if self.in_flight.contains(&page) {
                continue;
            }
            debug_log::log(format_args!(
                "request: page={} entry={} urgent=batch",
                page + 1,
                entry
            ));
            self.in_flight.insert(page);
            queue.jobs.push_front(Job { page, entry, name });
        }
        drop(queue);
        self.shared.ready.notify_one();
    }

    pub fn poll(&mut self) -> Vec<(usize, Result<Arc<RgbaImage>, String>)> {
        let mut finished = Vec::new();
        while let Ok((page, result)) = self.results.try_recv() {
            self.in_flight.remove(&page);
            finished.push((page, result));
        }
        finished
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
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue {
                    jobs: VecDeque::new(),
                    closed: false,
                }),
                ready: Condvar::new(),
            }),
            results,
            in_flight: HashSet::new(),
        }
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
