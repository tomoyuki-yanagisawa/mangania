use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use image::RgbaImage;
use winit::event_loop::EventLoopProxy;

use crate::archive::Archive;
use crate::decoder;

pub enum UserEvent {
    ImageReady,
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    Menu(muda::MenuEvent),
}

struct Job {
    page: usize,
    entry: usize,
}

struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

pub struct Prefetch {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    results: Receiver<(usize, Result<Arc<RgbaImage>, String>)>,
    in_flight: HashSet<usize>,
    worker: Option<JoinHandle<()>>,
}

impl Prefetch {
    /// Takes ownership of the archive so ZIP reads and decodes never block the UI thread.
    pub fn start(archive: Archive, proxy: EventLoopProxy<UserEvent>) -> Self {
        let queue = Arc::new((
            Mutex::new(Queue {
                jobs: VecDeque::new(),
                closed: false,
            }),
            Condvar::new(),
        ));
        let (sender, results) = mpsc::channel();
        let worker_queue = Arc::clone(&queue);
        let worker = thread::spawn(move || {
            let mut archive = archive;
            loop {
                let job = {
                    let (lock, ready) = &*worker_queue;
                    let mut queue = lock.lock().unwrap();
                    loop {
                        if queue.closed {
                            return;
                        }
                        if let Some(job) = queue.jobs.pop_front() {
                            break job;
                        }
                        queue = ready.wait(queue).unwrap();
                    }
                };

                let result = archive
                    .read_index(job.entry)
                    .and_then(|bytes| decoder::decode(&bytes))
                    .map(Arc::new);
                if sender.send((job.page, result)).is_err() {
                    return;
                }
                if proxy.send_event(UserEvent::ImageReady).is_err() {
                    return;
                }
            }
        });

        Self {
            queue,
            results,
            in_flight: HashSet::new(),
            worker: Some(worker),
        }
    }

    /// `urgent` jobs jump ahead of queued prefetches and discard obsolete ones.
    pub fn request(&mut self, page: usize, entry: usize, urgent: bool) {
        let (lock, ready) = &*self.queue;
        let mut queue = lock.lock().unwrap();
        if queue.closed {
            return;
        }

        if !urgent {
            if !self.in_flight.insert(page) {
                return;
            }
            queue.jobs.push_back(Job { page, entry });
        } else {
            // Drop queued (not yet running) jobs so the current page starts next.
            while let Some(job) = queue.jobs.pop_front() {
                self.in_flight.remove(&job.page);
            }
            if self.in_flight.contains(&page) {
                return;
            }
            self.in_flight.insert(page);
            queue.jobs.push_front(Job { page, entry });
        }
        drop(queue);
        ready.notify_one();
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
        let (lock, _) = &*self.queue;
        let mut queue = lock.lock().unwrap();
        while let Some(job) = queue.jobs.pop_front() {
            self.in_flight.remove(&job.page);
        }
    }
}

impl Drop for Prefetch {
    fn drop(&mut self) {
        {
            let (lock, ready) = &*self.queue;
            lock.lock().unwrap().closed = true;
            ready.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
