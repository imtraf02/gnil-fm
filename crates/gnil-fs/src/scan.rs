use std::{fs, io, path::Path, time::UNIX_EPOCH};

use gnil_core::{DirectorySnapshot, FileEntry, FileKind, FileMetadata, SortSpec};

#[derive(Clone, Copy, Debug, Default)]
pub struct ScanOptions {
    pub generation: u64,
    pub show_hidden: bool,
    pub sort: SortSpec,
}

pub fn scan_directory(path: &Path, options: ScanOptions) -> io::Result<DirectorySnapshot> {
    let mut entries = Vec::new();
    let mut unreadable_entries = 0;

    for item in fs::read_dir(path)? {
        let Ok(item) = item else {
            unreadable_entries += 1;
            continue;
        };
        let name = item.file_name().to_string_lossy().into_owned();
        let hidden = name.starts_with('.');
        if hidden && !options.show_hidden {
            continue;
        }
        let item_path = item.path();
        let Ok(metadata) = fs::symlink_metadata(&item_path) else {
            unreadable_entries += 1;
            continue;
        };
        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            FileKind::Symlink
        } else if file_type.is_dir() {
            FileKind::Directory
        } else if file_type.is_file() {
            FileKind::File
        } else {
            FileKind::Other
        };
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

        entries.push(FileEntry {
            path: item_path.clone(),
            name,
            kind,
            hidden,
            metadata: FileMetadata {
                len: metadata.len(),
                modified_unix_ms,
                mode,
                readonly: metadata.permissions().readonly(),
                symlink_target: file_type
                    .is_symlink()
                    .then(|| fs::read_link(&item_path).ok())
                    .flatten(),
                mime: (kind == FileKind::File)
                    .then(|| {
                        mime_guess::from_path(&item_path)
                            .first_raw()
                            .map(str::to_owned)
                    })
                    .flatten(),
            },
            git_status: None,
        });
    }

    options.sort.sort(&mut entries);
    Ok(DirectorySnapshot {
        generation: options.generation,
        path: path.to_path_buf(),
        entries,
        unreadable_entries,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn scan_hides_dotfiles_and_keeps_directories_first() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("file10"), b"x").unwrap();
        fs::write(root.path().join("file2"), b"x").unwrap();
        fs::write(root.path().join(".secret"), b"x").unwrap();
        fs::create_dir(root.path().join("folder")).unwrap();

        let snapshot = scan_directory(root.path(), ScanOptions::default()).unwrap();
        let names: Vec<_> = snapshot
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, ["folder", "file2", "file10"]);
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
}
