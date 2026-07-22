use std::{
    cmp::Ordering,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum FileKind {
    Directory,
    File,
    Symlink,
    #[default]
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum GitStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
    Conflicted,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileMetadata {
    pub len: u64,
    pub modified_unix_ms: Option<i64>,
    pub mode: Option<u32>,
    pub readonly: bool,
    pub symlink_target: Option<PathBuf>,
    pub mime: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub kind: FileKind,
    pub hidden: bool,
    pub metadata: FileMetadata,
    pub git_status: Option<GitStatus>,
}

impl FileEntry {
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.path.extension().and_then(OsStr::to_str)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DirectorySnapshot {
    pub generation: u64,
    pub path: PathBuf,
    pub entries: Vec<FileEntry>,
    pub unreadable_entries: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum SortField {
    #[default]
    Name,
    Size,
    Modified,
    Kind,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SortSpec {
    pub field: SortField,
    pub direction: SortDirection,
    pub directories_first: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            field: SortField::Name,
            direction: SortDirection::Ascending,
            directories_first: true,
        }
    }
}

impl SortSpec {
    pub fn sort(&self, entries: &mut [FileEntry]) {
        entries.sort_by(|left, right| {
            let directory_order = self.directories_first.then(|| {
                let left_dir = left.kind == FileKind::Directory;
                let right_dir = right.kind == FileKind::Directory;
                right_dir.cmp(&left_dir)
            });

            let mut order = directory_order
                .filter(|order| !order.is_eq())
                .unwrap_or_else(|| match self.field {
                    SortField::Name => natural_cmp(&left.name, &right.name),
                    SortField::Size => left.metadata.len.cmp(&right.metadata.len),
                    SortField::Modified => left
                        .metadata
                        .modified_unix_ms
                        .cmp(&right.metadata.modified_unix_ms),
                    SortField::Kind => kind_rank(left.kind)
                        .cmp(&kind_rank(right.kind))
                        .then_with(|| natural_cmp(&left.name, &right.name)),
                });

            if self.direction == SortDirection::Descending {
                order = order.reverse();
            }
            order
        });
    }
}

const fn kind_rank(kind: FileKind) -> u8 {
    match kind {
        FileKind::Directory => 0,
        FileKind::File => 1,
        FileKind::Symlink => 2,
        FileKind::Other => 3,
    }
}

/// Case-insensitive natural ordering (`file2` before `file10`).
#[must_use]
pub fn natural_cmp(left: &str, right: &str) -> Ordering {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() && right_index < right.len() {
        if left[left_index].is_ascii_digit() && right[right_index].is_ascii_digit() {
            let left_start = left_index;
            let right_start = right_index;
            while left_index < left.len() && left[left_index].is_ascii_digit() {
                left_index += 1;
            }
            while right_index < right.len() && right[right_index].is_ascii_digit() {
                right_index += 1;
            }
            let left_number = &left[left_start..left_index];
            let right_number = &right[right_start..right_index];
            let left_trimmed = trim_ascii_zeroes(left_number);
            let right_trimmed = trim_ascii_zeroes(right_number);
            let order = left_trimmed
                .len()
                .cmp(&right_trimmed.len())
                .then_with(|| left_trimmed.cmp(right_trimmed))
                .then_with(|| left_number.len().cmp(&right_number.len()));
            if !order.is_eq() {
                return order;
            }
        } else {
            let order = left[left_index]
                .to_ascii_lowercase()
                .cmp(&right[right_index].to_ascii_lowercase());
            if !order.is_eq() {
                return order;
            }
            left_index += 1;
            right_index += 1;
        }
    }
    left.len().cmp(&right.len())
}

fn trim_ascii_zeroes(value: &[u8]) -> &[u8] {
    let first_non_zero = value
        .iter()
        .position(|byte| *byte != b'0')
        .unwrap_or(value.len());
    &value[first_non_zero..]
}

#[must_use]
pub fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort_orders_numbers_for_humans() {
        let mut names = ["file10a", "file02", "file2", "File1", "file10b"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["File1", "file2", "file02", "file10a", "file10b"]);
    }
}
