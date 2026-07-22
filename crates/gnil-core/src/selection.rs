use std::{collections::BTreeSet, path::PathBuf};

use crate::FileEntry;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SelectionState {
    pub cursor: Option<usize>,
    pub anchor: Option<usize>,
    selected_paths: BTreeSet<PathBuf>,
}

impl SelectionState {
    pub fn clear(&mut self) {
        self.cursor = None;
        self.anchor = None;
        self.selected_paths.clear();
    }

    pub fn select_only(&mut self, index: usize, entries: &[FileEntry]) {
        if index >= entries.len() {
            return;
        }
        self.cursor = Some(index);
        self.anchor = Some(index);
        self.selected_paths.clear();
    }

    pub fn select_all(&mut self, entries: &[FileEntry]) {
        self.selected_paths = entries.iter().map(|entry| entry.path.clone()).collect();
        self.cursor = (!entries.is_empty()).then_some(0);
        self.anchor = self.cursor;
    }

    pub fn toggle(&mut self, index: usize, entries: &[FileEntry]) {
        let Some(entry) = entries.get(index) else {
            return;
        };
        self.cursor = Some(index);
        self.anchor = Some(index);
        if !self.selected_paths.remove(&entry.path) {
            self.selected_paths.insert(entry.path.clone());
        }
    }

    pub fn extend_to(&mut self, index: usize, entries: &[FileEntry]) {
        if index >= entries.len() {
            return;
        }
        let anchor = self.anchor.or(self.cursor).unwrap_or(index);
        let (start, end) = if anchor <= index {
            (anchor, index)
        } else {
            (index, anchor)
        };
        self.selected_paths.clear();
        self.selected_paths
            .extend(entries[start..=end].iter().map(|entry| entry.path.clone()));
        self.cursor = Some(index);
        self.anchor = Some(anchor);
    }

    pub fn move_cursor(&mut self, delta: isize, entries: &[FileEntry], extend: bool) {
        if entries.is_empty() {
            self.clear();
            return;
        }
        let current = self.cursor.unwrap_or(usize::from(delta < 0));
        let next = current
            .saturating_add_signed(delta)
            .min(entries.len().saturating_sub(1));
        if extend {
            self.extend_to(next, entries);
        } else {
            self.select_only(next, entries);
        }
    }

    pub fn move_cursor_preserving_selection(&mut self, delta: isize, entries: &[FileEntry]) {
        if entries.is_empty() {
            self.clear();
            return;
        }
        let current = self.cursor.unwrap_or(usize::from(delta < 0));
        let next = current
            .saturating_add_signed(delta)
            .min(entries.len().saturating_sub(1));
        self.cursor = Some(next);
        self.anchor = Some(next);
    }

    #[must_use]
    pub fn effective_paths(&self, entries: &[FileEntry]) -> Vec<PathBuf> {
        if self.selected_paths.is_empty() {
            return self
                .cursor
                .and_then(|index| entries.get(index))
                .map(|entry| vec![entry.path.clone()])
                .unwrap_or_default();
        }
        entries
            .iter()
            .filter(|entry| self.selected_paths.contains(&entry.path))
            .map(|entry| entry.path.clone())
            .collect()
    }

    #[must_use]
    pub fn is_highlighted(&self, index: usize, entry: &FileEntry) -> bool {
        self.cursor == Some(index) || self.selected_paths.contains(&entry.path)
    }

    #[must_use]
    pub fn selected_count(&self) -> usize {
        self.selected_paths.len()
    }

    pub fn retain_existing(&mut self, entries: &[FileEntry]) {
        let existing: BTreeSet<_> = entries.iter().map(|entry| entry.path.clone()).collect();
        self.selected_paths.retain(|path| existing.contains(path));
        if self.cursor.is_some_and(|index| index >= entries.len()) {
            self.cursor = entries.len().checked_sub(1);
        }
        if self.anchor.is_some_and(|index| index >= entries.len()) {
            self.anchor = self.cursor;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{FileEntry, FileKind, FileMetadata};

    use super::SelectionState;

    fn entries() -> Vec<FileEntry> {
        (0..4)
            .map(|index| FileEntry {
                path: PathBuf::from(format!("/{index}")),
                name: index.to_string(),
                kind: FileKind::File,
                hidden: false,
                metadata: FileMetadata::default(),
                git_status: None,
            })
            .collect()
    }

    #[test]
    fn range_and_toggle_keep_visual_order() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_only(1, &entries);
        selection.extend_to(3, &entries);
        assert_eq!(
            selection.effective_paths(&entries),
            vec![
                PathBuf::from("/1"),
                PathBuf::from("/2"),
                PathBuf::from("/3")
            ]
        );
        selection.toggle(2, &entries);
        assert_eq!(
            selection.effective_paths(&entries),
            vec![PathBuf::from("/1"), PathBuf::from("/3")]
        );
    }

    #[test]
    fn select_all_tracks_every_entry_and_handles_empty_lists() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_all(&entries);
        assert_eq!(selection.effective_paths(&entries).len(), entries.len());
        assert_eq!(selection.cursor, Some(0));

        selection.select_all(&[]);
        assert!(selection.effective_paths(&[]).is_empty());
        assert_eq!(selection.cursor, None);
    }
}
