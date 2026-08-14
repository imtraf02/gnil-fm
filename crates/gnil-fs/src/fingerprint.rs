use std::{
    fs,
    io::{self, Read as _},
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
};

use gnil_core::{TreeEntryFingerprint, TreeEntryKind, TreeFingerprint};
use thiserror::Error;
use uuid::Uuid;

use crate::secure_fs;

pub(crate) const MAX_UNDO_ENTRIES: usize = 50_000;

#[derive(Debug, Error)]
pub(crate) enum FingerprintError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("tree changed after the operation: {0}")]
    Conflict(PathBuf),
    #[error("undo could not restore quarantined data: {0:?}")]
    Recovery(Vec<PathBuf>),
}

struct Quarantined<'a> {
    expected: &'a TreeFingerprint,
    path: PathBuf,
}

pub(crate) fn fingerprint_trees(
    roots: &[PathBuf],
    limit: usize,
) -> Result<Option<Vec<TreeFingerprint>>, FingerprintError> {
    let mut remaining = limit;
    let mut trees = Vec::with_capacity(roots.len());
    for root in roots {
        let mut entries = Vec::new();
        if !fingerprint_path(root, root, &mut entries, &mut remaining)? {
            return Ok(None);
        }
        entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        trees.push(TreeFingerprint {
            root: root.clone(),
            entries,
        });
    }
    Ok(Some(trees))
}

pub(crate) fn quarantine_and_remove(trees: &[TreeFingerprint]) -> Result<(), FingerprintError> {
    let mut quarantined = Vec::with_capacity(trees.len());
    for tree in trees {
        let parent = tree
            .root
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "tree has no parent"))?;
        let path = parent.join(format!(".gnil-undo-{}", Uuid::new_v4()));
        let from = secure_fs::open_parent(&tree.root)?;
        let to = secure_fs::open_parent(&path)?;
        if let Err(error) = secure_fs::rename_noreplace(&from, &to) {
            let rollback = restore_quarantined(&quarantined);
            return if rollback.is_empty() {
                Err(if error.kind() == io::ErrorKind::AlreadyExists {
                    FingerprintError::Conflict(tree.root.clone())
                } else {
                    FingerprintError::Io(error)
                })
            } else {
                Err(FingerprintError::Recovery(rollback))
            };
        }
        quarantined.push(Quarantined {
            expected: tree,
            path,
        });
    }

    let verified = quarantined.iter().all(|item| {
        fingerprint_trees(
            std::slice::from_ref(&item.path),
            item.expected.entries.len() + 1,
        )
        .ok()
        .flatten()
        .and_then(|mut trees| trees.pop())
        .is_some_and(|actual| actual.entries == item.expected.entries)
    });
    if !verified {
        let recovery = restore_quarantined(&quarantined);
        return if recovery.is_empty() {
            Err(FingerprintError::Conflict(
                quarantined
                    .first()
                    .map_or_else(PathBuf::new, |item| item.expected.root.clone()),
            ))
        } else {
            Err(FingerprintError::Recovery(recovery))
        };
    }

    for item in quarantined.iter().rev() {
        let parent = secure_fs::open_parent(&item.path)?;
        secure_fs::remove_tree(&parent)?;
    }
    Ok(())
}

fn restore_quarantined(quarantined: &[Quarantined<'_>]) -> Vec<PathBuf> {
    let mut recovery = Vec::new();
    for item in quarantined.iter().rev() {
        let result = secure_fs::open_parent(&item.path).and_then(|from| {
            secure_fs::open_parent(&item.expected.root)
                .and_then(|to| secure_fs::rename_noreplace(&from, &to))
        });
        if result.is_err() {
            recovery.push(item.path.clone());
        }
    }
    recovery
}

fn fingerprint_path(
    root: &Path,
    path: &Path,
    entries: &mut Vec<TreeEntryFingerprint>,
    remaining: &mut usize,
) -> Result<bool, FingerprintError> {
    if *remaining == 0 {
        return Ok(false);
    }
    *remaining -= 1;

    let metadata = fs::symlink_metadata(path)?;
    let (kind, content_blake3) = if metadata.file_type().is_symlink() {
        (
            TreeEntryKind::Symlink {
                target: fs::read_link(path)?,
            },
            None,
        )
    } else if metadata.is_dir() {
        (TreeEntryKind::Directory, None)
    } else if metadata.is_file() {
        let parent = secure_fs::open_parent(path)?;
        let mut file = fs::File::from(secure_fs::open_file_readonly(&parent)?);
        let opened = file.metadata()?;
        if (opened.dev(), opened.ino()) != (metadata.dev(), metadata.ino()) {
            return Err(FingerprintError::Conflict(path.to_path_buf()));
        }
        let mut hasher = blake3::Hasher::new();
        let mut buffer = vec![0_u8; 128 * 1024].into_boxed_slice();
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let after = file.metadata()?;
        if file_metadata_changed(&opened, &after) {
            return Err(FingerprintError::Conflict(path.to_path_buf()));
        }
        (TreeEntryKind::File, Some(*hasher.finalize().as_bytes()))
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported file kind: {}", path.display()),
        )
        .into());
    };

    entries.push(TreeEntryFingerprint {
        relative_path: path
            .strip_prefix(root)
            .unwrap_or_else(|_| Path::new(""))
            .to_path_buf(),
        kind,
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode() & 0o7777,
        uid: metadata.uid(),
        gid: metadata.gid(),
        len: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        content_blake3,
    });

    if metadata.is_dir() {
        let mut names = secure_fs::list_dir(path)?;
        names.sort();
        for name in names {
            if !fingerprint_path(root, &path.join(name), entries, remaining)? {
                return Ok(false);
            }
        }
        let after = fs::symlink_metadata(path)?;
        if file_metadata_changed(&metadata, &after) {
            return Err(FingerprintError::Conflict(path.to_path_buf()));
        }
    }
    Ok(true)
}

fn file_metadata_changed(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
}
