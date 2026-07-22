use std::{fs, io, path::PathBuf};

use gnil_core::{FileKind, TrashEntryRef};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashEntry {
    pub reference: TrashEntryRef,
    pub name: String,
    pub original_path: PathBuf,
    pub deletion_unix: i64,
    pub kind: FileKind,
    pub len: u64,
    pub trash_root: PathBuf,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrashSnapshot {
    pub entries: Vec<TrashEntry>,
    pub unreadable_entries: usize,
}

pub fn scan_trash() -> io::Result<TrashSnapshot> {
    let items = trash::os_limited::list().map_err(io::Error::other)?;
    let mut snapshot = TrashSnapshot::default();
    for item in items {
        let info_path = PathBuf::from(&item.id);
        let Some(stored_name) = info_path.file_stem() else {
            snapshot.unreadable_entries += 1;
            continue;
        };
        let Some(trash_root) = info_path
            .parent()
            .and_then(|info| info.parent())
            .map(PathBuf::from)
        else {
            snapshot.unreadable_entries += 1;
            continue;
        };
        let trashed_path = trash_root.join("files").join(stored_name);
        let Ok(metadata) = fs::symlink_metadata(&trashed_path) else {
            snapshot.unreadable_entries += 1;
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
        let original_path = item.original_path();
        snapshot.entries.push(TrashEntry {
            reference: TrashEntryRef {
                info_path,
                trashed_path,
                original_path: original_path.clone(),
            },
            name: item.name.to_string_lossy().into_owned(),
            original_path,
            deletion_unix: item.time_deleted,
            kind,
            len: metadata.len(),
            trash_root,
        });
    }
    snapshot.entries.sort_by(|left, right| {
        right
            .deletion_unix
            .cmp(&left.deletion_unix)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_snapshot_is_empty() {
        let snapshot = TrashSnapshot::default();
        assert!(snapshot.entries.is_empty());
        assert_eq!(snapshot.unreadable_entries, 0);
    }
}
