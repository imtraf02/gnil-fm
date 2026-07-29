use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

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
    selected_paths: Arc<HashSet<PathBuf>>,
    selected_revision: u64,
}

impl SelectionState {
    pub fn clear(&mut self) -> bool {
        let changed =
            self.cursor.is_some() || self.anchor.is_some() || !self.selected_paths.is_empty();
        if !self.selected_paths.is_empty() {
            Arc::make_mut(&mut self.selected_paths).clear();
            self.bump_selected_revision();
        }
        self.cursor = None;
        self.anchor = None;
        changed
    }

    pub fn select_only(&mut self, index: usize, entries: &[FileEntry]) -> bool {
        let Some(entry) = entries.get(index) else {
            return false;
        };
        let selection_changed =
            self.selected_paths.len() != 1 || !self.selected_paths.contains(&entry.path);
        let changed = selection_changed || self.cursor != Some(index) || self.anchor != Some(index);
        self.cursor = Some(index);
        self.anchor = Some(index);
        if selection_changed {
            self.selected_paths = Arc::new(HashSet::from([entry.path.clone()]));
            self.bump_selected_revision();
        }
        changed
    }

    pub fn select_all(&mut self, entries: &[FileEntry]) -> bool {
        let selected_paths: HashSet<_> = entries.iter().map(|entry| entry.path.clone()).collect();
        let cursor = (!entries.is_empty()).then_some(0);
        let selection_changed = *self.selected_paths != selected_paths;
        let changed = selection_changed || self.cursor != cursor || self.anchor != cursor;
        if selection_changed {
            self.selected_paths = Arc::new(selected_paths);
            self.bump_selected_revision();
        }
        self.cursor = cursor;
        self.anchor = cursor;
        changed
    }

    pub fn toggle(&mut self, index: usize, entries: &[FileEntry]) -> bool {
        let Some(entry) = entries.get(index) else {
            return false;
        };
        self.cursor = Some(index);
        self.anchor = Some(index);
        let selected_paths = Arc::make_mut(&mut self.selected_paths);
        if !selected_paths.remove(&entry.path) {
            selected_paths.insert(entry.path.clone());
        }
        self.bump_selected_revision();
        true
    }

    pub fn extend_to(&mut self, index: usize, entries: &[FileEntry]) -> bool {
        self.extend_to_with_mode(index, entries, false)
    }

    pub fn extend_to_additive(&mut self, index: usize, entries: &[FileEntry]) -> bool {
        self.extend_to_with_mode(index, entries, true)
    }

    fn extend_to_with_mode(&mut self, index: usize, entries: &[FileEntry], additive: bool) -> bool {
        if index >= entries.len() {
            return false;
        }
        let anchor = self.anchor.or(self.cursor).unwrap_or(index);
        let (start, end) = if anchor <= index {
            (anchor, index)
        } else {
            (index, anchor)
        };
        let mut selected_paths = if additive {
            self.selected_paths.as_ref().clone()
        } else {
            HashSet::new()
        };
        selected_paths.extend(entries[start..=end].iter().map(|entry| entry.path.clone()));
        let selection_changed = *self.selected_paths != selected_paths;
        let changed =
            selection_changed || self.cursor != Some(index) || self.anchor != Some(anchor);
        if selection_changed {
            self.selected_paths = Arc::new(selected_paths);
            self.bump_selected_revision();
        }
        self.cursor = Some(index);
        self.anchor = Some(anchor);
        changed
    }

    pub fn apply_indices(
        &mut self,
        baseline: &Self,
        indices: impl IntoIterator<Item = usize>,
        merge: SelectionMerge,
        cursor: Option<usize>,
        entries: &[FileEntry],
    ) -> bool {
        let hit_paths: HashSet<_> = indices
            .into_iter()
            .filter_map(|index| entries.get(index))
            .map(|entry| entry.path.clone())
            .collect();

        let selected_paths = match merge {
            SelectionMerge::Replace => hit_paths,
            SelectionMerge::Union => {
                let mut selected = baseline.selected_paths.as_ref().clone();
                selected.extend(hit_paths);
                selected
            }
            SelectionMerge::Toggle => {
                let mut selected = baseline.selected_paths.as_ref().clone();
                for path in hit_paths {
                    if !selected.remove(&path) {
                        selected.insert(path);
                    }
                }
                selected
            }
        };

        let (cursor, anchor) = if let Some(cursor) = cursor.filter(|index| *index < entries.len()) {
            (Some(cursor), Some(cursor))
        } else if merge == SelectionMerge::Replace {
            (None, None)
        } else {
            (baseline.cursor, baseline.anchor)
        };
        let selection_changed = *self.selected_paths != selected_paths;
        let changed = selection_changed || self.cursor != cursor || self.anchor != anchor;
        if selection_changed {
            self.selected_paths = Arc::new(selected_paths);
            self.bump_selected_revision();
        }
        self.cursor = cursor;
        self.anchor = anchor;
        changed
    }

    pub fn move_cursor(&mut self, delta: isize, entries: &[FileEntry], extend: bool) -> bool {
        if entries.is_empty() {
            return self.clear();
        }
        let current = self.cursor.unwrap_or(usize::from(delta < 0));
        let next = current
            .saturating_add_signed(delta)
            .min(entries.len().saturating_sub(1));
        if extend {
            self.extend_to(next, entries)
        } else {
            self.select_only(next, entries)
        }
    }

    pub fn move_cursor_preserving_selection(
        &mut self,
        delta: isize,
        entries: &[FileEntry],
    ) -> bool {
        if entries.is_empty() {
            return self.clear();
        }
        let current = self.cursor.unwrap_or(usize::from(delta < 0));
        let next = current
            .saturating_add_signed(delta)
            .min(entries.len().saturating_sub(1));
        let changed = self.cursor != Some(next) || self.anchor != Some(next);
        self.cursor = Some(next);
        self.anchor = Some(next);
        changed
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

    #[must_use]
    pub fn selected_revision(&self) -> u64 {
        self.selected_revision
    }

    pub fn retain_existing(&mut self, entries: &[FileEntry]) -> bool {
        let previous_cursor = self.cursor;
        let previous_anchor = self.anchor;
        if self.cursor.is_some_and(|index| index >= entries.len()) {
            self.cursor = entries.len().checked_sub(1);
        }
        if self.anchor.is_some_and(|index| index >= entries.len()) {
            self.anchor = self.cursor;
        }
        if self.selected_paths.is_empty() {
            return self.cursor != previous_cursor || self.anchor != previous_anchor;
        }

        let existing: HashSet<&Path> = entries.iter().map(|entry| entry.path.as_path()).collect();
        let mut selected_paths = self.selected_paths.as_ref().clone();
        selected_paths.retain(|path| existing.contains(path.as_path()));
        let selection_changed = *self.selected_paths != selected_paths;
        if selection_changed {
            self.selected_paths = Arc::new(selected_paths);
            self.bump_selected_revision();
        }
        selection_changed || self.cursor != previous_cursor || self.anchor != previous_anchor
    }

    fn bump_selected_revision(&mut self) {
        self.selected_revision = self.selected_revision.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use crate::{EntryMetadata, FileEntry, FileKind, FileMetadata};

    use super::{SelectionMerge, SelectionState};

    fn entries() -> Vec<FileEntry> {
        (0..4)
            .map(|index| FileEntry {
                path: PathBuf::from(format!("/{index}")),
                name: index.to_string(),
                kind: FileKind::File,
                hidden: false,
                metadata: EntryMetadata::Ready(FileMetadata::default()),
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
    fn selecting_the_current_entry_is_a_noop() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_only(2, &entries);
        let previous = selection.clone();

        selection.select_only(2, &entries);

        assert_eq!(selection, previous);
    }

    #[test]
    fn cloned_selection_shares_paths_until_mutated() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_only(0, &entries);
        let baseline = selection.clone();

        assert!(Arc::ptr_eq(
            &selection.selected_paths,
            &baseline.selected_paths
        ));
        selection.toggle(1, &entries);
        assert!(!Arc::ptr_eq(
            &selection.selected_paths,
            &baseline.selected_paths
        ));
        assert_eq!(baseline.selected_count(), 1);
        assert_eq!(selection.selected_count(), 2);
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
        selection.apply_indices(&baseline, [1, 2], SelectionMerge::Toggle, Some(2), &entries);
        assert_eq!(
            selection.effective_paths(&entries),
            vec![PathBuf::from("/0"), PathBuf::from("/1")]
        );

        selection.apply_indices(&baseline, [1, 2], SelectionMerge::Union, Some(2), &entries);
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
