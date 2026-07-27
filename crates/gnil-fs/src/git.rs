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
        self.entries.get(path).copied()
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
        .arg("-C")
        .arg(&root)
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--",
        ])
        .arg(path.strip_prefix(&root).unwrap_or(path))
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
        let changed = root.join(path.as_ref());
        insert_status(&mut entries, changed.as_path(), &root, status);
    }
    GitStatusSnapshot {
        root: Some(root),
        entries,
    }
}

fn insert_status(
    entries: &mut HashMap<PathBuf, GitStatus>,
    changed: &Path,
    root: &Path,
    status: GitStatus,
) {
    let mut current = Some(changed);
    while let Some(path) = current {
        entries
            .entry(path.to_path_buf())
            .and_modify(|existing| {
                if git_severity(status) > git_severity(*existing) {
                    *existing = status;
                }
            })
            .or_insert(status);
        if path == root {
            break;
        }
        current = path.parent().filter(|parent| parent.starts_with(root));
    }
}
