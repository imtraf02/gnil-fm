use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
    },
    thread,
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use gnil_core::{JobDecision, JobEvent, JobId, JobPriority, JobProgress, JobPrompt, JobState};

type TaskFn<R> = Box<dyn FnOnce(JobContext) -> JobResult<R> + Send + 'static>;

#[derive(Debug)]
pub enum JobResult<R> {
    Completed(R),
    CompletedWithIssues(R),
    Cancelled(R),
    Failed { value: Option<R>, message: String },
}

impl<R> JobResult<R> {
    #[must_use]
    pub fn state(&self) -> JobState {
        match self {
            Self::Completed(_) => JobState::Completed,
            Self::CompletedWithIssues(_) => JobState::CompletedWithIssues,
            Self::Cancelled(_) => JobState::Cancelled,
            Self::Failed { .. } => JobState::Failed,
        }
    }
}

struct QueuedTask<R> {
    id: JobId,
    priority: JobPriority,
    sequence: u64,
    control: Arc<JobControl>,
    events: Sender<JobEvent>,
    completion: Sender<JobResult<R>>,
    run: TaskFn<R>,
}

impl<R> PartialEq for QueuedTask<R> {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.sequence == other.sequence
    }
}

impl<R> Eq for QueuedTask<R> {}

impl<R> PartialOrd for QueuedTask<R> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<R> Ord for QueuedTask<R> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

struct QueueState<R> {
    tasks: BinaryHeap<QueuedTask<R>>,
    shutdown: bool,
}

struct SchedulerInner<R> {
    queue: Mutex<QueueState<R>>,
    controls: Mutex<Vec<Weak<JobControl>>>,
    available: Condvar,
    sequence: AtomicU64,
}

pub struct TaskScheduler<R: Send + Default + 'static> {
    inner: Arc<SchedulerInner<R>>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl<R: Send + Default + 'static> TaskScheduler<R> {
    #[must_use]
    pub fn new(worker_count: usize) -> Self {
        let inner = Arc::new(SchedulerInner {
            queue: Mutex::new(QueueState {
                tasks: BinaryHeap::new(),
                shutdown: false,
            }),
            controls: Mutex::new(Vec::new()),
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

    pub fn submit<F>(&self, priority: JobPriority, run: F) -> JobHandle<R>
    where
        F: FnOnce(JobContext) -> JobResult<R> + Send + 'static,
    {
        let id = JobId::new();
        let control = Arc::new(JobControl::default());
        self.inner
            .controls
            .lock()
            .expect("job controls lock poisoned")
            .push(Arc::downgrade(&control));
        let (events, receiver) = unbounded();
        let (completion, completed) = unbounded();
        let sequence = self.inner.sequence.fetch_add(1, AtomicOrdering::Relaxed);
        let task = QueuedTask {
            id,
            priority,
            sequence,
            control: Arc::clone(&control),
            events: events.clone(),
            completion,
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
            controller: JobController {
                id,
                control,
                events,
            },
            events: receiver,
            completion: completed,
        }
    }
}

impl<R: Send + Default + 'static> Drop for TaskScheduler<R> {
    fn drop(&mut self) {
        for control in self
            .inner
            .controls
            .lock()
            .expect("job controls lock poisoned")
            .iter()
            .filter_map(Weak::upgrade)
        {
            control.cancelled.store(true, AtomicOrdering::Relaxed);
            control.changed.notify_all();
        }
        {
            let mut state = self.inner.queue.lock().expect("queue lock poisoned");
            state.shutdown = true;
            for task in &state.tasks {
                task.control.cancelled.store(true, AtomicOrdering::Relaxed);
                task.control.changed.notify_all();
            }
        }
        self.inner.available.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_loop<R: Send + Default + 'static>(inner: &SchedulerInner<R>) {
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
        if task.control.cancelled.load(AtomicOrdering::Relaxed) {
            let _ = task.events.send(JobEvent::State {
                id: task.id,
                state: JobState::Cancelled,
            });
            let _ = task.completion.send(JobResult::Cancelled(R::default()));
            continue;
        }
        let initially_paused = {
            let mut state = task
                .control
                .state
                .lock()
                .expect("job control lock poisoned");
            state.started = true;
            state.paused
        };
        let _ = task.events.send(JobEvent::State {
            id: task.id,
            state: if initially_paused {
                JobState::Paused
            } else {
                JobState::Running
            },
        });
        let context = JobContext {
            id: task.id,
            control: Arc::clone(&task.control),
            events: task.events.clone(),
        };
        let result = (task.run)(context);
        let state = result.state();
        if let JobResult::Failed { message, .. } = &result {
            let _ = task.events.send(JobEvent::Message {
                id: task.id,
                message: message.clone(),
            });
        }
        let _ = task.events.send(JobEvent::State { id: task.id, state });
        let _ = task.completion.send(result);
    }
}

pub struct JobHandle<R> {
    pub id: JobId,
    pub controller: JobController,
    pub events: Receiver<JobEvent>,
    pub completion: Receiver<JobResult<R>>,
}

#[derive(Clone)]
pub struct JobController {
    id: JobId,
    control: Arc<JobControl>,
    events: Sender<JobEvent>,
}

impl JobController {
    pub fn pause(&self) {
        let mut state = self
            .control
            .state
            .lock()
            .expect("job control lock poisoned");
        if !self.control.cancelled.load(AtomicOrdering::Relaxed) {
            state.paused = true;
            let _ = self.events.send(JobEvent::State {
                id: self.id,
                state: JobState::Paused,
            });
        }
    }

    pub fn resume(&self) {
        let mut state = self
            .control
            .state
            .lock()
            .expect("job control lock poisoned");
        if state.paused {
            state.paused = false;
            self.control.changed.notify_all();
            let _ = self.events.send(JobEvent::State {
                id: self.id,
                state: if state.started {
                    JobState::Running
                } else {
                    JobState::Queued
                },
            });
        }
    }

    pub fn cancel(&self) {
        self.control.cancelled.store(true, AtomicOrdering::Relaxed);
        self.control.changed.notify_all();
    }

    pub fn respond(&self, decision: JobDecision) {
        let mut state = self
            .control
            .state
            .lock()
            .expect("job control lock poisoned");
        state.decision = Some(decision);
        self.control.changed.notify_all();
    }
}

#[derive(Default)]
struct ControlState {
    paused: bool,
    started: bool,
    decision: Option<JobDecision>,
}

#[derive(Default)]
struct JobControl {
    cancelled: AtomicBool,
    state: Mutex<ControlState>,
    changed: Condvar,
}

#[derive(Clone)]
pub struct JobContext {
    id: JobId,
    control: Arc<JobControl>,
    events: Sender<JobEvent>,
}

impl JobContext {
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.control.cancelled.load(AtomicOrdering::Relaxed)
    }

    #[must_use]
    pub fn cancelled_flag(&self) -> &AtomicBool {
        &self.control.cancelled
    }

    #[must_use]
    pub fn checkpoint(&self) -> bool {
        let mut state = self
            .control
            .state
            .lock()
            .expect("job control lock poisoned");
        while state.paused && !self.is_cancelled() {
            state = self
                .control
                .changed
                .wait(state)
                .expect("job control lock poisoned");
        }
        !self.is_cancelled()
    }

    pub fn state(&self, state: JobState) {
        let paused = self
            .control
            .state
            .lock()
            .expect("job control lock poisoned")
            .paused;
        let state = if paused && matches!(state, JobState::Preparing | JobState::Running) {
            JobState::Paused
        } else {
            state
        };
        let _ = self.events.send(JobEvent::State { id: self.id, state });
    }

    pub fn progress(&self, progress: JobProgress) {
        let _ = self.events.send(JobEvent::Progress {
            id: self.id,
            progress,
        });
    }

    #[must_use]
    pub fn request(&self, prompt: JobPrompt) -> JobDecision {
        let waiting_state = match prompt {
            JobPrompt::Conflict { .. } => JobState::WaitingForConflict,
            JobPrompt::IoError { .. } => JobState::WaitingForError,
        };
        self.state(waiting_state);
        let _ = self.events.send(JobEvent::Prompt {
            id: self.id,
            prompt,
        });
        let mut state = self
            .control
            .state
            .lock()
            .expect("job control lock poisoned");
        loop {
            if self.is_cancelled() {
                return JobDecision::Cancel;
            }
            if let Some(decision) = state.decision.take() {
                drop(state);
                self.state(JobState::Running);
                return decision;
            }
            state = self
                .control
                .changed
                .wait(state)
                .expect("job control lock poisoned");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn scheduler_runs_and_reports_completion() {
        let scheduler = TaskScheduler::new(1);
        let handle = scheduler.submit(JobPriority::Foreground, |_context| {
            JobResult::Completed(7_u8)
        });
        assert!(matches!(
            handle.completion.recv().unwrap(),
            JobResult::Completed(7)
        ));
        let events: Vec<_> = handle.events.try_iter().collect();
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
            JobResult::Completed(())
        });
        let cancelled =
            scheduler.submit(JobPriority::Background, |_context| JobResult::Completed(()));
        cancelled.controller.cancel();
        let _ = blocker.completion.recv().unwrap();
        thread::sleep(Duration::from_millis(5));
        let events = cancelled.events.try_iter().collect::<Vec<_>>();
        assert!(events.iter().any(|event| matches!(
            event,
            JobEvent::State {
                state: JobState::Cancelled,
                ..
            }
        )));
    }

    #[test]
    fn pause_and_prompt_are_cooperative() {
        let scheduler = TaskScheduler::new(1);
        let handle = scheduler.submit(JobPriority::Foreground, |context| {
            assert!(context.checkpoint());
            let decision = context.request(JobPrompt::IoError {
                path: "/tmp/example".into(),
                message: "retry me".into(),
                can_skip: true,
            });
            assert_eq!(decision, JobDecision::Retry);
            JobResult::Completed(())
        });
        loop {
            if matches!(handle.events.recv().unwrap(), JobEvent::Prompt { .. }) {
                break;
            }
        }
        handle.controller.respond(JobDecision::Retry);
        assert!(matches!(
            handle.completion.recv().unwrap(),
            JobResult::Completed(())
        ));
    }

    #[test]
    fn one_worker_runs_jobs_sequentially() {
        let scheduler = TaskScheduler::new(1);
        let (release_first, wait_first) = crossbeam_channel::bounded::<()>(0);
        let (started_second, second_started) = crossbeam_channel::bounded::<()>(1);
        let first = scheduler.submit(JobPriority::Foreground, move |_context| {
            wait_first.recv().unwrap();
            JobResult::Completed(())
        });
        let second = scheduler.submit(JobPriority::Foreground, move |_context| {
            started_second.send(()).unwrap();
            JobResult::Completed(())
        });

        assert!(
            second_started
                .recv_timeout(Duration::from_millis(30))
                .is_err()
        );
        release_first.send(()).unwrap();
        let _ = first.completion.recv().unwrap();
        second_started.recv_timeout(Duration::from_secs(1)).unwrap();
        let _ = second.completion.recv().unwrap();
    }
}
