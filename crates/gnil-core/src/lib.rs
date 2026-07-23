//! Domain models and stable contracts shared by the gnil-fm UI and services.

mod action;
mod entry;
mod job;
mod operation;
mod selection;
mod settings;
mod tab;
mod theme;

pub use action::{ActionId, actions};
pub use entry::{
    DirectorySnapshot, FileEntry, FileKind, FileMetadata, GitStatus, SortDirection, SortField,
    SortSpec, display_name, natural_cmp,
};
pub use job::{JobEvent, JobId, JobPriority, JobProgress, JobState};
pub use operation::{
    ConflictDecision, ExtractedEntryFingerprint, ExtractedEntryKind, ExtractedTreeFingerprint,
    FileFingerprint, FsOperation, OperationOutcome, PermissionChange, PermissionUndo, RenamePair,
    TrashEntryRef, UndoKind, UndoRecord,
};
pub use selection::{SelectionMerge, SelectionState};
pub use settings::{AppSettings, ConfigPaths, KeymapProfile, ThemeMode};
pub use tab::{TabLocation, TabRoot, TabState};
pub use theme::{
    LoadedTheme, ThemeAppearance, ThemeCatalog, ThemeColorOverrides, ThemeColors, ThemeFile,
};
