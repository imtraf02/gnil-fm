use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    fs::{self, File, FileTimes, OpenOptions},
    io::{self, BufReader, Read, Write},
    os::unix::fs::PermissionsExt as _,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use gnil_core::{
    ExtractedEntryFingerprint, ExtractedEntryKind, ExtractedTreeFingerprint, JobProgress,
    OperationOutcome, UndoKind, UndoRecord,
};
use libarchive2::{CallbackReader, FileType, ReadArchive};
use nix::{
    errno::Errno,
    fcntl::{RenameFlags, renameat2},
    sys::statvfs::statvfs,
};
use thiserror::Error;
use uuid::Uuid;
use xz2::read::XzDecoder;

const MAX_ENTRIES: usize = 250_000;
const MAX_UNDO_ENTRIES: usize = 50_000;
const FREE_SPACE_RESERVE: u64 = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("unsupported archive: {0}")]
    Unsupported(PathBuf),
    #[error("could not read archive; it may be damaged, unsupported, or header-encrypted: {0}")]
    Unreadable(PathBuf),
    #[error("archive contains encrypted data: {0}")]
    Encrypted(PathBuf),
    #[error("unsafe archive entry in {archive}: {reason}")]
    UnsafeEntry { archive: PathBuf, reason: String },
    #[error("archive has more than {MAX_ENTRIES} entries")]
    TooManyEntries,
    #[error("not enough free space to extract {0}")]
    NoSpace(PathBuf),
    #[error("could not commit extracted files without replacing an existing item: {0}")]
    Commit(PathBuf),
    #[error("extraction failed and rollback was incomplete: {0}")]
    Rollback(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    File,
    Directory,
    Symlink,
}

#[derive(Clone, Debug)]
struct PlannedEntry {
    path: PathBuf,
    kind: EntryKind,
    mode: u32,
    mtime: Option<SystemTime>,
    size: u64,
    link_target: Option<PathBuf>,
}

#[derive(Debug)]
struct ArchivePlan {
    source: PathBuf,
    output_name: OsString,
    entries: Vec<PlannedEntry>,
    strip_root: Option<OsString>,
    standalone: Option<StreamKind>,
}

#[derive(Clone, Copy, Debug)]
enum StreamKind {
    Gzip,
    Bzip2,
    Xz,
    Zstd,
}

struct StagedOutput {
    staging_path: PathBuf,
    requested_name: OsString,
}

#[must_use]
pub fn is_archive_candidate(path: &Path) -> bool {
    archive_kind(path).is_some()
}

#[allow(clippy::too_many_lines)]
pub fn extract_archives(
    sources: &[PathBuf],
    destination: &Path,
    cancelled: &AtomicBool,
    progress: &mut dyn FnMut(JobProgress),
) -> Result<OperationOutcome, ArchiveError> {
    let destination_metadata = fs::metadata(destination)?;
    if !destination_metadata.is_dir() {
        return Err(ArchiveError::Unsupported(destination.to_path_buf()));
    }

    let mut plans = Vec::with_capacity(sources.len());
    let mut total_entries = 0usize;
    let mut declared_bytes = 0u64;
    let mut sizes_known = true;
    for source in sources {
        check_cancelled(cancelled)?;
        progress(JobProgress {
            current_path: Some(source.clone()),
            ..JobProgress::default()
        });
        let plan = preflight(source)?;
        total_entries = total_entries
            .checked_add(plan.entries.len())
            .ok_or(ArchiveError::TooManyEntries)?;
        if total_entries > MAX_ENTRIES {
            return Err(ArchiveError::TooManyEntries);
        }
        declared_bytes =
            declared_bytes.saturating_add(plan.entries.iter().map(|entry| entry.size).sum::<u64>());
        sizes_known &= plan.standalone.is_none();
        plans.push(plan);
    }

    let mut staged = Vec::with_capacity(plans.len());
    let mut written = 0u64;
    let total_items = u64::try_from(total_entries).ok();
    let total_bytes = sizes_known.then_some(declared_bytes);
    let mut completed_base = 0u64;
    for plan in &plans {
        if let Err(error) = check_cancelled(cancelled) {
            cleanup_staged(&staged);
            return Err(error);
        }
        let staging_path = unique_staging_path(destination, &plan.output_name);
        let result = if let Some(kind) = plan.standalone {
            extract_stream(
                plan,
                kind,
                &staging_path,
                cancelled,
                &mut written,
                |done, bytes, path| {
                    progress(JobProgress {
                        completed_items: completed_base.saturating_add(done),
                        total_items,
                        completed_bytes: bytes,
                        total_bytes,
                        current_path: Some(path),
                    });
                },
            )
        } else {
            fs::create_dir(&staging_path)
                .map_err(ArchiveError::from)
                .and_then(|()| {
                    extract_multi(
                        plan,
                        &staging_path,
                        cancelled,
                        &mut written,
                        |done, bytes, path| {
                            progress(JobProgress {
                                completed_items: completed_base.saturating_add(done),
                                total_items,
                                completed_bytes: bytes,
                                total_bytes,
                                current_path: Some(path),
                            });
                        },
                    )
                })
        };
        if let Err(error) = result {
            cleanup_staged(&staged);
            remove_if_exists(&staging_path);
            return Err(error);
        }
        staged.push(StagedOutput {
            staging_path,
            requested_name: plan.output_name.clone(),
        });
        completed_base =
            completed_base.saturating_add(u64::try_from(plan.entries.len()).unwrap_or(0));
    }

    let mut committed: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(staged.len());
    for output in &staged {
        if let Err(error) = check_cancelled(cancelled) {
            let rollback = rollback_committed(&committed);
            cleanup_staged(&staged);
            if let Err(rollback_error) = rollback {
                return Err(rollback_error);
            }
            return Err(error);
        }
        match commit_keep_both(&output.staging_path, destination, &output.requested_name) {
            Ok(final_path) => committed.push((final_path, output.staging_path.clone())),
            Err(error) => {
                let rollback = rollback_committed(&committed);
                cleanup_staged(&staged);
                if let Err(rollback_error) = rollback {
                    return Err(rollback_error);
                }
                return Err(error);
            }
        }
    }

    let affected_paths: Vec<_> = committed.into_iter().map(|(path, _)| path).collect();
    let trees = fingerprint_outputs(&affected_paths, MAX_UNDO_ENTRIES)
        .ok()
        .flatten();
    let undo = trees.map(|trees| UndoRecord {
        label: "Extract archives".into(),
        kind: UndoKind::RemoveExtracted { trees },
    });
    Ok(OperationOutcome {
        affected_paths,
        skipped_paths: Vec::new(),
        undo,
    })
}

#[allow(clippy::too_many_lines)]
fn preflight(source: &Path) -> Result<ArchivePlan, ArchiveError> {
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.file_type().is_file() {
        return Err(ArchiveError::Unsupported(source.to_path_buf()));
    }
    let (output_name, stream) =
        archive_kind(source).ok_or_else(|| ArchiveError::Unsupported(source.to_path_buf()))?;
    if let Some(stream) = stream {
        return Ok(ArchivePlan {
            source: source.to_path_buf(),
            output_name,
            entries: vec![PlannedEntry {
                path: PathBuf::from("output"),
                kind: EntryKind::File,
                mode: 0o644,
                mtime: metadata.modified().ok(),
                size: 0,
                link_target: None,
            }],
            strip_root: None,
            standalone: Some(stream),
        });
    }

    let mut archive = open_archive(source)?;
    let mut entries = Vec::new();
    let mut paths = HashSet::new();
    let mut symlinks = HashSet::new();
    while let Some(entry) = archive
        .next_entry()
        .map_err(|_| ArchiveError::Unreadable(source.to_path_buf()))?
    {
        if entry.is_encrypted() || entry.is_data_encrypted() || entry.is_metadata_encrypted() {
            return Err(ArchiveError::Encrypted(source.to_path_buf()));
        }
        if entry.hardlink().is_some() {
            return unsafe_entry(source, "hard links are not supported");
        }
        let raw_path = entry.pathname().ok_or_else(|| ArchiveError::UnsafeEntry {
            archive: source.to_path_buf(),
            reason: "entry has no path".into(),
        })?;
        if raw_path.as_bytes().contains(&0) {
            return unsafe_entry(source, "entry path contains NUL");
        }
        let path = normalize_archive_path(Path::new(&raw_path)).ok_or_else(|| {
            ArchiveError::UnsafeEntry {
                archive: source.to_path_buf(),
                reason: format!("path escapes the output: {raw_path}"),
            }
        })?;
        if path.as_os_str().is_empty() {
            archive
                .skip_data()
                .map_err(|_| ArchiveError::Unreadable(source.to_path_buf()))?;
            continue;
        }
        if !paths.insert(path.clone()) {
            return unsafe_entry(source, &format!("duplicate path: {}", path.display()));
        }
        let kind = match entry.file_type() {
            FileType::RegularFile => EntryKind::File,
            FileType::Directory => EntryKind::Directory,
            FileType::SymbolicLink => EntryKind::Symlink,
            _ => return unsafe_entry(source, "device, FIFO, socket, or unknown entry"),
        };
        let link_target = if kind == EntryKind::Symlink {
            let target = entry.symlink().ok_or_else(|| ArchiveError::UnsafeEntry {
                archive: source.to_path_buf(),
                reason: format!("symlink has no target: {}", path.display()),
            })?;
            let target = PathBuf::from(target);
            if !safe_symlink_target(&path, &target) {
                return unsafe_entry(source, "symlink target escapes the output");
            }
            symlinks.insert(path.clone());
            Some(target)
        } else {
            None
        };
        entries.push(PlannedEntry {
            path,
            kind,
            mode: entry.mode() & 0o777,
            mtime: entry.mtime(),
            size: u64::try_from(entry.size()).unwrap_or(0),
            link_target,
        });
        archive
            .skip_data()
            .map_err(|_| ArchiveError::Unreadable(source.to_path_buf()))?;
        if entries.len() > MAX_ENTRIES {
            return Err(ArchiveError::TooManyEntries);
        }
    }
    for entry in &entries {
        if ancestors(&entry.path).any(|ancestor| symlinks.contains(ancestor)) {
            return unsafe_entry(source, "an entry would be written through a symlink");
        }
    }
    let strip_root = single_directory_root(&entries);
    Ok(ArchivePlan {
        source: source.to_path_buf(),
        output_name,
        entries,
        strip_root,
        standalone: None,
    })
}

#[allow(clippy::too_many_lines)]
fn extract_multi(
    plan: &ArchivePlan,
    staging: &Path,
    cancelled: &AtomicBool,
    written: &mut u64,
    mut report: impl FnMut(u64, u64, PathBuf),
) -> Result<(), ArchiveError> {
    let mut archive = open_archive(&plan.source)?;
    let mut completed = 0u64;
    let mut directories = Vec::new();
    for expected in &plan.entries {
        check_cancelled(cancelled)?;
        let actual = loop {
            let actual = {
                let entry = archive
                    .next_entry()
                    .map_err(|_| ArchiveError::Unreadable(plan.source.clone()))?
                    .ok_or_else(|| ArchiveError::Unreadable(plan.source.clone()))?;
                normalize_archive_path(Path::new(&entry.pathname().unwrap_or_default()))
                    .ok_or_else(|| ArchiveError::Unreadable(plan.source.clone()))?
            };
            if actual.as_os_str().is_empty() {
                archive
                    .skip_data()
                    .map_err(|_| ArchiveError::Unreadable(plan.source.clone()))?;
                continue;
            }
            break actual;
        };
        if actual != expected.path {
            return Err(ArchiveError::Unreadable(plan.source.clone()));
        }
        let relative = stripped_path(&expected.path, plan.strip_root.as_deref());
        if relative.as_os_str().is_empty() {
            if expected.kind == EntryKind::Directory {
                directories.push((staging.to_path_buf(), expected.mode, expected.mtime));
            }
            archive
                .skip_data()
                .map_err(|_| ArchiveError::Unreadable(plan.source.clone()))?;
            completed += 1;
            continue;
        }
        let output = staging.join(&relative);
        ensure_safe_parent(staging, &output)?;
        match expected.kind {
            EntryKind::Directory => {
                fs::create_dir_all(&output)?;
                directories.push((output.clone(), expected.mode, expected.mtime));
                archive
                    .skip_data()
                    .map_err(|_| ArchiveError::Unreadable(plan.source.clone()))?;
            }
            EntryKind::Symlink => {
                let target = expected.link_target.as_ref().expect("validated symlink");
                std::os::unix::fs::symlink(target, &output)?;
                archive
                    .skip_data()
                    .map_err(|_| ArchiveError::Unreadable(plan.source.clone()))?;
            }
            EntryKind::File => {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&output)?;
                let mut buffer = vec![0u8; 64 * 1024];
                loop {
                    check_cancelled(cancelled)?;
                    let count = archive
                        .read_data(&mut buffer)
                        .map_err(|_| ArchiveError::Unreadable(plan.source.clone()))?;
                    if count == 0 {
                        break;
                    }
                    reserve_space(staging, u64::try_from(count).unwrap_or(u64::MAX))?;
                    file.write_all(&buffer[..count])?;
                    *written = written.saturating_add(u64::try_from(count).unwrap_or(0));
                    report(completed, *written, output.clone());
                }
                file.set_permissions(fs::Permissions::from_mode(expected.mode))?;
                set_mtime(&file, expected.mtime)?;
            }
        }
        completed += 1;
        report(completed, *written, output);
    }
    loop {
        let path = {
            let Some(entry) = archive
                .next_entry()
                .map_err(|_| ArchiveError::Unreadable(plan.source.clone()))?
            else {
                break;
            };
            normalize_archive_path(Path::new(&entry.pathname().unwrap_or_default()))
                .ok_or_else(|| ArchiveError::Unreadable(plan.source.clone()))?
        };
        archive
            .skip_data()
            .map_err(|_| ArchiveError::Unreadable(plan.source.clone()))?;
        if !path.as_os_str().is_empty() {
            return Err(ArchiveError::Unreadable(plan.source.clone()));
        }
    }
    directories.sort_by_key(|(path, _, _)| std::cmp::Reverse(path.components().count()));
    for (path, mode, mtime) in directories {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
        let file = File::open(path)?;
        set_mtime(&file, mtime)?;
    }
    Ok(())
}

fn extract_stream(
    plan: &ArchivePlan,
    kind: StreamKind,
    staging: &Path,
    cancelled: &AtomicBool,
    written: &mut u64,
    mut report: impl FnMut(u64, u64, PathBuf),
) -> Result<(), ArchiveError> {
    let source = File::open(&plan.source)?;
    let reader: Box<dyn Read> = match kind {
        StreamKind::Gzip => Box::new(GzDecoder::new(BufReader::new(source))),
        StreamKind::Bzip2 => Box::new(BzDecoder::new(BufReader::new(source))),
        StreamKind::Xz => Box::new(XzDecoder::new(BufReader::new(source))),
        StreamKind::Zstd => Box::new(zstd::stream::read::Decoder::new(BufReader::new(source))?),
    };
    let mut reader = reader;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging)?;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        check_cancelled(cancelled)?;
        let count = reader
            .read(&mut buffer)
            .map_err(|_| ArchiveError::Unreadable(plan.source.clone()))?;
        if count == 0 {
            break;
        }
        reserve_space(
            staging.parent().unwrap_or(staging),
            u64::try_from(count).unwrap_or(u64::MAX),
        )?;
        output.write_all(&buffer[..count])?;
        *written = written.saturating_add(u64::try_from(count).unwrap_or(0));
        report(0, *written, staging.to_path_buf());
    }
    set_mtime(&output, plan.entries[0].mtime)?;
    report(1, *written, staging.to_path_buf());
    Ok(())
}

fn open_archive(path: &Path) -> Result<ReadArchive<'static>, ArchiveError> {
    let file = File::open(path)?;
    ReadArchive::open_callback(CallbackReader::new(BufReader::new(file)))
        .map_err(|_| ArchiveError::Unreadable(path.to_path_buf()))
}

fn archive_kind(path: &Path) -> Option<(OsString, Option<StreamKind>)> {
    let name = path.file_name()?.to_string_lossy();
    let lower = name.to_ascii_lowercase();
    for suffix in [
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tgz", ".tbz2", ".txz", ".tzst", ".zip",
        ".tar", ".7z", ".rar",
    ] {
        if lower.ends_with(suffix) && name.len() > suffix.len() {
            return Some((OsString::from(&name[..name.len() - suffix.len()]), None));
        }
    }
    for (suffix, kind) in [
        (".gz", StreamKind::Gzip),
        (".bz2", StreamKind::Bzip2),
        (".xz", StreamKind::Xz),
        (".zst", StreamKind::Zstd),
    ] {
        if lower.ends_with(suffix) && name.len() > suffix.len() {
            return Some((
                OsString::from(&name[..name.len() - suffix.len()]),
                Some(kind),
            ));
        }
    }
    None
}

fn normalize_archive_path(path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

fn safe_symlink_target(link_path: &Path, target: &Path) -> bool {
    !target.is_absolute()
        && normalize_archive_path(&link_path.parent().unwrap_or(Path::new("")).join(target))
            .is_some()
}

fn ancestors(path: &Path) -> impl Iterator<Item = &Path> {
    path.ancestors()
        .skip(1)
        .filter(|path| !path.as_os_str().is_empty())
}

fn single_directory_root(entries: &[PlannedEntry]) -> Option<OsString> {
    let roots: HashSet<_> = entries
        .iter()
        .filter_map(|entry| entry.path.components().next())
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_os_string()),
            _ => None,
        })
        .collect();
    let root = roots.into_iter().next()?;
    let is_directory = entries.iter().any(|entry| {
        entry.path == Path::new(&root) && entry.kind == EntryKind::Directory
            || entry.path.components().count() > 1
    });
    is_directory.then_some(root)
}

fn stripped_path(path: &Path, root: Option<&OsStr>) -> PathBuf {
    root.and_then(|root| path.strip_prefix(Path::new(root)).ok())
        .unwrap_or(path)
        .to_path_buf()
}

fn ensure_safe_parent(root: &Path, output: &Path) -> Result<(), ArchiveError> {
    let parent = output.parent().unwrap_or(root);
    fs::create_dir_all(parent)?;
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| ArchiveError::UnsafeEntry {
            archive: root.to_path_buf(),
            reason: "output escaped staging".into(),
        })?;
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        cursor.push(component);
        let metadata = fs::symlink_metadata(&cursor)?;
        if metadata.file_type().is_symlink() {
            return unsafe_entry(root, "output parent is a symlink");
        }
    }
    Ok(())
}

fn reserve_space(path: &Path, next_write: u64) -> Result<(), ArchiveError> {
    let stats = statvfs(path).map_err(io::Error::from)?;
    let available = stats
        .blocks_available()
        .saturating_mul(stats.fragment_size());
    if available < next_write.saturating_add(FREE_SPACE_RESERVE) {
        return Err(ArchiveError::NoSpace(path.to_path_buf()));
    }
    Ok(())
}

fn commit_keep_both(
    staging: &Path,
    destination: &Path,
    requested_name: &OsStr,
) -> Result<PathBuf, ArchiveError> {
    let directory = File::open(destination)?;
    let staging_name = staging
        .file_name()
        .ok_or_else(|| ArchiveError::Commit(staging.to_path_buf()))?;
    for index in 0u32.. {
        let candidate = if index == 0 {
            requested_name.to_os_string()
        } else {
            keep_both_name(requested_name, index)
        };
        match renameat2(
            &directory,
            Path::new(staging_name),
            &directory,
            Path::new(&candidate),
            RenameFlags::RENAME_NOREPLACE,
        ) {
            Ok(()) => return Ok(destination.join(candidate)),
            Err(Errno::EEXIST) => {}
            Err(_) => return Err(ArchiveError::Commit(destination.join(candidate))),
        }
    }
    unreachable!()
}

fn keep_both_name(name: &OsStr, index: u32) -> OsString {
    let path = Path::new(name);
    if path.extension().is_some() && path.file_stem().is_some() {
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let extension = path.extension().unwrap_or_default().to_string_lossy();
        OsString::from(format!("{stem} ({index}).{extension}"))
    } else {
        OsString::from(format!("{} ({index})", name.to_string_lossy()))
    }
}

fn unique_staging_path(destination: &Path, output_name: &OsStr) -> PathBuf {
    let safe = output_name.to_string_lossy().replace(['/', '\\'], "_");
    destination.join(format!(".{safe}.gnil-extract-{}", Uuid::new_v4()))
}

fn set_mtime(file: &File, mtime: Option<SystemTime>) -> io::Result<()> {
    if let Some(mtime) = mtime {
        file.set_times(FileTimes::new().set_modified(mtime))?;
    }
    Ok(())
}

fn fingerprint_outputs(
    paths: &[PathBuf],
    limit: usize,
) -> Result<Option<Vec<ExtractedTreeFingerprint>>, ArchiveError> {
    let mut remaining = limit;
    let mut overflowed = false;
    let mut trees = Vec::with_capacity(paths.len());
    for root in paths {
        let mut entries = Vec::new();
        fingerprint_path(root, root, &mut entries, &mut remaining, &mut overflowed)?;
        if overflowed {
            return Ok(None);
        }
        trees.push(ExtractedTreeFingerprint {
            root: root.clone(),
            entries,
        });
    }
    Ok(Some(trees))
}

fn fingerprint_path(
    root: &Path,
    path: &Path,
    entries: &mut Vec<ExtractedEntryFingerprint>,
    remaining: &mut usize,
    overflowed: &mut bool,
) -> Result<(), ArchiveError> {
    if *remaining == 0 {
        *overflowed = true;
        return Ok(());
    }
    *remaining -= 1;
    let metadata = fs::symlink_metadata(path)?;
    let kind = if metadata.file_type().is_symlink() {
        ExtractedEntryKind::Symlink {
            target: fs::read_link(path)?,
        }
    } else if metadata.is_dir() {
        ExtractedEntryKind::Directory
    } else {
        ExtractedEntryKind::File
    };
    entries.push(ExtractedEntryFingerprint {
        relative_path: path
            .strip_prefix(root)
            .unwrap_or(Path::new(""))
            .to_path_buf(),
        kind,
        len: metadata.len(),
        modified_unix_ms: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .and_then(|duration| i64::try_from(duration.as_millis()).ok()),
    });
    if metadata.is_dir() {
        for child in fs::read_dir(path)? {
            fingerprint_path(root, &child?.path(), entries, remaining, overflowed)?;
            if *overflowed {
                break;
            }
        }
    }
    Ok(())
}

fn rollback_committed(committed: &[(PathBuf, PathBuf)]) -> Result<(), ArchiveError> {
    let mut errors = Vec::new();
    for (final_path, staging) in committed.iter().rev() {
        if let Err(error) = fs::rename(final_path, staging) {
            errors.push(format!("{}: {error}", final_path.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ArchiveError::Rollback(errors.join("; ")))
    }
}

fn cleanup_staged(staged: &[StagedOutput]) {
    for output in staged {
        remove_if_exists(&output.staging_path);
    }
}

fn remove_if_exists(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    let _ = if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), ArchiveError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(ArchiveError::Cancelled)
    } else {
        Ok(())
    }
}

fn unsafe_entry<T>(archive: &Path, reason: &str) -> Result<T, ArchiveError> {
    Err(ArchiveError::UnsafeEntry {
        archive: archive.to_path_buf(),
        reason: reason.into(),
    })
}

pub fn verify_extracted_trees(trees: &[ExtractedTreeFingerprint]) -> Result<(), ArchiveError> {
    for tree in trees {
        let actual = fingerprint_outputs(std::slice::from_ref(&tree.root), tree.entries.len() + 1)?
            .and_then(|mut trees| trees.pop())
            .ok_or_else(|| ArchiveError::Commit(tree.root.clone()))?;
        let expected: HashSet<_> = tree.entries.iter().cloned().collect();
        let actual: HashSet<_> = actual.entries.into_iter().collect();
        if expected != actual {
            return Err(ArchiveError::Commit(tree.root.clone()));
        }
    }
    Ok(())
}

pub fn remove_extracted_trees(trees: &[ExtractedTreeFingerprint]) -> Result<(), ArchiveError> {
    verify_extracted_trees(trees)?;
    for tree in trees.iter().rev() {
        let metadata = fs::symlink_metadata(&tree.root)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            fs::remove_dir_all(&tree.root)?;
        } else {
            fs::remove_file(&tree.root)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use libarchive2::{ArchiveFormat, CompressionFormat, WriteArchive};
    use tempfile::tempdir;

    #[test]
    fn recognizes_supported_suffixes_and_output_names() {
        assert_eq!(
            archive_kind(Path::new("project.tar.zst")).unwrap().0,
            "project"
        );
        assert_eq!(archive_kind(Path::new("notes.gz")).unwrap().0, "notes");
        assert!(is_archive_candidate(Path::new("PROJECT.RAR")));
        assert!(!is_archive_candidate(Path::new("readme.txt")));
    }

    #[test]
    fn normalization_rejects_escape_and_absolute_paths() {
        assert_eq!(
            normalize_archive_path(Path::new("a/../b")),
            Some(PathBuf::from("b"))
        );
        assert_eq!(normalize_archive_path(Path::new("../secret")), None);
        assert_eq!(normalize_archive_path(Path::new("/etc/passwd")), None);
        assert!(safe_symlink_target(
            Path::new("project/bin/tool"),
            Path::new("../lib/tool")
        ));
        assert!(!safe_symlink_target(
            Path::new("project/link"),
            Path::new("../../outside")
        ));
    }

    #[test]
    fn keep_both_keeps_file_extension() {
        assert_eq!(keep_both_name(OsStr::new("data.txt"), 2), "data (2).txt");
        assert_eq!(keep_both_name(OsStr::new("folder"), 1), "folder (1)");
    }

    #[test]
    fn extracts_single_root_without_double_nesting_and_can_undo() {
        let temporary = tempdir().unwrap();
        let source = temporary.path().join("project.tar");
        let destination = temporary.path().join("output");
        fs::create_dir(&destination).unwrap();
        let mut writer = WriteArchive::new()
            .format(ArchiveFormat::TarPax)
            .compression(CompressionFormat::None)
            .open_file(source.to_str().unwrap())
            .unwrap();
        writer.add_directory("project").unwrap();
        writer
            .add_file("project/readme.txt", b"safe extraction")
            .unwrap();
        writer.finish().unwrap();

        let outcome = extract_archives(
            std::slice::from_ref(&source),
            &destination,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        let output = destination.join("project");
        assert_eq!(
            fs::read(output.join("readme.txt")).unwrap(),
            b"safe extraction"
        );
        assert!(!output.join("project").exists());
        let UndoKind::RemoveExtracted { trees } = &outcome.undo.unwrap().kind else {
            panic!("expected extraction undo");
        };
        remove_extracted_trees(trees).unwrap();
        assert!(!output.exists());
    }

    #[test]
    fn commit_resolves_keep_both_at_the_last_moment() {
        let temporary = tempdir().unwrap();
        let destination = temporary.path();
        fs::create_dir(destination.join("project")).unwrap();
        let staging = destination.join(".staging");
        fs::create_dir(&staging).unwrap();
        let committed = commit_keep_both(&staging, destination, OsStr::new("project")).unwrap();
        assert_eq!(committed, destination.join("project (1)"));
        assert!(destination.join("project").is_dir());
        assert!(committed.is_dir());
    }

    #[test]
    fn undo_refuses_a_modified_extracted_tree() {
        let temporary = tempdir().unwrap();
        let root = temporary.path().join("output");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("file.txt"), b"before").unwrap();
        let trees = fingerprint_outputs(std::slice::from_ref(&root), MAX_UNDO_ENTRIES)
            .unwrap()
            .unwrap();
        fs::write(root.join("file.txt"), b"after and changed").unwrap();
        assert!(remove_extracted_trees(&trees).is_err());
        assert!(root.exists());
    }

    #[test]
    fn extracts_a_standalone_gzip_stream() {
        let temporary = tempdir().unwrap();
        let source = temporary.path().join("notes.txt.gz");
        let destination = temporary.path().join("output");
        fs::create_dir(&destination).unwrap();
        let file = File::create(&source).unwrap();
        let mut encoder = GzEncoder::new(file, Compression::fast());
        encoder.write_all(b"streamed payload").unwrap();
        encoder.finish().unwrap();

        extract_archives(
            std::slice::from_ref(&source),
            &destination,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(
            fs::read(destination.join("notes.txt")).unwrap(),
            b"streamed payload"
        );
    }

    #[test]
    fn batch_preflight_failure_leaves_no_partial_output() {
        let temporary = tempdir().unwrap();
        let valid = temporary.path().join("valid.tar");
        let invalid = temporary.path().join("broken.zip");
        let destination = temporary.path().join("output");
        fs::create_dir(&destination).unwrap();
        let mut writer = WriteArchive::new()
            .format(ArchiveFormat::TarPax)
            .compression(CompressionFormat::None)
            .open_file(valid.to_str().unwrap())
            .unwrap();
        writer.add_file("file.txt", b"data").unwrap();
        writer.finish().unwrap();
        fs::write(&invalid, b"not an archive").unwrap();

        assert!(
            extract_archives(
                &[valid, invalid],
                &destination,
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    }
}
