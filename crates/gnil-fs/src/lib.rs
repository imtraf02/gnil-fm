//! Local filesystem engine for gnil-fm.

mod archive;
mod devices;
mod fingerprint;
mod git;
mod mime;
mod operation;
mod scan;
mod scheduler;
mod search;
mod secure_fs;
mod trash_view;
mod watcher;

pub use archive::is_archive_candidate;
pub use devices::{
    DeviceEntry, DeviceError, DeviceKind, DeviceMonitor, eject_device, mount_device, scan_devices,
    unmount_device,
};
pub use git::{GitStatusSnapshot, scan_git_status};
pub use mime::detect_mime;
pub use operation::{OperationError, OperationExecutor, validate_transfer_destination};
pub use scan::{ScanOptions, scan_directory, scan_directory_progressive, scan_entry};
pub use scheduler::{JobContext, JobController, JobHandle, JobResult, TaskScheduler};
pub use search::{SearchHit, SearchOptions, fuzzy_match_score, search_paths};
pub use trash_view::{TrashEntry, TrashSnapshot, scan_trash};
pub use watcher::{DirectoryWatcher, WatchEvent};
