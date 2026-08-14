use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ConflictDecision {
    #[default]
    Ask,
    KeepBoth,
    Replace,
    Skip,
    MergeDirectory,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FsOperation {
    CreateFile {
        path: PathBuf,
    },
    CreateDirectory {
        path: PathBuf,
    },
    Rename {
        from: PathBuf,
        to: PathBuf,
    },
    BulkRename {
        pairs: Vec<RenamePair>,
    },
    CreateSymlink {
        link_path: PathBuf,
        target: PathBuf,
    },
    SetPermissions {
        paths: Vec<PathBuf>,
        change: PermissionChange,
    },
    Copy {
        sources: Vec<PathBuf>,
        destination: PathBuf,
        conflict: ConflictDecision,
    },
    Move {
        sources: Vec<PathBuf>,
        destination: PathBuf,
        conflict: ConflictDecision,
    },
    Trash {
        paths: Vec<PathBuf>,
    },
    DeletePermanently {
        paths: Vec<PathBuf>,
    },
    RestoreTrash {
        entries: Vec<TrashEntryRef>,
        replace_existing: bool,
    },
    PurgeTrash {
        entries: Vec<TrashEntryRef>,
    },
    ExtractArchives {
        sources: Vec<PathBuf>,
        destination: PathBuf,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TrashEntryRef {
    pub info_path: PathBuf,
    pub trashed_path: PathBuf,
    pub original_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RenamePair {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PermissionChange {
    Exact(u32),
    Mask { set: u32, clear: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PermissionUndo {
    pub path: PathBuf,
    pub before: u32,
    pub after: u32,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum TreeEntryKind {
    File,
    Directory,
    Symlink { target: PathBuf },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct TreeEntryFingerprint {
    pub relative_path: PathBuf,
    pub kind: TreeEntryKind,
    pub device: u64,
    pub inode: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub len: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
    pub content_blake3: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TreeFingerprint {
    pub root: PathBuf,
    pub entries: Vec<TreeEntryFingerprint>,
}

pub type ExtractedEntryKind = TreeEntryKind;
pub type ExtractedEntryFingerprint = TreeEntryFingerprint;
pub type ExtractedTreeFingerprint = TreeFingerprint;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UndoKind {
    RemoveCreated { trees: Vec<TreeFingerprint> },
    RenameBack { from: PathBuf, to: PathBuf },
    BulkRenameBack { pairs: Vec<RenamePair> },
    RemoveSymlink { link_path: PathBuf, target: PathBuf },
    RestorePermissions { entries: Vec<PermissionUndo> },
    MoveBack { pairs: Vec<(PathBuf, PathBuf)> },
    RestoreTrash { original_paths: Vec<PathBuf> },
    RemoveExtracted { trees: Vec<TreeFingerprint> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UndoRecord {
    pub label: String,
    pub kind: UndoKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationOutcome {
    pub affected_paths: Vec<PathBuf>,
    pub skipped_paths: Vec<PathBuf>,
    pub issues: Vec<OperationIssue>,
    pub undo: Option<UndoRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OperationIssue {
    pub path: PathBuf,
    pub message: String,
}
