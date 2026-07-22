use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::UNIX_EPOCH,
};

use gnil_core::{
    ConflictDecision, FileFingerprint, FsOperation, JobProgress, OperationOutcome,
    PermissionChange, PermissionUndo, RenamePair, TrashEntryRef, UndoKind, UndoRecord,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum OperationError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("destination already exists: {0}")]
    Conflict(PathBuf),
    #[error("refusing to copy a directory into itself: {0}")]
    RecursiveDestination(PathBuf),
    #[error("path has no file name: {0}")]
    MissingFileName(PathBuf),
    #[error("invalid permission mode: {0:o}")]
    InvalidMode(u32),
    #[error("refusing to chmod symlink: {0}")]
    SymlinkPermissions(PathBuf),
    #[error("invalid bulk rename: {0}")]
    InvalidRename(String),
    #[error("operation failed and rollback was incomplete: {0}")]
    RollbackFailed(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("trash operation failed: {0}")]
    Trash(String),
    #[error(transparent)]
    Archive(#[from] crate::archive::ArchiveError),
}

#[derive(Default)]
pub struct OperationExecutor;

// Keep an executor value in the public API so policy and progress hooks can be added without
// changing every caller when the MVP grows beyond local operations.
#[allow(clippy::unused_self)]
impl OperationExecutor {
    pub fn execute(
        &self,
        operation: &FsOperation,
        cancelled: &AtomicBool,
    ) -> Result<OperationOutcome, OperationError> {
        self.execute_with_progress(operation, cancelled, &mut |_| {})
    }

    pub fn execute_with_progress(
        &self,
        operation: &FsOperation,
        cancelled: &AtomicBool,
        progress: &mut dyn FnMut(JobProgress),
    ) -> Result<OperationOutcome, OperationError> {
        match operation {
            FsOperation::CreateFile { path } => self.create_file(path),
            FsOperation::CreateDirectory { path } => self.create_directory(path),
            FsOperation::Rename { from, to } => self.rename(from, to),
            FsOperation::BulkRename { pairs } => self.bulk_rename(pairs, cancelled),
            FsOperation::CreateSymlink { link_path, target } => {
                self.create_symlink(link_path, target)
            }
            FsOperation::SetPermissions { paths, change } => self.set_permissions(paths, *change),
            FsOperation::Copy {
                sources,
                destination,
                conflict,
            } => self.copy(sources, destination, *conflict, cancelled),
            FsOperation::Move {
                sources,
                destination,
                conflict,
            } => self.move_paths(sources, destination, *conflict, cancelled),
            FsOperation::Trash { paths } => self.trash(paths),
            FsOperation::DeletePermanently { paths } => self.delete_permanently(paths, cancelled),
            FsOperation::RestoreTrash {
                entries,
                replace_existing,
            } => self.restore_trash(entries, *replace_existing, cancelled),
            FsOperation::PurgeTrash { entries } => self.purge_trash(entries, cancelled),
            FsOperation::ExtractArchives {
                sources,
                destination,
            } => Ok(crate::archive::extract_archives(
                sources,
                destination,
                cancelled,
                progress,
            )?),
        }
    }

    fn create_file(&self, path: &Path) -> Result<OperationOutcome, OperationError> {
        OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(created_outcome("Create file", vec![path.to_path_buf()]))
    }

    fn create_directory(&self, path: &Path) -> Result<OperationOutcome, OperationError> {
        fs::create_dir(path)?;
        Ok(created_outcome("Create folder", vec![path.to_path_buf()]))
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<OperationOutcome, OperationError> {
        if path_lexists(to) {
            return Err(OperationError::Conflict(to.to_path_buf()));
        }
        fs::rename(from, to)?;
        Ok(OperationOutcome {
            affected_paths: vec![to.to_path_buf()],
            skipped_paths: Vec::new(),
            undo: Some(UndoRecord {
                label: "Rename".into(),
                kind: UndoKind::RenameBack {
                    from: to.to_path_buf(),
                    to: from.to_path_buf(),
                },
            }),
        })
    }

    fn create_symlink(
        &self,
        link_path: &Path,
        target: &Path,
    ) -> Result<OperationOutcome, OperationError> {
        if path_lexists(link_path) {
            return Err(OperationError::Conflict(link_path.to_path_buf()));
        }
        std::os::unix::fs::symlink(target, link_path)?;
        Ok(OperationOutcome {
            affected_paths: vec![link_path.to_path_buf()],
            skipped_paths: Vec::new(),
            undo: Some(UndoRecord {
                label: "Create symlink".into(),
                kind: UndoKind::RemoveSymlink {
                    link_path: link_path.to_path_buf(),
                    target: target.to_path_buf(),
                },
            }),
        })
    }

    fn set_permissions(
        &self,
        paths: &[PathBuf],
        change: PermissionChange,
    ) -> Result<OperationOutcome, OperationError> {
        const MODE_MASK: u32 = 0o7777;
        let (set, clear) = match change {
            PermissionChange::Exact(mode) if mode <= MODE_MASK => (Some(mode), None),
            PermissionChange::Exact(mode) => return Err(OperationError::InvalidMode(mode)),
            PermissionChange::Mask { set, clear }
                if set <= MODE_MASK && clear <= MODE_MASK && set & clear == 0 =>
            {
                (None, Some((set, clear)))
            }
            PermissionChange::Mask { set, clear } => {
                return Err(OperationError::InvalidMode(set | clear));
            }
        };

        let mut entries = Vec::with_capacity(paths.len());
        for path in paths {
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() {
                return Err(OperationError::SymlinkPermissions(path.clone()));
            }
            let before = metadata.permissions().mode() & MODE_MASK;
            let after = set.unwrap_or_else(|| {
                let (bits_to_set, bits_to_clear) = clear.expect("validated mask");
                (before | bits_to_set) & !bits_to_clear & MODE_MASK
            });
            if before != after {
                entries.push(PermissionUndo {
                    path: path.clone(),
                    before,
                    after,
                });
            }
        }

        let mut changed = Vec::new();
        for entry in &entries {
            if let Err(error) =
                fs::set_permissions(&entry.path, fs::Permissions::from_mode(entry.after))
            {
                let rollback_errors = rollback_permissions(&changed);
                if rollback_errors.is_empty() {
                    return Err(error.into());
                }
                return Err(OperationError::RollbackFailed(rollback_errors.join("; ")));
            }
            changed.push(entry.clone());
        }

        Ok(OperationOutcome {
            affected_paths: entries.iter().map(|entry| entry.path.clone()).collect(),
            skipped_paths: Vec::new(),
            undo: (!entries.is_empty()).then(|| UndoRecord {
                label: "Change permissions".into(),
                kind: UndoKind::RestorePermissions { entries },
            }),
        })
    }

    fn bulk_rename(
        &self,
        pairs: &[RenamePair],
        cancelled: &AtomicBool,
    ) -> Result<OperationOutcome, OperationError> {
        let pairs = preflight_rename_pairs(pairs)?;
        if pairs.is_empty() {
            return Ok(OperationOutcome::default());
        }

        let mut staged = Vec::with_capacity(pairs.len());
        for pair in &pairs {
            if let Err(error) = ensure_not_cancelled(cancelled) {
                let rollback_errors = rollback_staged(&staged);
                if rollback_errors.is_empty() {
                    return Err(error);
                }
                return Err(OperationError::RollbackFailed(rollback_errors.join("; ")));
            }
            let parent = pair
                .from
                .parent()
                .ok_or_else(|| OperationError::MissingFileName(pair.from.clone()))?;
            let temporary = unique_temporary_path(parent);
            if let Err(error) = fs::rename(&pair.from, &temporary) {
                let rollback_errors = rollback_staged(&staged);
                if rollback_errors.is_empty() {
                    return Err(error.into());
                }
                return Err(OperationError::RollbackFailed(rollback_errors.join("; ")));
            }
            staged.push((pair.clone(), temporary));
        }

        for (completed, (pair, temporary)) in staged.iter().enumerate() {
            if let Err(error) = ensure_not_cancelled(cancelled) {
                let rollback_errors = rollback_bulk_rename(&staged, completed);
                if rollback_errors.is_empty() {
                    return Err(error);
                }
                return Err(OperationError::RollbackFailed(rollback_errors.join("; ")));
            }
            if let Err(error) = fs::rename(temporary, &pair.to) {
                let rollback_errors = rollback_bulk_rename(&staged, completed);
                if rollback_errors.is_empty() {
                    return Err(error.into());
                }
                return Err(OperationError::RollbackFailed(rollback_errors.join("; ")));
            }
        }

        let undo_pairs = pairs
            .iter()
            .map(|pair| RenamePair {
                from: pair.to.clone(),
                to: pair.from.clone(),
            })
            .collect();
        Ok(OperationOutcome {
            affected_paths: pairs.iter().map(|pair| pair.to.clone()).collect(),
            skipped_paths: Vec::new(),
            undo: Some(UndoRecord {
                label: "Bulk rename".into(),
                kind: UndoKind::BulkRenameBack { pairs: undo_pairs },
            }),
        })
    }

    fn copy(
        &self,
        sources: &[PathBuf],
        destination: &Path,
        conflict: ConflictDecision,
        cancelled: &AtomicBool,
    ) -> Result<OperationOutcome, OperationError> {
        let mut affected = Vec::new();
        let mut skipped = Vec::new();
        for source in sources {
            ensure_not_cancelled(cancelled)?;
            let name = source
                .file_name()
                .ok_or_else(|| OperationError::MissingFileName(source.clone()))?;
            let requested = destination.join(name);
            let Some(target) = resolve_conflict(&requested, conflict)? else {
                skipped.push(source.clone());
                continue;
            };
            guard_recursive_destination(source, &target)?;
            copy_path(source, &target, cancelled)?;
            affected.push(target);
        }
        let fingerprints = affected
            .iter()
            .filter_map(|path| fingerprint(path).map(|value| (path.clone(), value)))
            .collect();
        Ok(OperationOutcome {
            affected_paths: affected,
            skipped_paths: skipped,
            undo: Some(UndoRecord {
                label: "Copy".into(),
                kind: UndoKind::RemoveCreated {
                    paths: fingerprints,
                },
            }),
        })
    }

    fn move_paths(
        &self,
        sources: &[PathBuf],
        destination: &Path,
        conflict: ConflictDecision,
        cancelled: &AtomicBool,
    ) -> Result<OperationOutcome, OperationError> {
        let mut affected = Vec::new();
        let mut skipped = Vec::new();
        let mut undo_pairs = Vec::new();
        for source in sources {
            ensure_not_cancelled(cancelled)?;
            let name = source
                .file_name()
                .ok_or_else(|| OperationError::MissingFileName(source.clone()))?;
            let requested = destination.join(name);
            let Some(target) = resolve_conflict(&requested, conflict)? else {
                skipped.push(source.clone());
                continue;
            };
            guard_recursive_destination(source, &target)?;
            match fs::rename(source, &target) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
                    copy_path(source, &target, cancelled)?;
                    remove_path(source)?;
                }
                Err(error) => return Err(error.into()),
            }
            affected.push(target.clone());
            undo_pairs.push((target, source.clone()));
        }
        Ok(OperationOutcome {
            affected_paths: affected,
            skipped_paths: skipped,
            undo: Some(UndoRecord {
                label: "Move".into(),
                kind: UndoKind::MoveBack { pairs: undo_pairs },
            }),
        })
    }

    fn trash(&self, paths: &[PathBuf]) -> Result<OperationOutcome, OperationError> {
        trash::delete_all(paths).map_err(|error| OperationError::Trash(error.to_string()))?;
        Ok(OperationOutcome {
            affected_paths: paths.to_vec(),
            skipped_paths: Vec::new(),
            undo: Some(UndoRecord {
                label: "Move to trash".into(),
                kind: UndoKind::RestoreTrash {
                    original_paths: paths.to_vec(),
                },
            }),
        })
    }

    fn delete_permanently(
        &self,
        paths: &[PathBuf],
        cancelled: &AtomicBool,
    ) -> Result<OperationOutcome, OperationError> {
        for path in paths {
            ensure_not_cancelled(cancelled)?;
            remove_path(path)?;
        }
        Ok(OperationOutcome {
            affected_paths: paths.to_vec(),
            skipped_paths: Vec::new(),
            undo: None,
        })
    }

    fn restore_trash(
        &self,
        entries: &[TrashEntryRef],
        replace_existing: bool,
        cancelled: &AtomicBool,
    ) -> Result<OperationOutcome, OperationError> {
        for entry in entries {
            fs::symlink_metadata(&entry.trashed_path)?;
        }
        if !replace_existing
            && let Some(entry) = entries
                .iter()
                .find(|entry| path_lexists(&entry.original_path))
        {
            return Err(OperationError::Conflict(entry.original_path.clone()));
        }
        let mut affected = Vec::with_capacity(entries.len());
        for entry in entries {
            ensure_not_cancelled(cancelled)?;
            if replace_existing && path_lexists(&entry.original_path) {
                remove_path(&entry.original_path)?;
            }
            if let Some(parent) = entry.original_path.parent() {
                fs::create_dir_all(parent)?;
            }
            match fs::rename(&entry.trashed_path, &entry.original_path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
                    copy_path(&entry.trashed_path, &entry.original_path, cancelled)?;
                    remove_path(&entry.trashed_path)?;
                }
                Err(error) => return Err(error.into()),
            }
            if entry.info_path.exists() {
                fs::remove_file(&entry.info_path)?;
            }
            affected.push(entry.original_path.clone());
        }
        Ok(OperationOutcome {
            affected_paths: affected,
            skipped_paths: Vec::new(),
            undo: None,
        })
    }

    fn purge_trash(
        &self,
        entries: &[TrashEntryRef],
        cancelled: &AtomicBool,
    ) -> Result<OperationOutcome, OperationError> {
        let mut affected = Vec::with_capacity(entries.len());
        for entry in entries {
            ensure_not_cancelled(cancelled)?;
            if path_lexists(&entry.trashed_path) {
                remove_path(&entry.trashed_path)?;
            }
            if entry.info_path.exists() {
                fs::remove_file(&entry.info_path)?;
            }
            affected.push(entry.trashed_path.clone());
        }
        Ok(OperationOutcome {
            affected_paths: affected,
            skipped_paths: Vec::new(),
            undo: None,
        })
    }

    pub fn undo(&self, record: &UndoRecord) -> Result<(), OperationError> {
        match &record.kind {
            UndoKind::RemoveCreated { paths } => {
                for (path, expected) in paths.iter().rev() {
                    if fingerprint(path).as_ref() != Some(expected) {
                        return Err(OperationError::Conflict(path.clone()));
                    }
                    remove_path(path)?;
                }
            }
            UndoKind::RenameBack { from, to } => {
                if path_lexists(to) {
                    return Err(OperationError::Conflict(to.clone()));
                }
                fs::rename(from, to)?;
            }
            UndoKind::BulkRenameBack { pairs } => {
                self.bulk_rename(pairs, &AtomicBool::new(false))?;
            }
            UndoKind::RemoveSymlink { link_path, target } => {
                let metadata = fs::symlink_metadata(link_path)
                    .map_err(|_| OperationError::Conflict(link_path.clone()))?;
                if !metadata.file_type().is_symlink() || fs::read_link(link_path)? != *target {
                    return Err(OperationError::Conflict(link_path.clone()));
                }
                fs::remove_file(link_path)?;
            }
            UndoKind::RestorePermissions { entries } => {
                for entry in entries {
                    let metadata = fs::symlink_metadata(&entry.path)?;
                    if metadata.file_type().is_symlink()
                        || metadata.permissions().mode() & 0o7777 != entry.after
                    {
                        return Err(OperationError::Conflict(entry.path.clone()));
                    }
                }
                for entry in entries {
                    fs::set_permissions(&entry.path, fs::Permissions::from_mode(entry.before))?;
                }
            }
            UndoKind::MoveBack { pairs } => {
                for (from, to) in pairs.iter().rev() {
                    if path_lexists(to) {
                        return Err(OperationError::Conflict(to.clone()));
                    }
                    fs::rename(from, to)?;
                }
            }
            UndoKind::RestoreTrash { original_paths } => restore_from_trash(original_paths)?,
            UndoKind::RemoveExtracted { trees } => {
                crate::archive::remove_extracted_trees(trees)?;
            }
        }
        Ok(())
    }
}

fn created_outcome(label: &str, paths: Vec<PathBuf>) -> OperationOutcome {
    let fingerprints = paths
        .iter()
        .filter_map(|path| fingerprint(path).map(|value| (path.clone(), value)))
        .collect();
    OperationOutcome {
        affected_paths: paths,
        skipped_paths: Vec::new(),
        undo: Some(UndoRecord {
            label: label.into(),
            kind: UndoKind::RemoveCreated {
                paths: fingerprints,
            },
        }),
    }
}

fn path_lexists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn preflight_rename_pairs(pairs: &[RenamePair]) -> Result<Vec<RenamePair>, OperationError> {
    let pairs: Vec<_> = pairs
        .iter()
        .filter(|pair| pair.from != pair.to)
        .cloned()
        .collect();
    let mut sources = HashSet::with_capacity(pairs.len());
    let mut destinations = HashSet::with_capacity(pairs.len());
    let mut common_parent: Option<&Path> = None;

    for pair in &pairs {
        let from_parent = pair
            .from
            .parent()
            .ok_or_else(|| OperationError::MissingFileName(pair.from.clone()))?;
        let to_parent = pair
            .to
            .parent()
            .ok_or_else(|| OperationError::MissingFileName(pair.to.clone()))?;
        if pair.from.file_name().is_none() || pair.to.file_name().is_none() {
            return Err(OperationError::InvalidRename(
                "every path must have a file name".into(),
            ));
        }
        if from_parent != to_parent || common_parent.is_some_and(|parent| parent != from_parent) {
            return Err(OperationError::InvalidRename(
                "bulk rename is limited to one directory".into(),
            ));
        }
        common_parent = Some(from_parent);
        if !path_lexists(&pair.from) {
            return Err(OperationError::InvalidRename(format!(
                "source does not exist: {}",
                pair.from.display()
            )));
        }
        if !sources.insert(pair.from.clone()) {
            return Err(OperationError::InvalidRename(format!(
                "duplicate source: {}",
                pair.from.display()
            )));
        }
        if !destinations.insert(pair.to.clone()) {
            return Err(OperationError::InvalidRename(format!(
                "duplicate destination: {}",
                pair.to.display()
            )));
        }
    }

    for destination in &destinations {
        if path_lexists(destination) && !sources.contains(destination) {
            return Err(OperationError::Conflict(destination.clone()));
        }
    }
    Ok(pairs)
}

fn unique_temporary_path(parent: &Path) -> PathBuf {
    loop {
        let candidate = parent.join(format!(".gnil-rename-{}", Uuid::new_v4()));
        if !path_lexists(&candidate) {
            return candidate;
        }
    }
}

fn rollback_staged(staged: &[(RenamePair, PathBuf)]) -> Vec<String> {
    let mut errors = Vec::new();
    for (pair, temporary) in staged.iter().rev() {
        if path_lexists(temporary) {
            if let Err(error) = fs::rename(temporary, &pair.from) {
                errors.push(format!("{}: {error}", pair.from.display()));
            }
        }
    }
    errors
}

fn rollback_bulk_rename(staged: &[(RenamePair, PathBuf)], completed: usize) -> Vec<String> {
    let mut errors = Vec::new();
    for (pair, temporary) in staged[..completed].iter().rev() {
        if let Err(error) = fs::rename(&pair.to, temporary) {
            errors.push(format!("{}: {error}", pair.to.display()));
        }
    }
    errors.extend(rollback_staged(staged));
    errors
}

fn rollback_permissions(entries: &[PermissionUndo]) -> Vec<String> {
    let mut errors = Vec::new();
    for entry in entries.iter().rev() {
        if let Err(error) =
            fs::set_permissions(&entry.path, fs::Permissions::from_mode(entry.before))
        {
            errors.push(format!("{}: {error}", entry.path.display()));
        }
    }
    errors
}

fn resolve_conflict(
    requested: &Path,
    decision: ConflictDecision,
) -> Result<Option<PathBuf>, OperationError> {
    if !path_lexists(requested) {
        return Ok(Some(requested.to_path_buf()));
    }
    match decision {
        ConflictDecision::Ask => Err(OperationError::Conflict(requested.to_path_buf())),
        ConflictDecision::Skip => Ok(None),
        ConflictDecision::KeepBoth => Ok(Some(unique_copy_path(requested))),
        ConflictDecision::Replace => {
            remove_path(requested)?;
            Ok(Some(requested.to_path_buf()))
        }
        ConflictDecision::MergeDirectory => Ok(Some(requested.to_path_buf())),
    }
}

fn unique_copy_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path.extension().map(|value| value.to_string_lossy());
    for index in 1.. {
        let suffix = if index == 1 {
            " copy".to_owned()
        } else {
            format!(" copy {index}")
        };
        let mut name = format!("{stem}{suffix}");
        if let Some(extension) = &extension {
            name.push('.');
            name.push_str(extension);
        }
        let candidate = parent.join(name);
        if !path_lexists(&candidate) {
            return candidate;
        }
    }
    unreachable!("unbounded name search must produce a candidate")
}

fn guard_recursive_destination(source: &Path, target: &Path) -> Result<(), OperationError> {
    if fs::symlink_metadata(source).is_ok_and(|metadata| metadata.is_dir()) {
        let source = fs::canonicalize(source)?;
        let prospective_parent = target.parent().unwrap_or(target);
        if fs::canonicalize(prospective_parent).is_ok_and(|parent| parent.starts_with(&source)) {
            return Err(OperationError::RecursiveDestination(target.to_path_buf()));
        }
    }
    Ok(())
}

fn copy_path(source: &Path, target: &Path, cancelled: &AtomicBool) -> Result<(), OperationError> {
    ensure_not_cancelled(cancelled)?;
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        copy_symlink(source, target)?;
    } else if metadata.is_dir() {
        fs::create_dir_all(target)?;
        for item in fs::read_dir(source)? {
            let item = item?;
            copy_path(&item.path(), &target.join(item.file_name()), cancelled)?;
        }
        fs::set_permissions(target, metadata.permissions())?;
    } else if metadata.is_file() {
        copy_file_atomic(source, target, cancelled)?;
    }
    Ok(())
}

fn copy_file_atomic(
    source: &Path,
    target: &Path,
    cancelled: &AtomicBool,
) -> Result<(), OperationError> {
    if path_lexists(target) {
        return Err(OperationError::Conflict(target.to_path_buf()));
    }
    let parent = target
        .parent()
        .ok_or_else(|| OperationError::MissingFileName(target.to_path_buf()))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".gnil-part-{}", Uuid::new_v4()));
    let result = (|| {
        let metadata = fs::metadata(source)?;
        let mut reader = BufReader::new(File::open(source)?);
        let mut writer = BufWriter::new(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?,
        );
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            ensure_not_cancelled(cancelled)?;
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            writer.write_all(&buffer[..read])?;
        }
        writer.flush()?;
        writer.get_ref().sync_all()?;
        fs::set_permissions(&temporary, metadata.permissions())?;
        fs::rename(&temporary, target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
fn copy_symlink(source: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, target)
}

#[cfg(not(unix))]
fn copy_symlink(_source: &Path, _target: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symlink copying is unsupported",
    ))
}

fn remove_path(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn fingerprint(path: &Path) -> Option<FileFingerprint> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let modified_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok());
    Some(FileFingerprint {
        len: metadata.len(),
        modified_unix_ms,
    })
}

fn restore_from_trash(original_paths: &[PathBuf]) -> Result<(), OperationError> {
    let items =
        trash::os_limited::list().map_err(|error| OperationError::Trash(error.to_string()))?;
    let mut matching = Vec::with_capacity(original_paths.len());
    for original in original_paths {
        let item = items
            .iter()
            .filter(|item| item.original_path() == *original)
            .max_by_key(|item| item.time_deleted)
            .cloned();
        if let Some(item) = item {
            matching.push(item);
        }
    }
    if matching.len() != original_paths.len() {
        return Err(OperationError::Trash(
            "unable to identify every item in trash".into(),
        ));
    }
    trash::os_limited::restore_all(matching)
        .map_err(|error| OperationError::Trash(error.to_string()))
}

fn ensure_not_cancelled(cancelled: &AtomicBool) -> Result<(), OperationError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(OperationError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    #[test]
    fn copy_uses_safe_suffix_and_undo_removes_unchanged_copy() {
        let root = tempfile::tempdir().unwrap();
        let source_dir = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let source = source_dir.join("notes.txt");
        fs::write(&source, b"hello").unwrap();
        fs::write(destination.join("notes.txt"), b"existing").unwrap();
        let executor = OperationExecutor;
        let outcome = executor
            .execute(
                &FsOperation::Copy {
                    sources: vec![source],
                    destination: destination.clone(),
                    conflict: ConflictDecision::KeepBoth,
                },
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(
            outcome.affected_paths[0],
            destination.join("notes copy.txt")
        );
        executor.undo(outcome.undo.as_ref().unwrap()).unwrap();
        assert!(!destination.join("notes copy.txt").exists());
    }

    #[test]
    fn trash_restore_detects_conflict_and_can_replace_explicitly() {
        let root = tempfile::tempdir().unwrap();
        let trash_root = root.path().join("Trash");
        let trashed = trash_root.join("files/note.txt");
        let info = trash_root.join("info/note.txt.trashinfo");
        let original = root.path().join("original/note.txt");
        fs::create_dir_all(trashed.parent().unwrap()).unwrap();
        fs::create_dir_all(info.parent().unwrap()).unwrap();
        fs::create_dir_all(original.parent().unwrap()).unwrap();
        fs::write(&trashed, b"restored").unwrap();
        fs::write(&info, b"metadata").unwrap();
        fs::write(&original, b"existing").unwrap();
        let entry = TrashEntryRef {
            info_path: info.clone(),
            trashed_path: trashed.clone(),
            original_path: original.clone(),
        };
        let cancelled = AtomicBool::new(false);
        let conflict = OperationExecutor.execute(
            &FsOperation::RestoreTrash {
                entries: vec![entry.clone()],
                replace_existing: false,
            },
            &cancelled,
        );
        assert!(matches!(conflict, Err(OperationError::Conflict(path)) if path == original));
        OperationExecutor
            .execute(
                &FsOperation::RestoreTrash {
                    entries: vec![entry],
                    replace_existing: true,
                },
                &cancelled,
            )
            .unwrap();
        assert_eq!(fs::read(&original).unwrap(), b"restored");
        assert!(!trashed.exists());
        assert!(!info.exists());
    }

    #[test]
    fn purge_trash_removes_payload_and_info_without_undo() {
        let root = tempfile::tempdir().unwrap();
        let trashed = root.path().join("files/folder");
        let info = root.path().join("info/folder.trashinfo");
        fs::create_dir_all(&trashed).unwrap();
        fs::create_dir_all(info.parent().unwrap()).unwrap();
        fs::write(trashed.join("child"), b"data").unwrap();
        fs::write(&info, b"metadata").unwrap();
        let outcome = OperationExecutor
            .execute(
                &FsOperation::PurgeTrash {
                    entries: vec![TrashEntryRef {
                        info_path: info.clone(),
                        trashed_path: trashed.clone(),
                        original_path: root.path().join("original/folder"),
                    }],
                },
                &AtomicBool::new(false),
            )
            .unwrap();
        assert!(outcome.undo.is_none());
        assert!(!trashed.exists());
        assert!(!info.exists());
    }

    #[test]
    fn copy_into_descendant_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("folder");
        let destination = source.join("child");
        fs::create_dir_all(&destination).unwrap();
        let error = OperationExecutor
            .execute(
                &FsOperation::Copy {
                    sources: vec![source],
                    destination,
                    conflict: ConflictDecision::Ask,
                },
                &AtomicBool::new(false),
            )
            .unwrap_err();
        assert!(matches!(error, OperationError::RecursiveDestination(_)));
    }

    #[test]
    fn cancelled_copy_leaves_no_partial_file() {
        let root = tempfile::tempdir().unwrap();
        let source_dir = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let source = source_dir.join("large.bin");
        fs::write(&source, vec![7_u8; 2 * 1024 * 1024]).unwrap();
        let cancelled = AtomicBool::new(true);
        let result = OperationExecutor.execute(
            &FsOperation::Copy {
                sources: vec![source],
                destination: destination.clone(),
                conflict: ConflictDecision::Ask,
            },
            &cancelled,
        );
        assert!(matches!(result, Err(OperationError::Cancelled)));
        assert!(fs::read_dir(destination).unwrap().next().is_none());
    }

    #[test]
    fn undo_refuses_to_remove_modified_copy() {
        let root = tempfile::tempdir().unwrap();
        let source_dir = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let source = source_dir.join("notes.txt");
        fs::write(&source, b"hello").unwrap();
        let executor = OperationExecutor;
        let outcome = executor
            .execute(
                &FsOperation::Copy {
                    sources: vec![source],
                    destination: destination.clone(),
                    conflict: ConflictDecision::Ask,
                },
                &AtomicBool::new(false),
            )
            .unwrap();
        fs::write(destination.join("notes.txt"), b"changed after copy").unwrap();
        assert!(matches!(
            executor.undo(outcome.undo.as_ref().unwrap()),
            Err(OperationError::Conflict(_))
        ));
    }

    #[test]
    fn creates_dangling_relative_symlink_and_undoes_safely() {
        let root = tempfile::tempdir().unwrap();
        let link = root.path().join("config-link");
        let target = PathBuf::from("../missing/config");
        let executor = OperationExecutor;
        let outcome = executor
            .execute(
                &FsOperation::CreateSymlink {
                    link_path: link.clone(),
                    target: target.clone(),
                },
                &AtomicBool::new(false),
            )
            .unwrap();

        assert_eq!(fs::read_link(&link).unwrap(), target);
        assert!(!link.exists());
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        executor.undo(outcome.undo.as_ref().unwrap()).unwrap();
        assert!(fs::symlink_metadata(link).is_err());
    }

    #[test]
    fn chmod_mask_is_non_recursive_and_undoable() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("config");
        let child = directory.join("settings.toml");
        fs::create_dir(&directory).unwrap();
        fs::write(&child, b"theme = 'dark'").unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&child, fs::Permissions::from_mode(0o600)).unwrap();

        let executor = OperationExecutor;
        let outcome = executor
            .execute(
                &FsOperation::SetPermissions {
                    paths: vec![directory.clone()],
                    change: PermissionChange::Mask {
                        set: 0o050,
                        clear: 0,
                    },
                },
                &AtomicBool::new(false),
            )
            .unwrap();

        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o7777,
            0o750
        );
        assert_eq!(
            fs::metadata(&child).unwrap().permissions().mode() & 0o7777,
            0o600
        );
        executor.undo(outcome.undo.as_ref().unwrap()).unwrap();
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o7777,
            0o700
        );
    }

    #[test]
    fn chmod_refuses_to_follow_symlink() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let link = root.path().join("link");
        fs::write(&target, b"secret").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let result = OperationExecutor.execute(
            &FsOperation::SetPermissions {
                paths: vec![link],
                change: PermissionChange::Exact(0o777),
            },
            &AtomicBool::new(false),
        );

        assert!(matches!(result, Err(OperationError::SymlinkPermissions(_))));
        assert_eq!(
            fs::metadata(target).unwrap().permissions().mode() & 0o7777,
            0o600
        );
    }

    #[test]
    fn bulk_rename_handles_swap_and_undo() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("a.txt");
        let second = root.path().join("b.txt");
        fs::write(&first, b"A").unwrap();
        fs::write(&second, b"B").unwrap();
        let executor = OperationExecutor;
        let outcome = executor
            .execute(
                &FsOperation::BulkRename {
                    pairs: vec![
                        RenamePair {
                            from: first.clone(),
                            to: second.clone(),
                        },
                        RenamePair {
                            from: second.clone(),
                            to: first.clone(),
                        },
                    ],
                },
                &AtomicBool::new(false),
            )
            .unwrap();

        assert_eq!(fs::read(&first).unwrap(), b"B");
        assert_eq!(fs::read(&second).unwrap(), b"A");
        executor.undo(outcome.undo.as_ref().unwrap()).unwrap();
        assert_eq!(fs::read(first).unwrap(), b"A");
        assert_eq!(fs::read(second).unwrap(), b"B");
    }

    #[test]
    fn bulk_rename_detects_dangling_symlink_collision() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("occupied");
        fs::write(&source, b"source").unwrap();
        std::os::unix::fs::symlink("missing", &destination).unwrap();

        let result = OperationExecutor.execute(
            &FsOperation::BulkRename {
                pairs: vec![RenamePair {
                    from: source,
                    to: destination.clone(),
                }],
            },
            &AtomicBool::new(false),
        );
        assert!(matches!(result, Err(OperationError::Conflict(path)) if path == destination));
    }
}
