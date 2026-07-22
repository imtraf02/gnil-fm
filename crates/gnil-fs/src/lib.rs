//! Local filesystem engine for gnil-fm.

mod archive;
mod devices;
mod git;
mod operation;
mod scan;
mod scheduler;
mod search;
mod trash_view;
mod watcher;

pub use archive::is_archive_candidate;
pub use devices::{
    DeviceEntry, DeviceError, DeviceKind, DeviceMonitor, eject_device, mount_device, scan_devices,
    unmount_device,
};
pub use git::{GitStatusSnapshot, scan_git_status};
pub use operation::{OperationError, OperationExecutor};
pub use scan::{ScanOptions, scan_directory};
pub use scheduler::{JobContext, JobHandle, TaskScheduler};
pub use search::{SearchHit, SearchOptions, fuzzy_match_score, search_paths};
pub use trash_view::{TrashEntry, TrashSnapshot, scan_trash};
pub use watcher::{DirectoryWatcher, WatchEvent};
