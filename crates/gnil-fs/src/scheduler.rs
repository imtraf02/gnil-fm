use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
    },
    thread,
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use gnil_core::{JobEvent, JobId, JobPriority, JobProgress, JobState};

type TaskFn = Box<dyn FnOnce(JobContext) -> Result<(), String> + Send + 'static>;

struct QueuedTask {
    id: JobId,
    priority: JobPriority,
    sequence: u64,
    cancelled: Arc<AtomicBool>,
    events: Sender<JobEvent>,
    run: TaskFn,
}

impl PartialEq for QueuedTask {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.sequence == other.sequence
    }
}
impl Eq for QueuedTask {}
impl PartialOrd for QueuedTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueuedTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

struct QueueState {
    tasks: BinaryHeap<QueuedTask>,
    shutdown: bool,
}

struct SchedulerInner {
    queue: Mutex<QueueState>,
    available: Condvar,
    sequence: AtomicU64,
}

pub struct TaskScheduler {
    inner: Arc<SchedulerInner>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl TaskScheduler {
    #[must_use]
    pub fn new(worker_count: usize) -> Self {
        let inner = Arc::new(SchedulerInner {
            queue: Mutex::new(QueueState {
                tasks: BinaryHeap::new(),
                shutdown: false,
            }),
            available: Condvar::new(),
            sequence: AtomicU64::new(0),
        });
        let workers = (0..worker_count.max(1))
            .map(|index| {
                let inner = Arc::clone(&inner);
                thread::Builder::new()
                    .name(format!("gnil-worker-{index}"))
                    .spawn(move || worker_loop(&inner))
                    .expect("worker thread can start")
            })
            .collect();
        Self { inner, workers }
    }

    pub fn submit<F>(&self, priority: JobPriority, run: F) -> JobHandle
    where
        F: FnOnce(JobContext) -> Result<(), String> + Send + 'static,
    {
        let id = JobId::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let (events, receiver) = unbounded();
        let sequence = self.inner.sequence.fetch_add(1, AtomicOrdering::Relaxed);
        let task = QueuedTask {
            id,
            priority,
            sequence,
            cancelled: Arc::clone(&cancelled),
            events: events.clone(),
            run: Box::new(run),
        };
        let _ = events.send(JobEvent::State {
            id,
            state: JobState::Queued,
        });
        self.inner
            .queue
            .lock()
            .expect("queue lock poisoned")
            .tasks
            .push(task);
        self.inner.available.notify_one();
        JobHandle {
            id,
            cancelled,
            events: receiver,
        }
    }
}

impl Drop for TaskScheduler {
    fn drop(&mut self) {
        {
            let mut state = self.inner.queue.lock().expect("queue lock poisoned");
            state.shutdown = true;
        }
        self.inner.available.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_loop(inner: &SchedulerInner) {
    loop {
        let task = {
            let mut state = inner.queue.lock().expect("queue lock poisoned");
            while state.tasks.is_empty() && !state.shutdown {
                state = inner.available.wait(state).expect("queue lock poisoned");
            }
            if state.shutdown {
                return;
            }
            state.tasks.pop()
        };
        let Some(task) = task else { continue };
        if task.cancelled.load(AtomicOrdering::Relaxed) {
            let _ = task.events.send(JobEvent::State {
                id: task.id,
                state: JobState::Cancelled,
            });
            continue;
        }
        let _ = task.events.send(JobEvent::State {
            id: task.id,
            state: JobState::Running,
        });
        let context = JobContext {
            id: task.id,
            cancelled: Arc::clone(&task.cancelled),
            events: task.events.clone(),
        };
        let result = (task.run)(context);
        let state = if task.cancelled.load(AtomicOrdering::Relaxed) {
            JobState::Cancelled
        } else if result.is_ok() {
            JobState::Completed
        } else {
            if let Err(message) = result {
                let _ = task.events.send(JobEvent::Message {
                    id: task.id,
                    message,
                });
            }
            JobState::Failed
        };
        let _ = task.events.send(JobEvent::State { id: task.id, state });
    }
}

pub struct JobHandle {
    pub id: JobId,
    cancelled: Arc<AtomicBool>,
    pub events: Receiver<JobEvent>,
}

impl JobHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, AtomicOrdering::Relaxed);
    }
}

#[derive(Clone)]
pub struct JobContext {
    id: JobId,
    cancelled: Arc<AtomicBool>,
    events: Sender<JobEvent>,
}

impl JobContext {
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(AtomicOrdering::Relaxed)
    }

    pub fn progress(&self, progress: JobProgress) {
        let _ = self.events.send(JobEvent::Progress {
            id: self.id,
            progress,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn scheduler_runs_and_reports_completion() {
        let scheduler = TaskScheduler::new(1);
        let handle = scheduler.submit(JobPriority::Foreground, |_context| Ok(()));
        let events: Vec<_> = handle.events.iter().take(3).collect();
        assert!(events.iter().any(|event| matches!(
            event,
            JobEvent::State {
                state: JobState::Completed,
                ..
            }
        )));
    }

    #[test]
    fn queued_job_can_be_cancelled() {
        let scheduler = TaskScheduler::new(1);
        let blocker = scheduler.submit(JobPriority::Foreground, |_context| {
            thread::sleep(Duration::from_millis(30));
            Ok(())
        });
        let cancelled = scheduler.submit(JobPriority::Background, |_context| Ok(()));
        cancelled.cancel();
        let _ = blocker.events.iter().take(3).collect::<Vec<_>>();
        let events = cancelled.events.iter().take(2).collect::<Vec<_>>();
        assert!(events.iter().any(|event| matches!(
            event,
            JobEvent::State {
                state: JobState::Cancelled,
                ..
            }
        )));
    }
}
