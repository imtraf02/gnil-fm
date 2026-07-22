use std::collections::HashSet;

use gnil_core::{FileEntry, FileKind, SelectionState};
use gnil_fs::is_archive_candidate;
use gpui::{Pixels, Point};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ActionMenuPlacement {
    Header,
    Cursor(Point<Pixels>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileMenuCommand {
    Open,
    Extract,
    ExtractTo,
    Copy,
    Cut,
    Paste,
    Rename,
    CreateSymlink,
    Permissions,
    CopyPathAbsolute,
    CopyPathRelative,
    Trash,
    DeletePermanently,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuEntry {
    Action {
        command: FileMenuCommand,
        label: &'static str,
        shortcut: Option<&'static str>,
        enabled: bool,
        danger: bool,
    },
    Separator,
}

impl MenuEntry {
    pub(crate) fn enabled_command(&self) -> Option<FileMenuCommand> {
        match self {
            Self::Action {
                command,
                enabled: true,
                ..
            } => Some(*command),
            Self::Action { .. } | Self::Separator => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuAnimationState {
    Opening,
    Closing,
}

#[derive(Clone, Debug)]
pub(crate) struct ActionMenuState {
    pub(crate) placement: ActionMenuPlacement,
    pub(crate) entries: Vec<MenuEntry>,
    pub(crate) focused: Option<usize>,
    pub(crate) animation: MenuAnimationState,
    pub(crate) serial: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct MenuContext {
    pub(crate) selected_count: usize,
    pub(crate) permissions_supported: bool,
    pub(crate) clipboard_valid: bool,
    pub(crate) operation_running: bool,
    pub(crate) all_selected_archives: bool,
}

impl MenuContext {
    pub(crate) fn from_selection(
        selection: &SelectionState,
        entries: &[FileEntry],
        clipboard_valid: bool,
        operation_running: bool,
    ) -> Self {
        let selected_paths = selection.effective_paths(entries);
        let selected: HashSet<_> = selected_paths.iter().collect();
        let selected_entries: Vec<_> = entries
            .iter()
            .filter(|entry| selected.contains(&entry.path))
            .collect();
        Self {
            selected_count: selected_paths.len(),
            permissions_supported: !selected_entries.is_empty()
                && selected_entries
                    .iter()
                    .all(|entry| entry.kind != FileKind::Symlink && entry.metadata.mode.is_some()),
            clipboard_valid,
            operation_running,
            all_selected_archives: !selected_entries.is_empty()
                && selected_entries
                    .iter()
                    .all(|entry| is_archive_candidate(&entry.path)),
        }
    }
}

impl ActionMenuState {
    pub(crate) fn new(placement: ActionMenuPlacement, context: MenuContext, serial: u64) -> Self {
        let has_selection = context.selected_count > 0;
        let single_selection = context.selected_count == 1;
        let writes_enabled = !context.operation_running;
        let mut entries = vec![
            action(
                FileMenuCommand::Open,
                "Open",
                Some("Enter"),
                single_selection,
            ),
            MenuEntry::Separator,
            action(FileMenuCommand::Copy, "Copy", Some("Ctrl+C"), has_selection),
            action(FileMenuCommand::Cut, "Cut", Some("Ctrl+X"), has_selection),
            action(
                FileMenuCommand::Paste,
                "Paste",
                Some("Ctrl+V"),
                context.clipboard_valid && writes_enabled,
            ),
            MenuEntry::Separator,
            action(
                FileMenuCommand::Rename,
                if context.selected_count > 1 {
                    "Bulk rename"
                } else {
                    "Rename"
                },
                Some("F2"),
                has_selection && writes_enabled,
            ),
            action(
                FileMenuCommand::CreateSymlink,
                "New symlink",
                Some("Ctrl+Shift+L"),
                writes_enabled,
            ),
            action(
                FileMenuCommand::Permissions,
                "Permissions",
                Some("Alt+Enter"),
                context.permissions_supported && writes_enabled,
            ),
            MenuEntry::Separator,
            action(
                FileMenuCommand::CopyPathAbsolute,
                "Copy absolute path",
                Some("Ctrl+Shift+C"),
                has_selection,
            ),
            action(
                FileMenuCommand::CopyPathRelative,
                "Copy relative path",
                Some("Ctrl+Alt+C"),
                has_selection,
            ),
            MenuEntry::Separator,
            action(
                FileMenuCommand::Trash,
                "Move to Trash",
                Some("Delete"),
                has_selection && writes_enabled,
            ),
            dangerous_action(
                FileMenuCommand::DeletePermanently,
                "Delete Permanently",
                Some("Shift+Delete"),
                has_selection && writes_enabled,
            ),
        ];
        if context.all_selected_archives {
            entries.splice(
                1..1,
                [
                    action(
                        FileMenuCommand::Extract,
                        "Extract",
                        Some("Ctrl+E"),
                        writes_enabled,
                    ),
                    action(
                        FileMenuCommand::ExtractTo,
                        "Extract to…",
                        Some("Ctrl+Shift+E"),
                        writes_enabled,
                    ),
                ],
            );
        }
        let focused = entries.iter().position(is_selectable);
        Self {
            placement,
            entries,
            focused,
            animation: MenuAnimationState::Opening,
            serial,
        }
    }

    pub(crate) fn move_focus(&mut self, direction: isize) {
        let selectable: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| is_selectable(entry).then_some(index))
            .collect();
        if selectable.is_empty() {
            self.focused = None;
            return;
        }
        let current = self
            .focused
            .and_then(|focused| selectable.iter().position(|index| *index == focused));
        let next = match (current, direction.is_negative()) {
            (Some(index), false) => (index + 1) % selectable.len(),
            (Some(0) | None, true) => selectable.len() - 1,
            (Some(index), true) => index - 1,
            (None, false) => 0,
        };
        self.focused = Some(selectable[next]);
    }

    pub(crate) fn focus_first(&mut self) {
        self.focused = self.entries.iter().position(is_selectable);
    }

    pub(crate) fn focus_last(&mut self) {
        self.focused = self.entries.iter().rposition(is_selectable);
    }

    pub(crate) fn focus(&mut self, index: usize) {
        if self.entries.get(index).is_some_and(is_selectable) {
            self.focused = Some(index);
        }
    }

    pub(crate) fn focused_command(&self) -> Option<FileMenuCommand> {
        self.focused
            .and_then(|index| self.entries.get(index))
            .and_then(MenuEntry::enabled_command)
    }
}

pub(crate) fn prepare_context_selection(
    selection: &mut SelectionState,
    entries: &[FileEntry],
    index: usize,
) -> bool {
    let Some(entry) = entries.get(index) else {
        return false;
    };
    let effective = selection.effective_paths(entries);
    if effective.len() > 1 && effective.iter().any(|path| path == &entry.path) {
        return false;
    }
    let changed = effective.as_slice() != [entry.path.clone()];
    selection.select_only(index, entries);
    changed
}

fn action(
    command: FileMenuCommand,
    label: &'static str,
    shortcut: Option<&'static str>,
    enabled: bool,
) -> MenuEntry {
    MenuEntry::Action {
        command,
        label,
        shortcut,
        enabled,
        danger: false,
    }
}

fn dangerous_action(
    command: FileMenuCommand,
    label: &'static str,
    shortcut: Option<&'static str>,
    enabled: bool,
) -> MenuEntry {
    MenuEntry::Action {
        command,
        label,
        shortcut,
        enabled,
        danger: true,
    }
}

fn is_selectable(entry: &MenuEntry) -> bool {
    entry.enabled_command().is_some()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gnil_core::{FileMetadata, SelectionState};
    use gpui::{point, px};

    use super::*;

    fn entries() -> Vec<FileEntry> {
        vec![
            entry("one", FileKind::File, Some(0o644)),
            entry("two", FileKind::Directory, Some(0o755)),
            entry("link", FileKind::Symlink, Some(0o777)),
        ]
    }

    fn entry(name: &str, kind: FileKind, mode: Option<u32>) -> FileEntry {
        FileEntry {
            path: PathBuf::from(format!("/{name}")),
            name: name.into(),
            kind,
            hidden: false,
            metadata: FileMetadata {
                mode,
                ..FileMetadata::default()
            },
            git_status: None,
        }
    }

    fn command(menu: &ActionMenuState, command: FileMenuCommand) -> &MenuEntry {
        menu.entries
            .iter()
            .find(|entry| {
                matches!(entry, MenuEntry::Action { command: candidate, .. } if *candidate == command)
            })
            .expect("command entry")
    }

    fn is_enabled(entry: &MenuEntry) -> bool {
        matches!(entry, MenuEntry::Action { enabled: true, .. })
    }

    #[test]
    fn empty_selection_exposes_only_context_free_and_valid_paste_actions() {
        let menu = ActionMenuState::new(
            ActionMenuPlacement::Header,
            MenuContext {
                clipboard_valid: true,
                ..MenuContext::default()
            },
            1,
        );
        assert!(!is_enabled(command(&menu, FileMenuCommand::Open)));
        assert!(!is_enabled(command(&menu, FileMenuCommand::Copy)));
        assert!(is_enabled(command(&menu, FileMenuCommand::Paste)));
        assert!(is_enabled(command(&menu, FileMenuCommand::CreateSymlink)));
    }

    #[test]
    fn single_and_multi_selection_set_labels_and_permissions() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_only(0, &entries);
        let single = MenuContext::from_selection(&selection, &entries, false, false);
        let single = ActionMenuState::new(ActionMenuPlacement::Header, single, 1);
        assert!(is_enabled(command(&single, FileMenuCommand::Open)));
        assert!(is_enabled(command(&single, FileMenuCommand::Permissions)));
        assert!(matches!(
            command(&single, FileMenuCommand::Rename),
            MenuEntry::Action {
                label: "Rename",
                ..
            }
        ));

        selection.extend_to(1, &entries);
        let multiple = MenuContext::from_selection(&selection, &entries, false, false);
        let multiple = ActionMenuState::new(ActionMenuPlacement::Header, multiple, 2);
        assert!(!is_enabled(command(&multiple, FileMenuCommand::Open)));
        assert!(matches!(
            command(&multiple, FileMenuCommand::Rename),
            MenuEntry::Action {
                label: "Bulk rename",
                enabled: true,
                ..
            }
        ));
    }

    #[test]
    fn symlinks_disable_permissions_and_operation_locks_writes() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_only(2, &entries);
        let context = MenuContext::from_selection(&selection, &entries, true, true);
        let menu = ActionMenuState::new(ActionMenuPlacement::Header, context, 1);
        assert!(!is_enabled(command(&menu, FileMenuCommand::Permissions)));
        assert!(!is_enabled(command(&menu, FileMenuCommand::Paste)));
        assert!(!is_enabled(command(&menu, FileMenuCommand::Rename)));
        assert!(!is_enabled(command(&menu, FileMenuCommand::Trash)));
        assert!(is_enabled(command(&menu, FileMenuCommand::Copy)));
    }

    #[test]
    fn invalid_clipboard_disables_paste_and_only_permanent_delete_is_dangerous() {
        let menu = ActionMenuState::new(
            ActionMenuPlacement::Cursor(point(px(4.0), px(8.0))),
            MenuContext {
                selected_count: 1,
                permissions_supported: true,
                clipboard_valid: false,
                operation_running: false,
                all_selected_archives: false,
            },
            1,
        );
        assert!(!is_enabled(command(&menu, FileMenuCommand::Paste)));
        assert!(matches!(
            command(&menu, FileMenuCommand::DeletePermanently),
            MenuEntry::Action { danger: true, .. }
        ));
        assert_eq!(
            menu.entries
                .iter()
                .filter(|entry| matches!(entry, MenuEntry::Action { danger: true, .. }))
                .count(),
            1
        );
    }

    #[test]
    fn extraction_actions_are_gradually_disclosed_for_archive_selections() {
        let menu = ActionMenuState::new(
            ActionMenuPlacement::Header,
            MenuContext {
                selected_count: 2,
                all_selected_archives: true,
                ..MenuContext::default()
            },
            1,
        );
        assert!(is_enabled(command(&menu, FileMenuCommand::Extract)));
        assert!(is_enabled(command(&menu, FileMenuCommand::ExtractTo)));

        let regular = ActionMenuState::new(
            ActionMenuPlacement::Header,
            MenuContext {
                selected_count: 1,
                ..MenuContext::default()
            },
            2,
        );
        assert!(
            regular
                .entries
                .iter()
                .all(|entry| entry.enabled_command() != Some(FileMenuCommand::Extract))
        );
    }

    #[test]
    fn keyboard_navigation_wraps_and_skips_disabled_and_separators() {
        let mut menu = ActionMenuState::new(ActionMenuPlacement::Header, MenuContext::default(), 1);
        assert_eq!(menu.focused_command(), Some(FileMenuCommand::CreateSymlink));
        menu.move_focus(1);
        assert_eq!(menu.focused_command(), Some(FileMenuCommand::CreateSymlink));
        menu.move_focus(-1);
        assert_eq!(menu.focused_command(), Some(FileMenuCommand::CreateSymlink));

        let mut populated = ActionMenuState::new(
            ActionMenuPlacement::Header,
            MenuContext {
                selected_count: 2,
                permissions_supported: true,
                clipboard_valid: true,
                operation_running: false,
                all_selected_archives: false,
            },
            2,
        );
        populated.focus_first();
        assert_eq!(populated.focused_command(), Some(FileMenuCommand::Copy));
        populated.move_focus(-1);
        assert_eq!(
            populated.focused_command(),
            Some(FileMenuCommand::DeletePermanently)
        );
        populated.move_focus(1);
        assert_eq!(populated.focused_command(), Some(FileMenuCommand::Copy));
        populated.focus_last();
        assert_eq!(
            populated.focused_command(),
            Some(FileMenuCommand::DeletePermanently)
        );
    }

    #[test]
    fn context_click_replaces_outside_selection_and_preserves_inside_multi_selection() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_only(0, &entries);
        selection.extend_to(1, &entries);
        assert!(!prepare_context_selection(&mut selection, &entries, 1));
        assert_eq!(selection.effective_paths(&entries).len(), 2);

        assert!(prepare_context_selection(&mut selection, &entries, 2));
        assert_eq!(
            selection.effective_paths(&entries),
            vec![PathBuf::from("/link")]
        );
        assert_eq!(selection.cursor, Some(2));
    }
}
