use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
};

use gnil_core::GitStatus;

#[derive(Clone, Debug, Default)]
pub struct GitStatusSnapshot {
    pub root: Option<PathBuf>,
    pub entries: HashMap<PathBuf, GitStatus>,
}

impl GitStatusSnapshot {
    #[must_use]
    pub fn status_for_path(&self, path: &Path) -> Option<GitStatus> {
        self.entries.get(path).copied().or_else(|| {
            self.entries
                .iter()
                .filter(|(changed, _)| changed.starts_with(path))
                .map(|(_, status)| *status)
                .max_by_key(|status| git_severity(*status))
        })
    }
}

const fn git_severity(status: GitStatus) -> u8 {
    match status {
        GitStatus::Conflicted => 5,
        GitStatus::Deleted => 4,
        GitStatus::Modified => 3,
        GitStatus::Added => 2,
        GitStatus::Untracked => 1,
    }
}

/// Reads porcelain status without mutating the repository. The process backend is intentionally
/// isolated here so it can be replaced by `gix` without changing UI contracts.
#[must_use]
pub fn scan_git_status(path: &Path) -> GitStatusSnapshot {
    let Ok(root_output) = Command::new("git")
        .args([
            "-C",
            &path.to_string_lossy(),
            "rev-parse",
            "--show-toplevel",
        ])
        .output()
    else {
        return GitStatusSnapshot::default();
    };
    if !root_output.status.success() {
        return GitStatusSnapshot::default();
    }
    let root = PathBuf::from(String::from_utf8_lossy(&root_output.stdout).trim());
    let Ok(output) = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ])
        .output()
    else {
        return GitStatusSnapshot {
            root: Some(root),
            entries: HashMap::new(),
        };
    };
    let mut entries = HashMap::new();
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| record.len() >= 4)
    {
        let code = &record[..2];
        let path = String::from_utf8_lossy(&record[3..]);
        let status = match code {
            b"??" => GitStatus::Untracked,
            b"AA" | b"DD" | b"AU" | b"UA" | b"DU" | b"UD" | b"UU" => GitStatus::Conflicted,
            code if code.contains(&b'A') => GitStatus::Added,
            code if code.contains(&b'D') => GitStatus::Deleted,
            _ => GitStatus::Modified,
        };
        entries.insert(root.join(path.as_ref()), status);
    }
    GitStatusSnapshot {
        root: Some(root),
        entries,
    }
}
