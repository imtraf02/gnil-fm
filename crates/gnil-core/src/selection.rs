use std::{collections::BTreeSet, path::PathBuf};

use crate::FileEntry;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionMerge {
    Replace,
    Union,
    Toggle,
}

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
        let Some(entry) = entries.get(index) else {
            return;
        };
        self.cursor = Some(index);
        self.anchor = Some(index);
        self.selected_paths.clear();
        self.selected_paths.insert(entry.path.clone());
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
        self.extend_to_with_mode(index, entries, false);
    }

    pub fn extend_to_additive(&mut self, index: usize, entries: &[FileEntry]) {
        self.extend_to_with_mode(index, entries, true);
    }

    fn extend_to_with_mode(&mut self, index: usize, entries: &[FileEntry], additive: bool) {
        if index >= entries.len() {
            return;
        }
        let anchor = self.anchor.or(self.cursor).unwrap_or(index);
        let (start, end) = if anchor <= index {
            (anchor, index)
        } else {
            (index, anchor)
        };
        if !additive {
            self.selected_paths.clear();
        }
        self.selected_paths
            .extend(entries[start..=end].iter().map(|entry| entry.path.clone()));
        self.cursor = Some(index);
        self.anchor = Some(anchor);
    }

    pub fn apply_indices(
        &mut self,
        baseline: &Self,
        indices: impl IntoIterator<Item = usize>,
        merge: SelectionMerge,
        cursor: Option<usize>,
        entries: &[FileEntry],
    ) {
        let hit_paths: BTreeSet<_> = indices
            .into_iter()
            .filter_map(|index| entries.get(index))
            .map(|entry| entry.path.clone())
            .collect();

        self.selected_paths = match merge {
            SelectionMerge::Replace => hit_paths,
            SelectionMerge::Union => baseline
                .selected_paths
                .union(&hit_paths)
                .cloned()
                .collect(),
            SelectionMerge::Toggle => baseline
                .selected_paths
                .symmetric_difference(&hit_paths)
                .cloned()
                .collect(),
        };

        if let Some(cursor) = cursor.filter(|index| *index < entries.len()) {
            self.cursor = Some(cursor);
            self.anchor = Some(cursor);
        } else if merge == SelectionMerge::Replace {
            self.cursor = None;
            self.anchor = None;
        } else {
            self.cursor = baseline.cursor;
            self.anchor = baseline.anchor;
        }
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
        entries
            .iter()
            .filter(|entry| self.selected_paths.contains(&entry.path))
            .map(|entry| entry.path.clone())
            .collect()
    }

    #[must_use]
    pub fn is_highlighted(&self, _index: usize, entry: &FileEntry) -> bool {
        self.selected_paths.contains(&entry.path)
    }

    #[must_use]
    pub fn selected_count(&self) -> usize {
        self.selected_paths.len()
    }

    #[must_use]
    pub fn contains_path(&self, path: &std::path::Path) -> bool {
        self.selected_paths.contains(path)
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

    use super::{SelectionMerge, SelectionState};

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
    fn single_selection_is_counted_and_can_be_toggled_off() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_only(2, &entries);
        assert_eq!(selection.selected_count(), 1);
        assert!(selection.is_highlighted(2, &entries[2]));

        selection.toggle(2, &entries);
        assert_eq!(selection.selected_count(), 0);
        assert!(selection.effective_paths(&entries).is_empty());
        assert!(!selection.is_highlighted(2, &entries[2]));
    }

    #[test]
    fn additive_range_keeps_existing_paths() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_only(0, &entries);
        selection.toggle(3, &entries);
        selection.anchor = Some(1);
        selection.extend_to_additive(2, &entries);

        assert_eq!(
            selection.effective_paths(&entries),
            vec![
                PathBuf::from("/0"),
                PathBuf::from("/1"),
                PathBuf::from("/2"),
                PathBuf::from("/3")
            ]
        );
    }

    #[test]
    fn rubber_band_merges_against_a_stable_baseline() {
        let entries = entries();
        let mut baseline = SelectionState::default();
        baseline.select_only(0, &entries);
        baseline.toggle(2, &entries);

        let mut selection = SelectionState::default();
        selection.apply_indices(
            &baseline,
            [1, 2],
            SelectionMerge::Toggle,
            Some(2),
            &entries,
        );
        assert_eq!(
            selection.effective_paths(&entries),
            vec![PathBuf::from("/0"), PathBuf::from("/1")]
        );

        selection.apply_indices(
            &baseline,
            [1, 2],
            SelectionMerge::Union,
            Some(2),
            &entries,
        );
        assert_eq!(selection.effective_paths(&entries).len(), 3);

        selection.apply_indices(
            &baseline,
            std::iter::empty(),
            SelectionMerge::Replace,
            None,
            &entries,
        );
        assert!(selection.effective_paths(&entries).is_empty());
        assert_eq!(selection.cursor, None);
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
