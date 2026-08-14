use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::operation::ConflictDecision;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct JobId(pub Uuid);

impl JobId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum JobPriority {
    Background,
    Preview,
    Foreground,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum JobState {
    Queued,
    Preparing,
    Running,
    Paused,
    WaitingForConflict,
    WaitingForError,
    Completed,
    CompletedWithIssues,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct JobProgress {
    pub completed_items: u64,
    pub total_items: Option<u64>,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
    pub current_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum JobPrompt {
    Conflict {
        source: PathBuf,
        destination: PathBuf,
        source_is_directory: bool,
        destination_is_directory: bool,
    },
    IoError {
        path: PathBuf,
        message: String,
        can_skip: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum JobDecision {
    Conflict {
        decision: ConflictDecision,
        apply_to_all: bool,
    },
    Retry,
    Skip,
    Cancel,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum JobEvent {
    State { id: JobId, state: JobState },
    Progress { id: JobId, progress: JobProgress },
    Prompt { id: JobId, prompt: JobPrompt },
    Message { id: JobId, message: String },
}
