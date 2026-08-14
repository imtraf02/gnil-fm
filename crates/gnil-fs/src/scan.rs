use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    thread,
    time::UNIX_EPOCH,
};

use gnil_core::{
    DirectoryLoadPhase, DirectorySnapshot, EntryMetadata, FileEntry, FileKind, FileMetadata,
    SortDirection, SortField, SortSpec,
};
use ignore::WalkBuilder;

use crate::{mime::detect_mime_with_metadata, scan_git_status};

#[derive(Clone, Copy, Debug)]
pub struct ScanOptions {
    pub generation: u64,
    pub show_hidden: bool,
    pub sort: SortSpec,
    pub respect_gitignore: bool,
    pub metadata_workers: usize,
    pub include_git_status: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            generation: 0,
            show_hidden: false,
            sort: SortSpec::default(),
            respect_gitignore: false,
            metadata_workers: 4,
            include_git_status: false,
        }
    }
}

pub fn scan_directory(path: &Path, options: ScanOptions) -> io::Result<DirectorySnapshot> {
    scan_directory_progressive(path, options, &AtomicBool::new(false), |_| {})
}

/// Reads one direct directory entry and its cheap metadata.
///
/// This is intended for incremental watcher updates. Like the initial directory scan, it never
/// enumerates a child directory to calculate its item count.
pub fn scan_entry(path: PathBuf) -> io::Result<FileEntry> {
    let file_type = fs::symlink_metadata(&path)?.file_type();
    let mut entry = pending_entry(path, Some(classify_file_type(file_type)));
    entry.metadata = EntryMetadata::Ready(read_metadata(&entry)?);
    Ok(entry)
}

pub fn scan_directory_progressive(
    path: &Path,
    options: ScanOptions,
    cancelled: &AtomicBool,
    mut discovered: impl FnMut(DirectorySnapshot),
) -> io::Result<DirectorySnapshot> {
    ensure_not_cancelled(cancelled)?;
    let (mut entries, discovery_errors) = discover_entries(path, options, cancelled)?;
    sort_discovered(&mut entries, options.sort);
    discovered(DirectorySnapshot {
        generation: options.generation,
        path: path.to_path_buf(),
        entries: entries.clone(),
        unreadable_entries: discovery_errors,
        phase: DirectoryLoadPhase::Discovered,
    });

    ensure_not_cancelled(cancelled)?;
    let metadata_errors = AtomicUsize::new(0);
    let git = thread::scope(|scope| {
        let git_task = options
            .include_git_status
            .then(|| scope.spawn(|| scan_git_status(path)));
        let workers = options
            .metadata_workers
            .clamp(1, 16)
            .min(entries.len().max(1));
        let chunk_size = entries.len().div_ceil(workers).max(1);
        for chunk in entries.chunks_mut(chunk_size) {
            let metadata_errors = &metadata_errors;
            scope.spawn(move || {
                for entry in chunk {
                    if cancelled.load(Ordering::Relaxed) {
                        return;
                    }
                    if let Ok(metadata) = read_metadata(entry) {
                        entry.metadata = EntryMetadata::Ready(metadata);
                    } else {
                        entry.metadata = EntryMetadata::Unavailable;
                        metadata_errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
        git_task.map(|task| task.join().unwrap_or_default())
    });
    ensure_not_cancelled(cancelled)?;

    if let Some(git) = git {
        for entry in &mut entries {
            entry.git_status = git.status_for_path(&entry.path);
        }
    }

    if matches!(options.sort.field, SortField::Size | SortField::Modified) {
        options.sort.sort(&mut entries);
    }
    Ok(DirectorySnapshot {
        generation: options.generation,
        path: path.to_path_buf(),
        entries,
        unreadable_entries: discovery_errors + metadata_errors.load(Ordering::Relaxed),
        phase: DirectoryLoadPhase::Complete,
    })
}

fn discover_entries(
    path: &Path,
    options: ScanOptions,
    cancelled: &AtomicBool,
) -> io::Result<(Vec<FileEntry>, usize)> {
    if options.respect_gitignore {
        let mut builder = WalkBuilder::new(path);
        builder
            .max_depth(Some(1))
            .hidden(!options.show_hidden)
            .ignore(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .follow_links(false);
        let mut entries = Vec::new();
        let mut unreadable = 0;
        for item in builder.build() {
            ensure_not_cancelled(cancelled)?;
            match item {
                Ok(item) if item.depth() == 1 => {
                    let kind = item.file_type().map(classify_file_type);
                    entries.push(pending_entry(item.into_path(), kind));
                }
                Ok(_) => {}
                Err(_) => unreadable += 1,
            }
        }
        return Ok((entries, unreadable));
    }

    let mut entries = Vec::new();
    let mut unreadable = 0;
    for item in fs::read_dir(path)? {
        ensure_not_cancelled(cancelled)?;
        let Ok(item) = item else {
            unreadable += 1;
            continue;
        };
        let name = item.file_name().to_string_lossy().into_owned();
        if !options.show_hidden && name.starts_with('.') {
            continue;
        }
        let kind = if let Ok(file_type) = item.file_type() {
            Some(classify_file_type(file_type))
        } else {
            unreadable += 1;
            None
        };
        entries.push(pending_entry_with_name(item.path(), name, kind));
    }
    Ok((entries, unreadable))
}

fn pending_entry(path: PathBuf, kind: Option<FileKind>) -> FileEntry {
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    pending_entry_with_name(path, name, kind)
}

fn pending_entry_with_name(path: PathBuf, name: String, kind: Option<FileKind>) -> FileEntry {
    FileEntry {
        path,
        hidden: name.starts_with('.'),
        name,
        kind: kind.unwrap_or(FileKind::Other),
        metadata: EntryMetadata::Pending,
        git_status: None,
    }
}

fn classify_file_type(file_type: fs::FileType) -> FileKind {
    if file_type.is_symlink() {
        FileKind::Symlink
    } else if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_file() {
        FileKind::File
    } else {
        FileKind::Other
    }
}

fn read_metadata(entry: &FileEntry) -> io::Result<FileMetadata> {
    let metadata = fs::symlink_metadata(&entry.path)?;
    let modified_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok());

    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::MetadataExt;
        Some(metadata.mode())
    };
    #[cfg(not(unix))]
    let mode = None;

    let symlink_target = metadata
        .file_type()
        .is_symlink()
        .then(|| fs::read_link(&entry.path).ok())
        .flatten();
    let symlink_target_kind = metadata.file_type().is_symlink().then(|| {
        fs::metadata(&entry.path).map_or(FileKind::Other, |target| {
            classify_file_type(target.file_type())
        })
    });
    let mime = if entry.kind == FileKind::File {
        Some(detect_mime_with_metadata(&entry.path, &metadata))
    } else if entry.kind == FileKind::Symlink && symlink_target_kind == Some(FileKind::File) {
        fs::metadata(&entry.path)
            .ok()
            .map(|target| detect_mime_with_metadata(&entry.path, &target))
    } else {
        None
    };
    Ok(FileMetadata {
        len: metadata.len(),
        // Match Thunar's fast path: listing a folder never recursively enumerates each child
        // directory. Item counts are loaded separately when an explicit preview needs them.
        child_count: None,
        modified_unix_ms,
        mode,
        readonly: metadata.permissions().readonly(),
        symlink_target,
        symlink_target_kind,
        mime,
    })
}

fn sort_discovered(entries: &mut [FileEntry], requested: SortSpec) {
    let mut sort = requested;
    if matches!(sort.field, SortField::Size | SortField::Modified) {
        sort.field = SortField::Name;
        sort.direction = SortDirection::default();
    }
    sort.sort(entries);
}

fn ensure_not_cancelled(cancelled: &AtomicBool) -> io::Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "directory scan cancelled",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::atomic::AtomicBool};

    use super::*;
    use gnil_core::{DirectoryLoadPhase, EntryMetadata};

    #[test]
    fn scan_hides_dotfiles_and_keeps_directories_first() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("file10"), b"x").unwrap();
        fs::write(root.path().join("file2"), b"x").unwrap();
        fs::write(root.path().join(".secret"), b"x").unwrap();
        let folder = root.path().join("folder");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("direct-child"), b"x").unwrap();
        fs::create_dir(folder.join("nested")).unwrap();
        fs::write(folder.join("nested/deep-child"), b"x").unwrap();

        let snapshot = scan_directory(root.path(), ScanOptions::default()).unwrap();
        let names: Vec<_> = snapshot
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, ["folder", "file2", "file10"]);
        assert_eq!(
            snapshot.entries[0]
                .metadata()
                .and_then(|metadata| metadata.child_count),
            None
        );
        assert_eq!(
            snapshot.entries[1].metadata().map(|metadata| metadata.len),
            Some(1)
        );
    }

    #[test]
    fn incremental_directory_entry_does_not_count_children() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("folder");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("child"), b"x").unwrap();

        let entry = scan_entry(folder).unwrap();
        assert_eq!(entry.kind, FileKind::Directory);
        assert_eq!(
            entry.metadata().and_then(|metadata| metadata.child_count),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn scan_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("target")).unwrap();
        symlink(root.path().join("target"), root.path().join("link")).unwrap();
        let snapshot = scan_directory(root.path(), ScanOptions::default()).unwrap();
        assert_eq!(snapshot.entries[0].kind, FileKind::Directory);
        assert_eq!(snapshot.entries[1].kind, FileKind::Symlink);
    }

    #[test]
    fn progressive_scan_emits_discovery_before_complete_metadata() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("notes.txt"), b"hello").unwrap();
        let mut discovered = Vec::new();
        let complete = scan_directory_progressive(
            root.path(),
            ScanOptions::default(),
            &AtomicBool::new(false),
            |snapshot| discovered.push(snapshot),
        )
        .unwrap();

        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].phase, DirectoryLoadPhase::Discovered);
        assert!(matches!(
            discovered[0].entries[0].metadata,
            EntryMetadata::Pending
        ));
        assert_eq!(complete.phase, DirectoryLoadPhase::Complete);
        assert!(matches!(
            complete.entries[0].metadata,
            EntryMetadata::Ready(_)
        ));
    }

    #[test]
    fn progressive_scan_can_filter_gitignored_children() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".git")).unwrap();
        fs::write(root.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(root.path().join("ignored.txt"), b"ignored").unwrap();
        fs::write(root.path().join("visible.txt"), b"visible").unwrap();
        let snapshot = scan_directory(
            root.path(),
            ScanOptions {
                respect_gitignore: true,
                ..ScanOptions::default()
            },
        )
        .unwrap();
        let names: Vec<_> = snapshot
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, ["visible.txt"]);
    }

    #[test]
    fn cancelled_scan_stops_before_emitting_a_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let cancelled = AtomicBool::new(true);
        let error =
            scan_directory_progressive(root.path(), ScanOptions::default(), &cancelled, |_| {
                panic!("cancelled scan must not emit")
            })
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    }
}
