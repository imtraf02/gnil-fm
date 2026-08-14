use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use gnil_core::{
    ConflictDecision, FsOperation, JobDecision, JobEvent, JobId, JobPriority, JobProgress,
    JobPrompt, JobState, OperationOutcome, UndoRecord,
};
use gnil_fs::{JobHandle, JobResult, OperationExecutor, TaskScheduler};
use gpui::{
    App, Bounds, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable, Global,
    KeyBinding, Render, SharedString, Subscription, Window, WindowBounds, WindowHandle,
    WindowOptions, actions, div, prelude::*, px, relative, rgb, size,
};

use crate::{
    theme_runtime::{
        accent, accent_background, border, danger, error as error_color, surface, surface_elevated,
        text, text_emphasized, text_muted, warning,
    },
    ui::control::FocusableControl,
    ui::icon::{IconSize, IconTone, ui_icon},
};

const WINDOW_DELAY: Duration = Duration::from_millis(400);
const SUCCESS_DISMISS_DELAY: Duration = Duration::from_millis(900);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

actions!(operation_window, [DismissOperationWindow]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OperationKind {
    Copy,
    Move,
    Trash,
    Delete,
    Restore,
    Extract,
    Create,
    Rename,
    Permissions,
    Undo,
}

impl OperationKind {
    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Copy => "icons/action-copy.svg",
            Self::Move => "icons/action-cut.svg",
            Self::Trash | Self::Delete => "icons/trash.svg",
            Self::Restore | Self::Undo => "icons/action-restore.svg",
            Self::Extract => "icons/file-archive.svg",
            Self::Create => "icons/action-new.svg",
            Self::Rename => "icons/action-rename.svg",
            Self::Permissions => "icons/action-permissions.svg",
        }
    }
}

#[derive(Clone)]
pub(crate) struct OperationSnapshot {
    pub(crate) id: JobId,
    pub(crate) kind: OperationKind,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) state: JobState,
    pub(crate) queue_position: Option<usize>,
    pub(crate) progress: JobProgress,
    pub(crate) prompt: Option<JobPrompt>,
    pub(crate) error: Option<String>,
    pub(crate) outcome: Option<OperationOutcome>,
    pub(crate) bytes_per_second: Option<f64>,
}

impl OperationSnapshot {
    pub(crate) fn terminal(&self) -> bool {
        matches!(
            self.state,
            JobState::Completed
                | JobState::CompletedWithIssues
                | JobState::Cancelled
                | JobState::Failed
        )
    }

    pub(crate) fn status_label(&self) -> &'static str {
        match self.state {
            JobState::Queued => "Queued",
            JobState::Preparing => "Preparing…",
            JobState::Running => "In progress",
            JobState::Paused => "Paused",
            JobState::WaitingForConflict => "Needs a decision",
            JobState::WaitingForError => "Needs attention",
            JobState::Completed => "Completed",
            JobState::CompletedWithIssues => "Completed with issues",
            JobState::Cancelled => "Cancelled",
            JobState::Failed => "Failed",
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn progress_ratio(&self) -> Option<f32> {
        if let Some(total) = self.progress.total_bytes.filter(|total| *total > 0) {
            return Some((self.progress.completed_bytes as f32 / total as f32).clamp(0.0, 1.0));
        }
        self.progress
            .total_items
            .filter(|total| *total > 0)
            .map(|total| (self.progress.completed_items as f32 / total as f32).clamp(0.0, 1.0))
    }
}

#[derive(Clone)]
pub(crate) enum OperationCenterEvent {
    Finished {
        origin: EntityId,
        state: JobState,
        outcome: OperationOutcome,
        error: Option<String>,
        clear_clipboard: bool,
    },
}

struct OperationRecord {
    snapshot: OperationSnapshot,
    origin: EntityId,
    clear_clipboard: bool,
    started_at: Option<Instant>,
    samples: VecDeque<(Instant, u64)>,
    handle: JobHandle<OperationOutcome>,
    window: Option<WindowHandle<OperationWindow>>,
    reserved_undo: Option<UndoRecord>,
}

pub(crate) struct OperationCoordinator {
    scheduler: TaskScheduler<OperationOutcome>,
    records: HashMap<JobId, OperationRecord>,
    order: Vec<JobId>,
    undo_stack: Vec<UndoRecord>,
    polling: bool,
}

impl EventEmitter<OperationCenterEvent> for OperationCoordinator {}

struct OperationRegistry(Entity<OperationCoordinator>);

impl Global for OperationRegistry {}

pub(crate) fn operation_coordinator(cx: &mut App) -> Entity<OperationCoordinator> {
    if !cx.has_global::<OperationRegistry>() {
        let coordinator = cx.new(|_| OperationCoordinator {
            scheduler: TaskScheduler::new(1),
            records: HashMap::new(),
            order: Vec::new(),
            undo_stack: Vec::new(),
            polling: false,
        });
        cx.set_global(OperationRegistry(coordinator));
        cx.bind_keys([KeyBinding::new(
            "escape",
            DismissOperationWindow,
            Some("OperationWindow"),
        )]);
    }
    cx.global::<OperationRegistry>().0.clone()
}

impl OperationCoordinator {
    pub(crate) fn enqueue(
        &mut self,
        operation: FsOperation,
        label: &str,
        origin: EntityId,
        clear_clipboard: bool,
        cx: &mut Context<Self>,
    ) -> JobId {
        let (kind, title, subtitle) = presentation(&operation, label);
        let handle = self.scheduler.submit(JobPriority::Foreground, move |job| {
            OperationExecutor.execute_job(&operation, &job)
        });
        let id = handle.id;
        self.records.insert(
            id,
            OperationRecord {
                snapshot: OperationSnapshot {
                    id,
                    kind,
                    title,
                    subtitle,
                    state: JobState::Queued,
                    queue_position: None,
                    progress: JobProgress::default(),
                    prompt: None,
                    error: None,
                    outcome: None,
                    bytes_per_second: None,
                },
                origin,
                clear_clipboard,
                started_at: None,
                samples: VecDeque::new(),
                handle,
                window: None,
                reserved_undo: None,
            },
        );
        self.order.push(id);
        self.refresh_queue_positions();
        Self::schedule_window(id, cx);
        self.ensure_polling(cx);
        cx.notify();
        id
    }

    pub(crate) fn enqueue_undo(
        &mut self,
        origin: EntityId,
        cx: &mut Context<Self>,
    ) -> Option<JobId> {
        let record = self.undo_stack.pop()?;
        let label = record.label.clone();
        let worker_record = record.clone();
        let handle =
            self.scheduler
                .submit(JobPriority::Foreground, move |_job| match OperationExecutor
                    .undo(&worker_record)
                {
                    Ok(()) => JobResult::Completed(OperationOutcome::default()),
                    Err(error) => JobResult::Failed {
                        value: Some(OperationOutcome::default()),
                        message: error.to_string(),
                    },
                });
        let id = handle.id;
        self.records.insert(
            id,
            OperationRecord {
                snapshot: OperationSnapshot {
                    id,
                    kind: OperationKind::Undo,
                    title: format!("Undo {label}"),
                    subtitle: "Reverting the latest completed operation".into(),
                    state: JobState::Queued,
                    queue_position: None,
                    progress: JobProgress {
                        total_items: Some(1),
                        ..JobProgress::default()
                    },
                    prompt: None,
                    error: None,
                    outcome: None,
                    bytes_per_second: None,
                },
                origin,
                clear_clipboard: false,
                started_at: None,
                samples: VecDeque::new(),
                handle,
                window: None,
                reserved_undo: Some(record),
            },
        );
        self.order.push(id);
        self.refresh_queue_positions();
        Self::schedule_window(id, cx);
        self.ensure_polling(cx);
        cx.notify();
        Some(id)
    }

    pub(crate) fn snapshots(&self) -> Vec<OperationSnapshot> {
        self.order
            .iter()
            .filter_map(|id| self.records.get(id).map(|record| record.snapshot.clone()))
            .collect()
    }

    pub(crate) fn snapshot(&self, id: JobId) -> Option<OperationSnapshot> {
        self.records.get(&id).map(|record| record.snapshot.clone())
    }

    pub(crate) fn active_snapshot(&self) -> Option<OperationSnapshot> {
        self.order
            .iter()
            .filter_map(|id| self.records.get(id))
            .find(|record| !record.snapshot.terminal())
            .or_else(|| self.order.iter().find_map(|id| self.records.get(id)))
            .map(|record| record.snapshot.clone())
    }

    pub(crate) fn count(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn has_active_operations(&self) -> bool {
        self.records
            .values()
            .any(|record| !record.snapshot.terminal())
    }

    pub(crate) fn show_window(&mut self, id: JobId, cx: &mut Context<Self>) {
        if let Some(handle) = self.records.get(&id).and_then(|record| record.window)
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        if let Some(record) = self.records.get_mut(&id) {
            record.window = None;
        }
        self.open_window(id, cx);
    }

    pub(crate) fn pause(&self, id: JobId) {
        if let Some(record) = self.records.get(&id) {
            record.handle.controller.pause();
        }
    }

    pub(crate) fn resume(&self, id: JobId) {
        if let Some(record) = self.records.get(&id) {
            record.handle.controller.resume();
        }
    }

    pub(crate) fn cancel(&self, id: JobId) {
        if let Some(record) = self.records.get(&id) {
            record.handle.controller.cancel();
        }
    }

    pub(crate) fn respond(&self, id: JobId, decision: JobDecision) {
        if let Some(record) = self.records.get(&id) {
            record.handle.controller.respond(decision);
        }
    }

    pub(crate) fn dismiss(&mut self, id: JobId, cx: &mut Context<Self>) {
        let Some(record) = self.records.get(&id) else {
            return;
        };
        if !record.snapshot.terminal() {
            if let Some(handle) = record.window {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
            if let Some(record) = self.records.get_mut(&id) {
                record.window = None;
            }
            cx.notify();
            return;
        }
        if let Some(record) = self.records.remove(&id)
            && let Some(handle) = record.window
        {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        self.order.retain(|candidate| *candidate != id);
        self.refresh_queue_positions();
        cx.notify();
        if self.records.is_empty() && cx.windows().is_empty() {
            cx.quit();
        }
    }

    fn schedule_window(id: JobId, cx: &mut Context<Self>) {
        let timer = cx.background_executor().timer(WINDOW_DELAY);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .records
                    .get(&id)
                    .is_some_and(|record| !record.snapshot.terminal())
                {
                    this.show_window(id, cx);
                }
            });
        })
        .detach();
    }

    fn open_window(&mut self, id: JobId, cx: &mut Context<Self>) {
        if !self.records.contains_key(&id) {
            return;
        }
        let center = cx.entity();
        let bounds = Bounds::centered(None, size(px(560.0), px(300.0)), cx);
        let title = self.records.get(&id).map_or_else(
            || "File operation".into(),
            |record| record.snapshot.title.clone(),
        );
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(480.0), px(260.0))),
                window_background: gpui::WindowBackgroundAppearance::Opaque,
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(format!("{title} — gnil-fm").into()),
                    ..Default::default()
                }),
                app_id: Some("gnil-fm".into()),
                ..Default::default()
            },
            move |window, cx| {
                cx.new(|cx| {
                    let operation_window = OperationWindow::new(center, id, cx);
                    window.focus(&operation_window.focus_handle(cx));
                    operation_window
                })
            },
        );
        if let Ok(handle) = opened {
            if let Some(record) = self.records.get_mut(&id) {
                record.window = Some(handle);
            }
            cx.activate(true);
        }
    }

    fn ensure_polling(&mut self, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        self.poll(cx);
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        let ids = self.order.clone();
        let mut finished = Vec::new();
        for id in ids {
            let events = self
                .records
                .get(&id)
                .map(|record| record.handle.events.try_iter().collect::<Vec<_>>())
                .unwrap_or_default();
            for event in events {
                self.apply_event(id, event);
            }
            let result = self
                .records
                .get(&id)
                .and_then(|record| record.handle.completion.try_recv().ok());
            if let Some(result) = result {
                finished.push((id, result));
            }
        }
        for (id, result) in finished {
            self.finish(id, result, cx);
        }
        self.refresh_queue_positions();
        cx.notify();

        if self
            .records
            .values()
            .any(|record| !record.snapshot.terminal())
        {
            let timer = cx.background_executor().timer(POLL_INTERVAL);
            cx.spawn(async move |this, cx| {
                timer.await;
                let _ = this.update(cx, OperationCoordinator::poll);
            })
            .detach();
        } else {
            self.polling = false;
        }
    }

    fn apply_event(&mut self, id: JobId, event: JobEvent) {
        let Some(record) = self.records.get_mut(&id) else {
            return;
        };
        match event {
            JobEvent::State { state, .. } => {
                record.snapshot.state = state;
                if state == JobState::Running && record.started_at.is_none() {
                    record.started_at = Some(Instant::now());
                }
                if !matches!(
                    state,
                    JobState::WaitingForConflict | JobState::WaitingForError
                ) {
                    record.snapshot.prompt = None;
                }
            }
            JobEvent::Progress { progress, .. } => {
                let now = Instant::now();
                record.samples.push_back((now, progress.completed_bytes));
                while record
                    .samples
                    .front()
                    .is_some_and(|(at, _)| now.duration_since(*at) > Duration::from_secs(2))
                {
                    record.samples.pop_front();
                }
                record.snapshot.bytes_per_second =
                    record.samples.front().and_then(|(at, bytes)| {
                        let seconds = now.duration_since(*at).as_secs_f64();
                        (seconds >= 0.5).then(|| {
                            rate_from_sample(
                                progress.completed_bytes.saturating_sub(*bytes),
                                seconds,
                            )
                        })
                    });
                record.snapshot.progress = progress;
            }
            JobEvent::Prompt { prompt, .. } => record.snapshot.prompt = Some(prompt),
            JobEvent::Message { message, .. } => record.snapshot.error = Some(message),
        }
    }

    fn finish(&mut self, id: JobId, result: JobResult<OperationOutcome>, cx: &mut Context<Self>) {
        let (state, outcome, error) = match result {
            JobResult::Completed(outcome) => (JobState::Completed, outcome, None),
            JobResult::CompletedWithIssues(outcome) => {
                (JobState::CompletedWithIssues, outcome, None)
            }
            JobResult::Cancelled(outcome) => (JobState::Cancelled, outcome, None),
            JobResult::Failed { value, message } => {
                (JobState::Failed, value.unwrap_or_default(), Some(message))
            }
        };
        let Some(record) = self.records.get_mut(&id) else {
            return;
        };
        record.snapshot.state = state;
        record.snapshot.error.clone_from(&error);
        record.snapshot.prompt = None;
        record.snapshot.outcome = Some(outcome.clone());
        if let Some(undo) = outcome.undo.clone() {
            self.undo_stack.push(undo);
        }
        if !matches!(state, JobState::Completed | JobState::CompletedWithIssues)
            && let Some(undo) = record.reserved_undo.take()
        {
            self.undo_stack.push(undo);
        }
        let event = OperationCenterEvent::Finished {
            origin: record.origin,
            state,
            outcome,
            error,
            clear_clipboard: record.clear_clipboard,
        };
        cx.emit(event);

        if state == JobState::Completed {
            let timer = cx.background_executor().timer(SUCCESS_DISMISS_DELAY);
            cx.spawn(async move |this, cx| {
                timer.await;
                let _ = this.update(cx, |this, cx| this.dismiss(id, cx));
            })
            .detach();
        } else {
            self.show_window(id, cx);
        }
    }

    fn refresh_queue_positions(&mut self) {
        let mut position = 0;
        for id in &self.order {
            let Some(record) = self.records.get_mut(id) else {
                continue;
            };
            if record.snapshot.state == JobState::Queued {
                position += 1;
                record.snapshot.queue_position = Some(position);
            } else {
                record.snapshot.queue_position = None;
            }
        }
    }
}

impl Drop for OperationCoordinator {
    fn drop(&mut self) {
        for record in self.records.values() {
            record.handle.controller.cancel();
        }
    }
}

fn presentation(operation: &FsOperation, fallback: &str) -> (OperationKind, String, String) {
    let count = operation_sources(operation).len();
    let item_label = format!("{count} item{}", if count == 1 { "" } else { "s" });
    match operation {
        FsOperation::Copy { destination, .. } => (
            OperationKind::Copy,
            "Copying files".into(),
            format!("{item_label} to {}", destination.display()),
        ),
        FsOperation::Move { destination, .. } => (
            OperationKind::Move,
            "Moving files".into(),
            format!("{item_label} to {}", destination.display()),
        ),
        FsOperation::Trash { .. } => (OperationKind::Trash, "Moving to Trash".into(), item_label),
        FsOperation::DeletePermanently { .. } | FsOperation::PurgeTrash { .. } => (
            OperationKind::Delete,
            "Deleting permanently".into(),
            item_label,
        ),
        FsOperation::RestoreTrash { .. } => (
            OperationKind::Restore,
            "Restoring from Trash".into(),
            item_label,
        ),
        FsOperation::ExtractArchives { destination, .. } => (
            OperationKind::Extract,
            "Extracting archives".into(),
            format!("{item_label} to {}", destination.display()),
        ),
        FsOperation::CreateFile { .. }
        | FsOperation::CreateDirectory { .. }
        | FsOperation::CreateSymlink { .. } => (
            OperationKind::Create,
            "Creating item".into(),
            fallback.into(),
        ),
        FsOperation::Rename { .. } | FsOperation::BulkRename { .. } => {
            (OperationKind::Rename, "Renaming items".into(), item_label)
        }
        FsOperation::SetPermissions { .. } => (
            OperationKind::Permissions,
            "Changing permissions".into(),
            item_label,
        ),
    }
}

fn operation_sources(operation: &FsOperation) -> Vec<PathBuf> {
    match operation {
        FsOperation::CreateFile { path } | FsOperation::CreateDirectory { path } => {
            vec![path.clone()]
        }
        FsOperation::Rename { from, .. } => vec![from.clone()],
        FsOperation::BulkRename { pairs } => pairs.iter().map(|pair| pair.from.clone()).collect(),
        FsOperation::CreateSymlink { link_path, .. } => vec![link_path.clone()],
        FsOperation::SetPermissions { paths, .. }
        | FsOperation::Copy { sources: paths, .. }
        | FsOperation::Move { sources: paths, .. }
        | FsOperation::Trash { paths }
        | FsOperation::DeletePermanently { paths }
        | FsOperation::ExtractArchives { sources: paths, .. } => paths.clone(),
        FsOperation::RestoreTrash { entries, .. } | FsOperation::PurgeTrash { entries } => entries
            .iter()
            .map(|entry| entry.trashed_path.clone())
            .collect(),
    }
}

struct OperationWindow {
    center: Entity<OperationCoordinator>,
    job_id: JobId,
    apply_to_all: bool,
    focus_handle: FocusHandle,
    _center_subscription: Subscription,
}

impl OperationWindow {
    fn new(center: Entity<OperationCoordinator>, job_id: JobId, cx: &mut Context<Self>) -> Self {
        let center_subscription = cx.observe(&center, |_, _, cx| cx.notify());
        Self {
            center,
            job_id,
            apply_to_all: false,
            focus_handle: cx.focus_handle(),
            _center_subscription: center_subscription,
        }
    }

    fn dismiss(&mut self, _: &DismissOperationWindow, _: &mut Window, cx: &mut Context<Self>) {
        self.center
            .update(cx, |center, cx| center.dismiss(self.job_id, cx));
    }

    #[allow(clippy::too_many_lines)]
    fn render_prompt(&mut self, prompt: JobPrompt, cx: &mut Context<Self>) -> gpui::AnyElement {
        match prompt {
            JobPrompt::Conflict {
                source,
                destination,
                source_is_directory,
                destination_is_directory,
            } => {
                let can_merge = source_is_directory && destination_is_directory;
                let center = self.center.clone();
                let id = self.job_id;
                let apply_to_all = self.apply_to_all;
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(surface_elevated()))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_sm()
                            .text_color(rgb(text_emphasized()))
                            .child("An item with this name already exists"),
                    )
                    .child(path_line("Source", &source))
                    .child(path_line("Destination", &destination))
                    .child(
                        div()
                            .id("operation-apply-to-all")
                            .with_focus_ring()
                            .h_8()
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_between()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(rgb(text()))
                            .bg(if apply_to_all {
                                rgb(accent_background())
                            } else {
                                rgb(surface_elevated())
                            })
                            .hover(|style| style.bg(rgb(border())))
                            .child("Apply this decision to all remaining conflicts")
                            .child(if apply_to_all { "On" } else { "Off" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply_to_all = !this.apply_to_all;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(decision_button("Skip", false).on_click({
                                let center = center.clone();
                                move |_, _, cx| {
                                    center.update(cx, |center, _| {
                                        center.respond(
                                            id,
                                            JobDecision::Conflict {
                                                decision: ConflictDecision::Skip,
                                                apply_to_all,
                                            },
                                        );
                                    });
                                }
                            }))
                            .when(can_merge, |row| {
                                row.child(decision_button("Merge", false).on_click({
                                    let center = center.clone();
                                    move |_, _, cx| {
                                        center.update(cx, |center, _| {
                                            center.respond(
                                                id,
                                                JobDecision::Conflict {
                                                    decision: ConflictDecision::MergeDirectory,
                                                    apply_to_all,
                                                },
                                            );
                                        });
                                    }
                                }))
                            })
                            .child(decision_button("Replace", false).on_click({
                                let center = center.clone();
                                move |_, _, cx| {
                                    center.update(cx, |center, _| {
                                        center.respond(
                                            id,
                                            JobDecision::Conflict {
                                                decision: ConflictDecision::Replace,
                                                apply_to_all,
                                            },
                                        );
                                    });
                                }
                            }))
                            .child(
                                decision_button("Keep both", true).on_click(move |_, _, cx| {
                                    center.update(cx, |center, _| {
                                        center.respond(
                                            id,
                                            JobDecision::Conflict {
                                                decision: ConflictDecision::KeepBoth,
                                                apply_to_all,
                                            },
                                        );
                                    });
                                }),
                            ),
                    )
                    .into_any_element()
            }
            JobPrompt::IoError {
                path,
                message,
                can_skip,
            } => {
                let center = self.center.clone();
                let id = self.job_id;
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(surface_elevated()))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_sm()
                            .text_color(rgb(error_color()))
                            .child("The operation could not continue"),
                    )
                    .child(path_line("Item", &path))
                    .child(div().text_xs().text_color(rgb(text_muted())).child(message))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(decision_button("Cancel job", false).on_click({
                                let center = center.clone();
                                move |_, _, cx| {
                                    center.update(cx, |center, _| {
                                        center.respond(id, JobDecision::Cancel);
                                    });
                                }
                            }))
                            .when(can_skip, |row| {
                                row.child(decision_button("Skip", false).on_click({
                                    let center = center.clone();
                                    move |_, _, cx| {
                                        center.update(cx, |center, _| {
                                            center.respond(id, JobDecision::Skip);
                                        });
                                    }
                                }))
                            })
                            .child(decision_button("Retry", true).on_click(move |_, _, cx| {
                                center.update(cx, |center, _| {
                                    center.respond(id, JobDecision::Retry);
                                });
                            })),
                    )
                    .into_any_element()
            }
        }
    }
}

impl Focusable for OperationWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for OperationWindow {
    #[allow(clippy::too_many_lines)]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(snapshot) = self.center.read(cx).snapshot(self.job_id) else {
            window.remove_window();
            return div().size_full().into_any_element();
        };
        let ratio = snapshot.progress_ratio().unwrap_or(0.0);
        let current_path = snapshot.progress.current_path.as_ref().map_or_else(
            || snapshot.subtitle.clone(),
            |path| path.display().to_string(),
        );
        let progress_text = progress_text(&snapshot);
        let center = self.center.clone();
        let id = self.job_id;
        let paused = snapshot.state == JobState::Paused;
        let terminal = snapshot.terminal();
        let prompt = snapshot.prompt.clone();
        let error = snapshot.error.clone();

        div()
            .id("operation-window-root")
            .key_context("OperationWindow")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::dismiss))
            .size_full()
            .bg(rgb(surface()))
            .text_color(rgb(text()))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(ui_icon(
                        snapshot.kind.icon(),
                        IconSize::Detail,
                        if matches!(snapshot.state, JobState::Failed | JobState::WaitingForError) {
                            IconTone::Danger
                        } else {
                            IconTone::Accent
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_sm()
                                    .text_color(rgb(text_emphasized()))
                                    .truncate()
                                    .child(snapshot.title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(state_color(snapshot.state))
                                    .child(snapshot.status_label()),
                            ),
                    ),
            )
            .when_some(prompt, |root, prompt| {
                root.child(self.render_prompt(prompt, cx))
            })
            .when(snapshot.prompt.is_none(), |root| {
                root.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .h_2()
                                .w_full()
                                .rounded_md()
                                .overflow_hidden()
                                .bg(rgb(accent_background()))
                                .child(
                                    div()
                                        .h_full()
                                        .w(relative(ratio))
                                        .rounded_md()
                                        .bg(rgb(accent())),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .truncate()
                                        .child(current_path),
                                )
                                .child(
                                    div().text_xs().text_color(rgb(text())).child(progress_text),
                                ),
                        )
                        .when_some(error, |details, error| {
                            details.child(
                                div()
                                    .p_2()
                                    .rounded_md()
                                    .bg(rgb(surface_elevated()))
                                    .text_xs()
                                    .text_color(rgb(error_color()))
                                    .child(error),
                            )
                        }),
                )
            })
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .when(!terminal && snapshot.prompt.is_none(), |row| {
                        row.child(
                            icon_button(
                                if paused {
                                    "icons/action-play.svg"
                                } else {
                                    "icons/action-pause.svg"
                                },
                                if paused { "Resume" } else { "Pause" },
                            )
                            .on_click({
                                let center = center.clone();
                                move |_, _, cx| {
                                    center.update(cx, |center, _| {
                                        if paused {
                                            center.resume(id);
                                        } else {
                                            center.pause(id);
                                        }
                                    });
                                }
                            }),
                        )
                        .child(decision_button("Cancel", false).on_click({
                            let center = center.clone();
                            move |_, _, cx| {
                                center.update(cx, |center, _| center.cancel(id));
                            }
                        }))
                    })
                    .when(terminal, |row| {
                        row.child(decision_button("Close", true).on_click(move |_, _, cx| {
                            center.update(cx, |center, cx| center.dismiss(id, cx));
                        }))
                    }),
            )
            .into_any_element()
    }
}

fn path_line(label: &'static str, path: &Path) -> gpui::Div {
    div()
        .flex()
        .gap_2()
        .text_xs()
        .child(div().w(px(72.0)).text_color(rgb(text_muted())).child(label))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(rgb(text()))
                .child(path.display().to_string()),
        )
}

fn decision_button(label: &'static str, primary: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(format!("operation-{label}")))
        .with_focus_ring()
        .h_8()
        .px_3()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_sm()
        .bg(if primary {
            rgb(accent())
        } else {
            rgb(surface_elevated())
        })
        .text_color(if primary { rgb(surface()) } else { rgb(text()) })
        .hover(move |style| {
            style.bg(if primary {
                rgb(accent_background())
            } else {
                rgb(border())
            })
        })
}

fn icon_button(icon: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(format!("operation-{label}")))
        .with_focus_ring()
        .h_8()
        .px_2()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .cursor_pointer()
        .text_sm()
        .text_color(rgb(text()))
        .hover(|style| style.bg(rgb(border())))
        .child(ui_icon(icon, IconSize::Small, IconTone::Default))
        .child(label)
}

fn progress_text(snapshot: &OperationSnapshot) -> String {
    if let Some(position) = snapshot.queue_position {
        return format!("Queued · position {position}");
    }
    let mut parts = Vec::new();
    if let Some(total) = snapshot.progress.total_items {
        parts.push(format!(
            "{} / {total} items",
            snapshot.progress.completed_items
        ));
    }
    if let Some(total) = snapshot.progress.total_bytes.filter(|total| *total > 0) {
        parts.push(format!(
            "{} / {}",
            format_bytes(snapshot.progress.completed_bytes),
            format_bytes(total)
        ));
    }
    if let Some(rate) = snapshot.bytes_per_second.filter(|rate| *rate > 0.0) {
        parts.push(format!("{}/s", format_decimal_bytes(rate)));
        if let Some(total) = snapshot.progress.total_bytes {
            let seconds = eta_seconds(
                total.saturating_sub(snapshot.progress.completed_bytes),
                rate,
            );
            if seconds > 0.0 {
                parts.push(format!("{} remaining", format_duration(seconds)));
            }
        }
    }
    if parts.is_empty() {
        snapshot.status_label().into()
    } else {
        parts.join(" · ")
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut divisor = 1_u64;
    let mut unit = 0_usize;
    while bytes / divisor >= 1024 && unit < UNITS.len() - 1 {
        divisor = divisor.saturating_mul(1024);
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        let whole = bytes / divisor;
        let decimal = bytes % divisor * 10 / divisor;
        format!("{whole}.{decimal} {}", UNITS[unit])
    }
}

fn format_duration(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{seconds:.0}s")
    } else if seconds < 3600.0 {
        format!("{:.0}m {:.0}s", (seconds / 60.0).floor(), seconds % 60.0)
    } else {
        format!(
            "{:.0}h {:.0}m",
            (seconds / 3600.0).floor(),
            ((seconds % 3600.0) / 60.0).floor()
        )
    }
}

#[allow(clippy::cast_precision_loss)]
fn rate_from_sample(bytes: u64, seconds: f64) -> f64 {
    bytes as f64 / seconds
}

fn format_decimal_bytes(bytes: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[allow(clippy::cast_precision_loss)]
fn eta_seconds(remaining_bytes: u64, bytes_per_second: f64) -> f64 {
    (remaining_bytes as f64 / bytes_per_second).ceil()
}

fn state_color(state: JobState) -> gpui::Rgba {
    rgb(match state {
        JobState::Failed | JobState::WaitingForError => error_color(),
        JobState::Paused | JobState::WaitingForConflict | JobState::CompletedWithIssues => {
            warning()
        }
        JobState::Cancelled => danger(),
        JobState::Completed => accent(),
        _ => text_muted(),
    })
}
