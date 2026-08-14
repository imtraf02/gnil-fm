use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    io::Read as _,
    os::unix::ffi::OsStringExt as _,
    os::unix::process::CommandExt as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use gnil_core::GitStatus;
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};

const GIT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_GIT_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

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
    let mut root_command = git_command();
    root_command
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"]);
    let Some(root_output) = run_git(root_command) else {
        return GitStatusSnapshot::default();
    };
    let mut root_bytes = root_output.as_slice();
    if let Some(trimmed) = root_bytes.strip_suffix(b"\n") {
        root_bytes = trimmed;
    }
    if let Some(trimmed) = root_bytes.strip_suffix(b"\r") {
        root_bytes = trimmed;
    }
    if root_bytes.is_empty() || root_bytes.contains(&0) {
        return GitStatusSnapshot::default();
    }
    let root = PathBuf::from(OsString::from_vec(root_bytes.to_vec()));
    if !root.is_absolute() {
        return GitStatusSnapshot::default();
    }
    let mut status_command = git_command();
    status_command
        .arg("-C")
        .arg(&root)
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=normal",
            "--",
        ])
        .arg(path.strip_prefix(&root).unwrap_or(path));
    let Some(output) = run_git(status_command) else {
        return GitStatusSnapshot {
            root: Some(root),
            entries: HashMap::new(),
        };
    };
    let mut entries = HashMap::new();
    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| record.len() >= 4)
    {
        let code = &record[..2];
        let path = OsString::from_vec(record[3..].to_vec());
        let status = match code {
            b"??" => GitStatus::Untracked,
            b"AA" | b"DD" | b"AU" | b"UA" | b"DU" | b"UD" | b"UU" => GitStatus::Conflicted,
            code if code.contains(&b'A') => GitStatus::Added,
            code if code.contains(&b'D') => GitStatus::Deleted,
            _ => GitStatus::Modified,
        };
        let changed = root.join(path);
        insert_status(&mut entries, changed.as_path(), &root, status);
    }
    GitStatusSnapshot {
        root: Some(root),
        entries,
    }
}

fn git_command() -> Command {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.untrackedCache=false",
        ])
        .env("GIT_CONFIG_GLOBAL", OsStr::new("/dev/null"))
        .env("GIT_CONFIG_SYSTEM", OsStr::new("/dev/null"))
        .env("GIT_CONFIG_NOSYSTEM", OsStr::new("1"))
        .env("GIT_OPTIONAL_LOCKS", OsStr::new("0"))
        .env("GIT_LITERAL_PATHSPECS", OsStr::new("1"))
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command.process_group(0);
    command
}

fn run_git(mut command: Command) -> Option<Vec<u8>> {
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        stdout
            .take((MAX_GIT_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut output)
            .map(|_| output)
    });
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) | Err(_) => {
                if let Ok(pid) = i32::try_from(child.id()) {
                    let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let output = reader.join().ok()?.ok()?;
    if !status?.success() || output.len() > MAX_GIT_OUTPUT_BYTES {
        return None;
    }
    Some(output)
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
