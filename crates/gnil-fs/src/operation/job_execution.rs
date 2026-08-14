use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    os::unix::fs::{MetadataExt as _, PermissionsExt as _},
    path::{Path, PathBuf},
};

use gnil_core::{
    ConflictDecision, FsOperation, JobDecision, JobProgress, JobPrompt, JobState, OperationIssue,
    OperationOutcome, TrashEntryRef, UndoKind, UndoRecord,
};
use uuid::Uuid;

use crate::{
    JobContext, JobResult,
    fingerprint::{self, MAX_UNDO_ENTRIES},
    secure_fs,
};

use super::{OperationError, OperationExecutor};

#[derive(Clone, Copy, Debug, Default)]
struct Totals {
    items: u64,
    bytes: u64,
}

impl Totals {
    fn add(&mut self, other: Self) {
        self.items = self.items.saturating_add(other.items);
        self.bytes = self.bytes.saturating_add(other.bytes);
    }
}

struct Prepared {
    total: Totals,
    roots: HashMap<PathBuf, Totals>,
    entry_bytes: HashMap<PathBuf, u64>,
    identities: HashMap<PathBuf, SourceIdentity>,
}

#[derive(Clone, Copy)]
struct SourceIdentity {
    device: u64,
    inode: u64,
}

struct ProgressTracker<'a> {
    job: &'a JobContext,
    total: Totals,
    completed: Totals,
    current: Totals,
    reported: Totals,
}

impl<'a> ProgressTracker<'a> {
    fn new(job: &'a JobContext, total: Totals) -> Self {
        let mut tracker = Self {
            job,
            total,
            completed: Totals::default(),
            current: Totals::default(),
            reported: Totals::default(),
        };
        tracker.emit(None);
        tracker
    }

    fn checkpoint(&self) -> Result<(), OperationError> {
        if self.job.checkpoint() {
            Ok(())
        } else {
            Err(OperationError::Cancelled)
        }
    }

    fn add_bytes(&mut self, bytes: u64, path: &Path) {
        self.current.bytes = self.current.bytes.saturating_add(bytes);
        self.emit(Some(path));
    }

    fn add_item(&mut self, bytes: u64, path: &Path) {
        self.current.items = self.current.items.saturating_add(1);
        if bytes > self.current.bytes {
            self.current.bytes = bytes;
        }
        self.emit(Some(path));
    }

    fn commit_root(&mut self, totals: Totals, path: &Path) {
        self.completed.add(totals);
        self.current = Totals::default();
        self.emit(Some(path));
    }

    fn emit(&mut self, current_path: Option<&Path>) {
        let observed = Totals {
            items: self.completed.items.saturating_add(self.current.items),
            bytes: self.completed.bytes.saturating_add(self.current.bytes),
        };
        // Events are absolute and capped. Retries may repeat work, but must never make the
        // progress bar move backwards or exceed the prepared total.
        let items = self
            .reported
            .items
            .max(observed.items)
            .min(self.total.items);
        let bytes = self
            .reported
            .bytes
            .max(observed.bytes)
            .min(self.total.bytes);
        self.reported = Totals { items, bytes };
        self.job.progress(JobProgress {
            completed_items: items,
            total_items: Some(self.total.items),
            completed_bytes: bytes,
            total_bytes: Some(self.total.bytes),
            current_path: current_path.map(Path::to_path_buf),
        });
    }
}

enum Run {
    Completed(OperationOutcome),
    Cancelled(OperationOutcome),
    Failed(OperationOutcome, String),
}

enum MoveTransfer {
    Renamed,
    Copied,
}

enum SourceRemoval {
    Removed,
    Skipped(OperationIssue),
    Cancelled(OperationIssue),
}

impl Run {
    fn into_job_result(self) -> JobResult<OperationOutcome> {
        match self {
            Self::Completed(outcome)
                if outcome.issues.is_empty() && outcome.skipped_paths.is_empty() =>
            {
                JobResult::Completed(outcome)
            }
            Self::Completed(outcome) => JobResult::CompletedWithIssues(outcome),
            Self::Cancelled(outcome) => JobResult::Cancelled(outcome),
            Self::Failed(outcome, message) => JobResult::Failed {
                value: Some(outcome),
                message,
            },
        }
    }
}

pub(super) fn execute(
    executor: &OperationExecutor,
    operation: &FsOperation,
    job: &JobContext,
) -> JobResult<OperationOutcome> {
    job.state(JobState::Preparing);
    let prepared = match prepare(operation, job) {
        Ok(prepared) => prepared,
        Err(OperationError::Cancelled) => {
            return JobResult::Cancelled(OperationOutcome::default());
        }
        Err(error) => {
            return JobResult::Failed {
                value: None,
                message: error.to_string(),
            };
        }
    };
    job.state(JobState::Running);

    let run = match operation {
        FsOperation::Copy {
            sources,
            destination,
            conflict,
        } => copy_paths(sources, destination, *conflict, &prepared, job),
        FsOperation::Move {
            sources,
            destination,
            conflict,
        } => move_paths(sources, destination, *conflict, &prepared, job),
        FsOperation::DeletePermanently { paths } => delete_paths(paths, &prepared, false, job),
        FsOperation::Trash { paths } => trash_paths(paths, &prepared, job),
        FsOperation::RestoreTrash {
            entries,
            replace_existing,
        } => restore_entries(executor, entries, *replace_existing, &prepared, job),
        FsOperation::PurgeTrash { entries } => purge_entries(entries, &prepared, job),
        FsOperation::ExtractArchives { .. } => extract(executor, operation, job),
        _ => quick_operation(executor, operation, prepared.total, job),
    };
    run.into_job_result()
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

fn prepare(operation: &FsOperation, job: &JobContext) -> Result<Prepared, OperationError> {
    let mut total = Totals::default();
    let mut roots = HashMap::new();
    let mut entry_bytes = HashMap::new();
    let mut identities = HashMap::new();
    let sources = operation_sources(operation);

    for source in sources {
        if !job.checkpoint() {
            return Err(OperationError::Cancelled);
        }
        let root = if source.exists() || fs::symlink_metadata(&source).is_ok() {
            scan_path(&source, job, &mut entry_bytes, &mut identities)?
        } else {
            // Create operations do not have a source yet, but still expose one determinate item.
            Totals { items: 1, bytes: 0 }
        };
        total.add(root);
        roots.insert(source, root);
    }
    if total.items == 0 {
        total.items = 1;
    }
    Ok(Prepared {
        total,
        roots,
        entry_bytes,
        identities,
    })
}

fn scan_path(
    path: &Path,
    job: &JobContext,
    entry_bytes: &mut HashMap<PathBuf, u64>,
    identities: &mut HashMap<PathBuf, SourceIdentity>,
) -> Result<Totals, OperationError> {
    if !job.checkpoint() {
        return Err(OperationError::Cancelled);
    }
    let metadata = fs::symlink_metadata(path)?;
    identities.insert(
        path.to_path_buf(),
        SourceIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
    );
    let bytes = if metadata.is_file() {
        metadata.len()
    } else {
        0
    };
    entry_bytes.insert(path.to_path_buf(), bytes);
    let mut totals = Totals { items: 1, bytes };
    job.progress(JobProgress {
        completed_items: entry_bytes.len() as u64,
        total_items: None,
        completed_bytes: totals.bytes,
        total_bytes: None,
        current_path: Some(path.to_path_buf()),
    });
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        for entry in fs::read_dir(path)? {
            totals.add(scan_path(&entry?.path(), job, entry_bytes, identities)?);
        }
    }
    Ok(totals)
}

fn quick_operation(
    executor: &OperationExecutor,
    operation: &FsOperation,
    total: Totals,
    job: &JobContext,
) -> Run {
    let tracker = ProgressTracker::new(job, total);
    if let Err(OperationError::Cancelled) = tracker.checkpoint() {
        return Run::Cancelled(OperationOutcome::default());
    }
    match executor.execute(operation, job.cancelled_flag()) {
        Ok(outcome) => {
            job.progress(JobProgress {
                completed_items: total.items,
                total_items: Some(total.items),
                completed_bytes: total.bytes,
                total_bytes: Some(total.bytes),
                current_path: operation_sources(operation).last().cloned(),
            });
            Run::Completed(outcome)
        }
        Err(OperationError::Cancelled) => Run::Cancelled(OperationOutcome::default()),
        Err(error) => Run::Failed(OperationOutcome::default(), error.to_string()),
    }
}

fn extract(executor: &OperationExecutor, operation: &FsOperation, job: &JobContext) -> Run {
    let result = executor.execute_with_progress(operation, job.cancelled_flag(), &mut |progress| {
        let _ = job.checkpoint();
        job.progress(progress);
    });
    match result {
        Ok(outcome) => Run::Completed(outcome),
        Err(OperationError::Cancelled) => Run::Cancelled(OperationOutcome::default()),
        Err(error) => Run::Failed(OperationOutcome::default(), error.to_string()),
    }
}

fn copy_paths(
    sources: &[PathBuf],
    destination: &Path,
    initial_conflict: ConflictDecision,
    prepared: &Prepared,
    job: &JobContext,
) -> Run {
    if let Err(error) = super::validate_transfer_destination(sources, destination) {
        return Run::Failed(OperationOutcome::default(), error.to_string());
    }
    let mut tracker = ProgressTracker::new(job, prepared.total);
    let mut affected = Vec::new();
    let mut skipped = Vec::new();
    let mut issues = Vec::new();
    let mut apply_all = (initial_conflict != ConflictDecision::Ask).then_some(initial_conflict);

    for source in sources {
        let totals = prepared.roots.get(source).copied().unwrap_or_default();
        loop {
            if let Err(OperationError::Cancelled) = tracker.checkpoint() {
                return Run::Cancelled(copy_outcome(affected, skipped, issues));
            }
            let target = match choose_destination(source, destination, &mut apply_all, job) {
                Ok(Some(target)) => target,
                Ok(None) => {
                    skipped.push(source.clone());
                    break;
                }
                Err(OperationError::Cancelled) => {
                    return Run::Cancelled(copy_outcome(affected, skipped, issues));
                }
                Err(error) => {
                    return Run::Failed(copy_outcome(affected, skipped, issues), error.to_string());
                }
            };
            let merge = target.merge;
            match copy_one(
                source,
                &target.path,
                target.replace,
                merge,
                prepared,
                &mut tracker,
            ) {
                Ok(()) => {
                    affected.push(target.path.clone());
                    tracker.commit_root(totals, source);
                    break;
                }
                Err(OperationError::Cancelled) => {
                    return Run::Cancelled(copy_outcome(affected, skipped, issues));
                }
                Err(error) => match request_error(source, &error, true, job) {
                    JobDecision::Retry => {}
                    JobDecision::Skip => {
                        skipped.push(source.clone());
                        issues.push(OperationIssue {
                            path: source.clone(),
                            message: "Skipped after a copy error".into(),
                        });
                        break;
                    }
                    _ => return Run::Cancelled(copy_outcome(affected, skipped, issues)),
                },
            }
        }
    }
    Run::Completed(copy_outcome(affected, skipped, issues))
}

fn copy_outcome(
    affected_paths: Vec<PathBuf>,
    skipped_paths: Vec<PathBuf>,
    issues: Vec<OperationIssue>,
) -> OperationOutcome {
    let trees = fingerprint::fingerprint_trees(&affected_paths, MAX_UNDO_ENTRIES)
        .ok()
        .flatten();
    OperationOutcome {
        affected_paths,
        skipped_paths,
        issues,
        undo: trees.map(|trees| UndoRecord {
            label: "Copy".into(),
            kind: UndoKind::RemoveCreated { trees },
        }),
    }
}

fn move_paths(
    sources: &[PathBuf],
    destination: &Path,
    initial_conflict: ConflictDecision,
    prepared: &Prepared,
    job: &JobContext,
) -> Run {
    if let Err(error) = super::validate_transfer_destination(sources, destination) {
        return Run::Failed(OperationOutcome::default(), error.to_string());
    }
    let mut tracker = ProgressTracker::new(job, prepared.total);
    let mut affected = Vec::new();
    let mut skipped = Vec::new();
    let mut issues = Vec::new();
    let mut undo_pairs = Vec::new();
    let mut apply_all = (initial_conflict != ConflictDecision::Ask).then_some(initial_conflict);

    for source in sources {
        let totals = prepared.roots.get(source).copied().unwrap_or_default();
        'attempt: loop {
            if let Err(OperationError::Cancelled) = tracker.checkpoint() {
                return Run::Cancelled(move_outcome(affected, skipped, issues, undo_pairs));
            }
            let target = match choose_destination(source, destination, &mut apply_all, job) {
                Ok(Some(target)) => target,
                Ok(None) => {
                    skipped.push(source.clone());
                    break;
                }
                Err(OperationError::Cancelled) => {
                    return Run::Cancelled(move_outcome(affected, skipped, issues, undo_pairs));
                }
                Err(error) => {
                    return Run::Failed(
                        move_outcome(affected, skipped, issues, undo_pairs),
                        error.to_string(),
                    );
                }
            };
            if target.path == *source {
                skipped.push(source.clone());
                break;
            }
            let result = start_move_transfer(source, &target, prepared, &mut tracker);
            match result {
                Ok(MoveTransfer::Renamed) => {
                    affected.push(target.path.clone());
                    undo_pairs.push((target.path, source.clone()));
                    tracker.commit_root(totals, source);
                    break;
                }
                Ok(MoveTransfer::Copied) => {
                    match remove_copied_source(source, prepared, &mut tracker, job) {
                        SourceRemoval::Removed => {
                            affected.push(target.path.clone());
                            undo_pairs.push((target.path, source.clone()));
                            tracker.commit_root(totals, source);
                            break 'attempt;
                        }
                        SourceRemoval::Skipped(issue) => {
                            affected.push(target.path.clone());
                            skipped.push(source.clone());
                            issues.push(issue);
                            tracker.commit_root(totals, source);
                            break 'attempt;
                        }
                        SourceRemoval::Cancelled(issue) => {
                            affected.push(target.path.clone());
                            issues.push(issue);
                            return Run::Cancelled(move_outcome(
                                affected, skipped, issues, undo_pairs,
                            ));
                        }
                    }
                }
                Err(OperationError::Cancelled) => {
                    return Run::Cancelled(move_outcome(affected, skipped, issues, undo_pairs));
                }
                Err(error) => match request_error(source, &error, true, job) {
                    JobDecision::Retry => {}
                    JobDecision::Skip => {
                        skipped.push(source.clone());
                        issues.push(OperationIssue {
                            path: source.clone(),
                            message: "Skipped after a move error".into(),
                        });
                        break;
                    }
                    _ => {
                        return Run::Cancelled(move_outcome(affected, skipped, issues, undo_pairs));
                    }
                },
            }
        }
    }
    Run::Completed(move_outcome(affected, skipped, issues, undo_pairs))
}

fn remove_copied_source(
    source: &Path,
    prepared: &Prepared,
    tracker: &mut ProgressTracker<'_>,
    job: &JobContext,
) -> SourceRemoval {
    loop {
        match controlled_remove(source, prepared, tracker, false) {
            Ok(()) => return SourceRemoval::Removed,
            Err(OperationError::Cancelled) => {
                return SourceRemoval::Cancelled(OperationIssue {
                    path: source.to_path_buf(),
                    message:
                        "The destination copy completed, but removing the source was cancelled"
                            .into(),
                });
            }
            Err(error) => match request_error(source, &error, true, job) {
                JobDecision::Retry => {}
                JobDecision::Skip => {
                    return SourceRemoval::Skipped(OperationIssue {
                        path: source.to_path_buf(),
                        message: format!(
                            "The destination copy completed, but the source could not be removed: {error}"
                        ),
                    });
                }
                _ => {
                    return SourceRemoval::Cancelled(OperationIssue {
                        path: source.to_path_buf(),
                        message:
                            "The destination copy completed, but the source was not fully removed"
                                .into(),
                    });
                }
            },
        }
    }
}

fn move_outcome(
    affected_paths: Vec<PathBuf>,
    skipped_paths: Vec<PathBuf>,
    issues: Vec<OperationIssue>,
    pairs: Vec<(PathBuf, PathBuf)>,
) -> OperationOutcome {
    OperationOutcome {
        affected_paths,
        skipped_paths,
        issues,
        undo: (!pairs.is_empty()).then(|| UndoRecord {
            label: "Move".into(),
            kind: UndoKind::MoveBack { pairs },
        }),
    }
}

struct TargetChoice {
    path: PathBuf,
    replace: bool,
    merge: bool,
}

fn start_move_transfer(
    source: &Path,
    target: &TargetChoice,
    prepared: &Prepared,
    tracker: &mut ProgressTracker<'_>,
) -> Result<MoveTransfer, OperationError> {
    if target.replace || target.merge {
        return copy_one(
            source,
            &target.path,
            target.replace,
            target.merge,
            prepared,
            tracker,
        )
        .map(|()| MoveTransfer::Copied);
    }
    match validate_source_identity(source, prepared).and_then(|()| {
        super::rename_path_noreplace(source, &target.path).map_err(OperationError::from)
    }) {
        Ok(()) => Ok(MoveTransfer::Renamed),
        Err(OperationError::Io(error)) if error.kind() == io::ErrorKind::CrossesDevices => {
            copy_one(source, &target.path, false, false, prepared, tracker)
                .map(|()| MoveTransfer::Copied)
        }
        Err(error) => Err(error),
    }
}

fn choose_destination(
    source: &Path,
    destination: &Path,
    apply_all: &mut Option<ConflictDecision>,
    job: &JobContext,
) -> Result<Option<TargetChoice>, OperationError> {
    let name = source
        .file_name()
        .ok_or_else(|| OperationError::MissingFileName(source.to_path_buf()))?;
    let requested = destination.join(name);
    super::guard_recursive_destination(source, &requested)?;
    if !super::path_lexists(&requested) {
        return Ok(Some(TargetChoice {
            path: requested,
            replace: false,
            merge: false,
        }));
    }
    let decision = if let Some(decision) = *apply_all {
        decision
    } else {
        let source_is_directory = fs::symlink_metadata(source)?.is_dir();
        let destination_is_directory = fs::symlink_metadata(&requested)?.is_dir();
        match job.request(JobPrompt::Conflict {
            source: source.to_path_buf(),
            destination: requested.clone(),
            source_is_directory,
            destination_is_directory,
        }) {
            JobDecision::Conflict {
                decision,
                apply_to_all: all,
            } => {
                if all {
                    *apply_all = Some(decision);
                }
                decision
            }
            JobDecision::Cancel => return Err(OperationError::Cancelled),
            _ => return Err(OperationError::Conflict(requested)),
        }
    };
    Ok(match decision {
        ConflictDecision::Ask => return Err(OperationError::Conflict(requested)),
        ConflictDecision::Skip => None,
        ConflictDecision::KeepBoth => Some(TargetChoice {
            path: super::unique_copy_path(&requested),
            replace: false,
            merge: false,
        }),
        ConflictDecision::Replace => Some(TargetChoice {
            path: requested,
            replace: true,
            merge: false,
        }),
        ConflictDecision::MergeDirectory => Some(TargetChoice {
            path: requested,
            replace: false,
            merge: true,
        }),
    })
}

fn copy_one(
    source: &Path,
    target: &Path,
    replace: bool,
    merge: bool,
    prepared: &Prepared,
    tracker: &mut ProgressTracker<'_>,
) -> Result<(), OperationError> {
    if merge {
        return copy_tree(source, target, true, prepared, tracker);
    }
    let parent = target
        .parent()
        .ok_or_else(|| OperationError::MissingFileName(target.to_path_buf()))?;
    let staging = parent.join(format!(".gnil-part-{}", Uuid::new_v4()));
    let result = (|| {
        copy_tree(source, &staging, false, prepared, tracker)?;
        if replace {
            super::commit_staged_replacement(&staging, target)
        } else {
            super::rename_path_noreplace(&staging, target).map_err(OperationError::from)
        }
    })();
    if result.is_err() && super::path_lexists(&staging) {
        let _ = super::remove_path(&staging);
    }
    result
}

fn copy_tree(
    source: &Path,
    target: &Path,
    merge: bool,
    prepared: &Prepared,
    tracker: &mut ProgressTracker<'_>,
) -> Result<(), OperationError> {
    tracker.checkpoint()?;
    let metadata = fs::symlink_metadata(source)?;
    validate_metadata_identity(source, &metadata, prepared)?;
    if metadata.file_type().is_symlink() {
        super::copy_symlink(source, target)?;
        tracker.add_item(0, source);
    } else if metadata.is_dir() {
        let target_parent = secure_fs::open_parent(target)?;
        match secure_fs::create_dir(&target_parent, metadata.permissions().mode() & 0o7777) {
            Ok(directory) => drop(directory),
            Err(error) if merge && error.kind() == io::ErrorKind::AlreadyExists => {
                drop(secure_fs::open_dir(target)?);
            }
            Err(error) => return Err(error.into()),
        }
        for name in secure_fs::list_dir(source)? {
            copy_tree(
                &source.join(&name),
                &target.join(name),
                merge,
                prepared,
                tracker,
            )?;
        }
        let target_metadata = fs::symlink_metadata(target)?;
        secure_fs::chmod_nofollow(
            target,
            metadata.permissions().mode() & 0o7777,
            (target_metadata.dev(), target_metadata.ino()),
        )?;
        tracker.add_item(0, source);
    } else if metadata.is_file() {
        copy_file(source, target, prepared, tracker)?;
        tracker.add_item(metadata.len(), source);
    } else {
        return Err(OperationError::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("unsupported file type: {}", source.display()),
        )));
    }
    Ok(())
}

fn copy_file(
    source: &Path,
    target: &Path,
    prepared: &Prepared,
    tracker: &mut ProgressTracker<'_>,
) -> Result<(), OperationError> {
    let temporary = target
        .parent()
        .ok_or_else(|| OperationError::MissingFileName(target.to_path_buf()))?
        .join(format!(".gnil-leaf-{}", Uuid::new_v4()));
    let result = (|| {
        let source_parent = secure_fs::open_parent(source)?;
        let source_file = File::from(secure_fs::open_file_readonly(&source_parent)?);
        let metadata = source_file.metadata()?;
        validate_metadata_identity(source, &metadata, prepared)?;
        let temporary_parent = secure_fs::open_parent(&temporary)?;
        let temporary_file = File::from(secure_fs::create_file(
            &temporary_parent,
            metadata.permissions().mode() & 0o7777,
        )?);
        let mut reader = BufReader::new(source_file);
        let mut writer = BufWriter::new(temporary_file);
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            tracker.checkpoint()?;
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            writer.write_all(&buffer[..read])?;
            tracker.add_bytes(read as u64, source);
        }
        writer.flush()?;
        writer.get_ref().sync_all()?;
        writer
            .get_ref()
            .set_permissions(fs::Permissions::from_mode(
                metadata.permissions().mode() & 0o7777,
            ))?;
        let target_parent = secure_fs::open_parent(target)?;
        secure_fs::rename_noreplace(&temporary_parent, &target_parent)?;
        Ok(())
    })();
    if result.is_err() && super::path_lexists(&temporary) {
        let _ = super::remove_path(&temporary);
    }
    result
}

fn delete_paths(paths: &[PathBuf], prepared: &Prepared, allow_skip: bool, job: &JobContext) -> Run {
    let mut tracker = ProgressTracker::new(job, prepared.total);
    let mut affected = Vec::new();
    let mut skipped = Vec::new();
    let mut issues = Vec::new();
    for path in paths {
        let totals = prepared.roots.get(path).copied().unwrap_or_default();
        loop {
            match controlled_remove(path, prepared, &mut tracker, true) {
                Ok(()) => {
                    affected.push(path.clone());
                    tracker.commit_root(totals, path);
                    break;
                }
                Err(OperationError::Cancelled) => {
                    issues.push(OperationIssue {
                        path: path.clone(),
                        message: "Deletion cancelled; this item may be partially removed".into(),
                    });
                    return Run::Cancelled(OperationOutcome {
                        affected_paths: affected,
                        skipped_paths: skipped,
                        issues,
                        undo: None,
                    });
                }
                Err(error) => match request_error(path, &error, allow_skip, job) {
                    JobDecision::Retry => {}
                    JobDecision::Skip if allow_skip => {
                        skipped.push(path.clone());
                        break;
                    }
                    _ => {
                        return Run::Cancelled(OperationOutcome {
                            affected_paths: affected,
                            skipped_paths: skipped,
                            issues,
                            undo: None,
                        });
                    }
                },
            }
        }
    }
    Run::Completed(OperationOutcome {
        affected_paths: affected,
        skipped_paths: skipped,
        issues,
        undo: None,
    })
}

fn controlled_remove(
    path: &Path,
    prepared: &Prepared,
    tracker: &mut ProgressTracker<'_>,
    report_progress: bool,
) -> Result<(), OperationError> {
    validate_source_identity(path, prepared)?;
    let job = tracker.job;
    let entry_bytes = &prepared.entry_bytes;
    let mut checkpoint = || {
        if job.checkpoint() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "operation cancelled",
            ))
        }
    };
    let mut removed = |removed_path: &Path| {
        if report_progress {
            let bytes = entry_bytes.get(removed_path).copied().unwrap_or(0);
            tracker.add_item(bytes, removed_path);
        }
    };
    secure_fs::remove_tree_controlled(path, &mut checkpoint, &mut removed).map_err(|error| {
        if error.kind() == io::ErrorKind::Interrupted && job.is_cancelled() {
            OperationError::Cancelled
        } else {
            OperationError::Io(error)
        }
    })
}

fn trash_paths(paths: &[PathBuf], prepared: &Prepared, job: &JobContext) -> Run {
    let mut tracker = ProgressTracker::new(job, prepared.total);
    let mut affected = Vec::new();
    let mut skipped = Vec::new();
    let mut issues = Vec::new();
    for path in paths {
        let totals = prepared.roots.get(path).copied().unwrap_or_default();
        loop {
            if let Err(OperationError::Cancelled) = tracker.checkpoint() {
                return Run::Cancelled(trash_outcome(affected, skipped, issues));
            }
            if let Err(error) = validate_source_identity(path, prepared) {
                match request_error(path, &error, true, job) {
                    JobDecision::Retry => {}
                    JobDecision::Skip => {
                        skipped.push(path.clone());
                        issues.push(OperationIssue {
                            path: path.clone(),
                            message: "Skipped because the source changed after preparation".into(),
                        });
                        break;
                    }
                    _ => return Run::Cancelled(trash_outcome(affected, skipped, issues)),
                }
            }
            match trash::delete(path) {
                Ok(()) => {
                    affected.push(path.clone());
                    tracker.commit_root(totals, path);
                    break;
                }
                Err(error) => match job.request(JobPrompt::IoError {
                    path: path.clone(),
                    message: error.to_string(),
                    can_skip: true,
                }) {
                    JobDecision::Retry => {}
                    JobDecision::Skip => {
                        skipped.push(path.clone());
                        issues.push(OperationIssue {
                            path: path.clone(),
                            message: error.to_string(),
                        });
                        break;
                    }
                    _ => return Run::Cancelled(trash_outcome(affected, skipped, issues)),
                },
            }
        }
    }
    Run::Completed(trash_outcome(affected, skipped, issues))
}

fn trash_outcome(
    affected_paths: Vec<PathBuf>,
    skipped_paths: Vec<PathBuf>,
    issues: Vec<OperationIssue>,
) -> OperationOutcome {
    OperationOutcome {
        undo: (!affected_paths.is_empty()).then(|| UndoRecord {
            label: "Move to trash".into(),
            kind: UndoKind::RestoreTrash {
                original_paths: affected_paths.clone(),
            },
        }),
        affected_paths,
        skipped_paths,
        issues,
    }
}

fn restore_entries(
    executor: &OperationExecutor,
    entries: &[TrashEntryRef],
    replace_existing: bool,
    prepared: &Prepared,
    job: &JobContext,
) -> Run {
    let mut tracker = ProgressTracker::new(job, prepared.total);
    let mut outcome = OperationOutcome::default();
    for entry in entries {
        let totals = prepared
            .roots
            .get(&entry.trashed_path)
            .copied()
            .unwrap_or_default();
        loop {
            if let Err(OperationError::Cancelled) = tracker.checkpoint() {
                return Run::Cancelled(outcome);
            }
            match executor.restore_trash(
                std::slice::from_ref(entry),
                replace_existing,
                job.cancelled_flag(),
            ) {
                Ok(entry_outcome) => {
                    outcome.affected_paths.extend(entry_outcome.affected_paths);
                    tracker.commit_root(totals, &entry.trashed_path);
                    break;
                }
                Err(error) => match request_error(&entry.original_path, &error, true, job) {
                    JobDecision::Retry => {}
                    JobDecision::Skip => {
                        outcome.skipped_paths.push(entry.original_path.clone());
                        break;
                    }
                    _ => return Run::Cancelled(outcome),
                },
            }
        }
    }
    Run::Completed(outcome)
}

fn purge_entries(entries: &[TrashEntryRef], prepared: &Prepared, job: &JobContext) -> Run {
    let paths = entries
        .iter()
        .map(|entry| entry.trashed_path.clone())
        .collect::<Vec<_>>();
    let mut run = delete_paths(&paths, prepared, false, job);
    if let Run::Completed(outcome) = &mut run {
        for entry in entries {
            if entry.info_path.exists()
                && let Err(error) = fs::remove_file(&entry.info_path)
            {
                outcome.issues.push(OperationIssue {
                    path: entry.info_path.clone(),
                    message: error.to_string(),
                });
            }
        }
    }
    run
}

fn request_error(
    path: &Path,
    error: &OperationError,
    can_skip: bool,
    job: &JobContext,
) -> JobDecision {
    if matches!(error, OperationError::Cancelled) {
        return JobDecision::Cancel;
    }
    job.request(JobPrompt::IoError {
        path: path.to_path_buf(),
        message: error.to_string(),
        can_skip,
    })
}

fn validate_source_identity(path: &Path, prepared: &Prepared) -> Result<(), OperationError> {
    let metadata = fs::symlink_metadata(path)?;
    validate_metadata_identity(path, &metadata, prepared)
}

fn validate_metadata_identity(
    path: &Path,
    metadata: &fs::Metadata,
    prepared: &Prepared,
) -> Result<(), OperationError> {
    let Some(expected) = prepared.identities.get(path) else {
        return Ok(());
    };
    if metadata.dev() == expected.device && metadata.ino() == expected.inode {
        return Ok(());
    }
    Err(OperationError::Io(io::Error::other(format!(
        "source changed after preparation: {}",
        path.display()
    ))))
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread, time::Duration};

    use gnil_core::{JobEvent, JobPriority};

    use crate::TaskScheduler;

    use super::*;

    #[test]
    fn copy_job_reports_determinate_progress_and_cleans_staging() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(source.join("large.bin"), vec![7_u8; 2 * 1024 * 1024]).unwrap();
        let operation = FsOperation::Copy {
            sources: vec![source.clone()],
            destination: destination.clone(),
            conflict: ConflictDecision::Ask,
        };
        let scheduler = TaskScheduler::new(1);
        let handle = scheduler.submit(JobPriority::Foreground, move |job| {
            OperationExecutor.execute_job(&operation, &job)
        });
        assert!(matches!(
            handle.completion.recv().unwrap(),
            JobResult::Completed(_)
        ));
        let progress = handle
            .events
            .try_iter()
            .filter_map(|event| match event {
                JobEvent::Progress { progress, .. } if progress.total_items.is_some() => {
                    Some(progress)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!progress.is_empty());
        assert!(progress.windows(2).all(|pair| {
            pair[0].completed_items <= pair[1].completed_items
                && pair[0].completed_bytes <= pair[1].completed_bytes
        }));
        assert!(destination.join("source/large.bin").exists());
        assert!(fs::read_dir(&destination).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".gnil-part-")
        }));
    }

    #[test]
    fn conflict_waits_for_a_typed_decision() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("note.txt");
        let destination = root.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, b"new").unwrap();
        fs::write(destination.join("note.txt"), b"old").unwrap();
        let operation = FsOperation::Copy {
            sources: vec![source],
            destination: destination.clone(),
            conflict: ConflictDecision::Ask,
        };
        let scheduler = TaskScheduler::new(1);
        let handle = scheduler.submit(JobPriority::Foreground, move |job| {
            OperationExecutor.execute_job(&operation, &job)
        });
        loop {
            if matches!(handle.events.recv().unwrap(), JobEvent::Prompt { .. }) {
                break;
            }
        }
        handle.controller.respond(JobDecision::Conflict {
            decision: ConflictDecision::KeepBoth,
            apply_to_all: false,
        });
        assert!(matches!(
            handle.completion.recv().unwrap(),
            JobResult::Completed(_)
        ));
        assert_eq!(fs::read(destination.join("note.txt")).unwrap(), b"old");
        assert_eq!(fs::read(destination.join("note copy.txt")).unwrap(), b"new");
    }

    #[test]
    fn paused_copy_does_not_block_the_test_thread() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("large.bin");
        let destination = root.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, vec![1_u8; 4 * 1024 * 1024]).unwrap();
        let operation = FsOperation::Copy {
            sources: vec![source],
            destination,
            conflict: ConflictDecision::Ask,
        };
        let scheduler = TaskScheduler::new(1);
        let handle = scheduler.submit(JobPriority::Foreground, move |job| {
            OperationExecutor.execute_job(&operation, &job)
        });
        handle.controller.pause();
        let (sent, received) = mpsc::channel();
        thread::spawn(move || sent.send(()).unwrap());
        received.recv_timeout(Duration::from_millis(100)).unwrap();
        handle.controller.resume();
        let _ = handle.completion.recv().unwrap();
    }

    #[test]
    fn cancelling_a_copy_removes_its_staging_tree() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("large.bin");
        let destination = root.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, vec![3_u8; 16 * 1024 * 1024]).unwrap();
        let operation = FsOperation::Copy {
            sources: vec![source],
            destination: destination.clone(),
            conflict: ConflictDecision::Ask,
        };
        let scheduler = TaskScheduler::new(1);
        let handle = scheduler.submit(JobPriority::Foreground, move |job| {
            OperationExecutor.execute_job(&operation, &job)
        });
        loop {
            if matches!(
                handle.events.recv().unwrap(),
                JobEvent::Progress {
                    progress: JobProgress {
                        completed_bytes: 1..,
                        total_bytes: Some(_),
                        ..
                    },
                    ..
                }
            ) {
                handle.controller.cancel();
                break;
            }
        }
        assert!(matches!(
            handle.completion.recv().unwrap(),
            JobResult::Cancelled(_)
        ));
        assert!(!destination.join("large.bin").exists());
        assert!(fs::read_dir(&destination).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".gnil-part-")
        }));
    }

    #[test]
    fn a_replaced_source_is_not_copied_after_preparation() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("note.txt");
        let destination = root.path().join("destination");
        fs::create_dir(&destination).unwrap();
        fs::write(&source, b"prepared source").unwrap();
        fs::write(destination.join("note.txt"), b"existing destination").unwrap();
        let operation = FsOperation::Copy {
            sources: vec![source.clone()],
            destination: destination.clone(),
            conflict: ConflictDecision::Ask,
        };
        let scheduler = TaskScheduler::new(1);
        let handle = scheduler.submit(JobPriority::Foreground, move |job| {
            OperationExecutor.execute_job(&operation, &job)
        });
        loop {
            if matches!(handle.events.recv().unwrap(), JobEvent::Prompt { .. }) {
                break;
            }
        }
        fs::rename(&source, root.path().join("original-note.txt")).unwrap();
        fs::write(&source, b"replacement source").unwrap();
        handle.controller.respond(JobDecision::Conflict {
            decision: ConflictDecision::KeepBoth,
            apply_to_all: false,
        });
        loop {
            if matches!(handle.events.recv().unwrap(), JobEvent::Prompt { .. }) {
                break;
            }
        }
        handle.controller.respond(JobDecision::Skip);

        assert!(matches!(
            handle.completion.recv().unwrap(),
            JobResult::CompletedWithIssues(_)
        ));
        assert_eq!(
            fs::read(destination.join("note.txt")).unwrap(),
            b"existing destination"
        );
        assert!(!destination.join("note copy.txt").exists());
    }

    #[test]
    fn move_reports_a_committed_copy_when_source_removal_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        let source_directory = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir(&source_directory).unwrap();
        fs::create_dir(&destination).unwrap();
        let source = source_directory.join("note.txt");
        fs::write(&source, b"new contents").unwrap();
        fs::write(destination.join("note.txt"), b"old contents").unwrap();
        fs::set_permissions(&source_directory, fs::Permissions::from_mode(0o555)).unwrap();
        let operation = FsOperation::Move {
            sources: vec![source.clone()],
            destination: destination.clone(),
            conflict: ConflictDecision::Replace,
        };
        let scheduler = TaskScheduler::new(1);
        let handle = scheduler.submit(JobPriority::Foreground, move |job| {
            OperationExecutor.execute_job(&operation, &job)
        });
        loop {
            if matches!(
                handle.events.recv().unwrap(),
                JobEvent::Prompt {
                    prompt: JobPrompt::IoError { .. },
                    ..
                }
            ) {
                break;
            }
        }
        handle.controller.respond(JobDecision::Skip);
        let outcome = match handle.completion.recv().unwrap() {
            JobResult::CompletedWithIssues(outcome) => outcome,
            other => panic!("expected a partial move outcome, got {other:?}"),
        };
        fs::set_permissions(&source_directory, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(
            fs::read(destination.join("note.txt")).unwrap(),
            b"new contents"
        );
        assert!(source.exists());
        assert_eq!(outcome.affected_paths, [destination.join("note.txt")]);
        assert_eq!(outcome.skipped_paths, [source]);
        assert_eq!(outcome.issues.len(), 1);
    }
}
