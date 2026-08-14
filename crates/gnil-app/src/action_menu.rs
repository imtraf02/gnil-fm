use std::collections::HashSet;

use gnil_core::{FileEntry, FileKind, SelectionState};
use gnil_fs::is_archive_candidate;
use gpui::{Pixels, Point};

use crate::open_with::DesktopApplication;

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
    ToggleFavorite,
    CreateSymlink,
    Permissions,
    CopyPathAbsolute,
    CopyPathRelative,
    Trash,
    DeletePermanently,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileMenuSubmenu {
    OpenWith,
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
    Submenu {
        submenu: FileMenuSubmenu,
        label: &'static str,
        enabled: bool,
    },
    Separator,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MenuToolbarAction {
    pub(crate) command: FileMenuCommand,
    pub(crate) label: &'static str,
    pub(crate) enabled: bool,
}

impl MenuToolbarAction {
    pub(crate) fn enabled_command(&self) -> Option<FileMenuCommand> {
        self.enabled.then_some(self.command)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuFocusTarget {
    Toolbar(usize),
    Entry(usize),
}

impl MenuEntry {
    pub(crate) fn enabled_command(&self) -> Option<FileMenuCommand> {
        match self {
            Self::Action {
                command,
                enabled: true,
                ..
            } => Some(*command),
            Self::Action { .. } | Self::Submenu { .. } | Self::Separator => None,
        }
    }

    pub(crate) fn enabled_submenu(&self) -> Option<FileMenuSubmenu> {
        match self {
            Self::Submenu {
                submenu,
                enabled: true,
                ..
            } => Some(*submenu),
            Self::Action { .. } | Self::Submenu { .. } | Self::Separator => None,
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
    pub(crate) toolbar_actions: Vec<MenuToolbarAction>,
    pub(crate) entries: Vec<MenuEntry>,
    pub(crate) focused: Option<MenuFocusTarget>,
    pub(crate) open_with_submenu: Option<OpenWithSubmenuState>,
    pub(crate) animation: MenuAnimationState,
    pub(crate) serial: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct OpenWithSubmenuState {
    pub(crate) path: std::path::PathBuf,
    pub(crate) mime_type: String,
    pub(crate) applications: Vec<DesktopApplication>,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) focused: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ActionMenuActivation {
    Command(FileMenuCommand),
    Submenu(FileMenuSubmenu),
    OpenWithApplication(DesktopApplication),
    ChooseAnotherApplication,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct MenuContext {
    pub(crate) selected_count: usize,
    pub(crate) open_with_eligible: bool,
    pub(crate) permissions_supported: bool,
    pub(crate) clipboard_valid: bool,
    pub(crate) operation_running: bool,
    pub(crate) all_selected_archives: bool,
    pub(crate) favorite_eligible: bool,
    pub(crate) selected_is_favorite: bool,
}

impl MenuContext {
    pub(crate) fn from_selection(
        selection: &SelectionState,
        entries: &[FileEntry],
        favorites: &[std::path::PathBuf],
        clipboard_valid: bool,
        operation_running: bool,
    ) -> Self {
        let selected_paths = selection.effective_paths(entries);
        let selected: HashSet<_> = selected_paths.iter().collect();
        let selected_entries: Vec<_> = entries
            .iter()
            .filter(|entry| selected.contains(&entry.path))
            .collect();
        let favorite_eligible =
            selected_entries.len() == 1 && selected_entries[0].is_directory_like();
        let open_with_eligible = selected_entries.len() == 1
            && (selected_entries[0].kind == FileKind::File
                || (selected_entries[0].kind == FileKind::Symlink
                    && selected_entries[0]
                        .metadata()
                        .and_then(|metadata| metadata.symlink_target_kind)
                        == Some(FileKind::File)));
        Self {
            selected_count: selected_paths.len(),
            open_with_eligible,
            permissions_supported: !selected_entries.is_empty()
                && selected_entries.iter().all(|entry| {
                    entry.kind != FileKind::Symlink
                        && entry
                            .metadata()
                            .is_some_and(|metadata| metadata.mode.is_some())
                }),
            clipboard_valid,
            operation_running,
            all_selected_archives: !selected_entries.is_empty()
                && selected_entries
                    .iter()
                    .all(|entry| is_archive_candidate(&entry.path)),
            favorite_eligible,
            selected_is_favorite: favorite_eligible
                && favorites
                    .iter()
                    .any(|path| path == &selected_entries[0].path),
        }
    }
}

impl ActionMenuState {
    pub(crate) fn new(placement: ActionMenuPlacement, context: MenuContext, serial: u64) -> Self {
        let has_selection = context.selected_count > 0;
        let single_selection = context.selected_count == 1;
        let writes_enabled = true;
        let toolbar_actions = vec![
            toolbar_action(FileMenuCommand::Cut, "Cut", has_selection),
            toolbar_action(FileMenuCommand::Copy, "Copy", has_selection),
            toolbar_action(
                FileMenuCommand::Paste,
                "Paste",
                context.clipboard_valid && writes_enabled,
            ),
            toolbar_action(
                FileMenuCommand::Rename,
                if context.selected_count > 1 {
                    "Bulk rename"
                } else {
                    "Rename"
                },
                has_selection && writes_enabled,
            ),
            toolbar_action(
                FileMenuCommand::Trash,
                "Move to Trash",
                has_selection && writes_enabled,
            ),
        ];
        let mut entries = vec![
            action(
                FileMenuCommand::Open,
                "Open",
                Some("Enter"),
                single_selection,
            ),
            submenu(
                FileMenuSubmenu::OpenWith,
                "Open with",
                context.open_with_eligible,
            ),
            MenuEntry::Separator,
            action(
                FileMenuCommand::CreateSymlink,
                "New symlink",
                Some("Ctrl+Shift+L"),
                writes_enabled,
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
            action(
                FileMenuCommand::Permissions,
                "Properties",
                Some("Alt+Enter"),
                context.permissions_supported && writes_enabled,
            ),
            MenuEntry::Separator,
            dangerous_action(
                FileMenuCommand::DeletePermanently,
                "Delete Permanently",
                Some("Shift+Delete"),
                has_selection && writes_enabled,
            ),
        ];
        add_contextual_actions(&mut entries, context, writes_enabled);
        let mut state = Self {
            placement,
            toolbar_actions,
            entries,
            focused: None,
            open_with_submenu: None,
            animation: MenuAnimationState::Opening,
            serial,
        };
        state.focus_first();
        state
    }

    pub(crate) fn move_focus(&mut self, direction: isize) {
        if let Some(submenu) = self.open_with_submenu.as_mut() {
            submenu.move_focus(direction);
            return;
        }
        let selectable = self.selectable_targets();
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
        if let Some(submenu) = self.open_with_submenu.as_mut() {
            submenu.focused = Some(0);
            return;
        }
        self.focused = self.selectable_targets().into_iter().next();
    }

    pub(crate) fn focus_last(&mut self) {
        if let Some(submenu) = self.open_with_submenu.as_mut() {
            submenu.focused = Some(submenu.applications.len());
            return;
        }
        self.focused = self.selectable_targets().into_iter().last();
    }

    pub(crate) fn focus_entry(&mut self, index: usize) {
        if self.entries.get(index).is_some_and(is_selectable) {
            self.focused = Some(MenuFocusTarget::Entry(index));
            if self
                .entries
                .get(index)
                .and_then(MenuEntry::enabled_submenu)
                .is_none()
            {
                self.close_submenu();
            }
        }
    }

    pub(crate) fn focus_toolbar(&mut self, index: usize) {
        if self
            .toolbar_actions
            .get(index)
            .is_some_and(|action| action.enabled)
        {
            self.focused = Some(MenuFocusTarget::Toolbar(index));
            self.close_submenu();
        }
    }

    pub(crate) fn focused_activation(&self) -> Option<ActionMenuActivation> {
        if let Some(submenu) = &self.open_with_submenu {
            let focused = submenu.focused?;
            return submenu
                .applications
                .get(focused)
                .cloned()
                .map(ActionMenuActivation::OpenWithApplication)
                .or_else(|| {
                    (focused == submenu.applications.len())
                        .then_some(ActionMenuActivation::ChooseAnotherApplication)
                });
        }
        match self.focused? {
            MenuFocusTarget::Toolbar(index) => self
                .toolbar_actions
                .get(index)
                .and_then(MenuToolbarAction::enabled_command)
                .map(ActionMenuActivation::Command),
            MenuFocusTarget::Entry(index) => {
                let entry = self.entries.get(index)?;
                entry
                    .enabled_command()
                    .map(ActionMenuActivation::Command)
                    .or_else(|| entry.enabled_submenu().map(ActionMenuActivation::Submenu))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn focused_command(&self) -> Option<FileMenuCommand> {
        match self.focused_activation() {
            Some(ActionMenuActivation::Command(command)) => Some(command),
            Some(
                ActionMenuActivation::Submenu(_)
                | ActionMenuActivation::OpenWithApplication(_)
                | ActionMenuActivation::ChooseAnotherApplication,
            )
            | None => None,
        }
    }

    pub(crate) fn open_open_with_submenu(&mut self, path: std::path::PathBuf, mime_type: String) {
        if self
            .open_with_submenu
            .as_ref()
            .is_some_and(|submenu| submenu.path == path && submenu.mime_type == mime_type)
        {
            return;
        }
        self.open_with_submenu = Some(OpenWithSubmenuState {
            path,
            mime_type,
            applications: Vec::new(),
            loading: true,
            error: None,
            focused: Some(0),
        });
    }

    pub(crate) fn close_submenu(&mut self) -> bool {
        self.open_with_submenu.take().is_some()
    }

    pub(crate) fn focus_open_with(&mut self, index: usize) {
        if let Some(submenu) = self.open_with_submenu.as_mut()
            && index <= submenu.applications.len()
        {
            submenu.focused = Some(index);
        }
    }

    fn selectable_targets(&self) -> Vec<MenuFocusTarget> {
        self.toolbar_actions
            .iter()
            .enumerate()
            .filter_map(|(index, action)| action.enabled.then_some(MenuFocusTarget::Toolbar(index)))
            .chain(
                self.entries
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| {
                        is_selectable(entry).then_some(MenuFocusTarget::Entry(index))
                    }),
            )
            .collect()
    }
}

impl OpenWithSubmenuState {
    fn move_focus(&mut self, direction: isize) {
        let item_count = self.applications.len() + 1;
        let current = self.focused.unwrap_or_default().min(item_count - 1);
        self.focused = Some(if direction.is_negative() {
            if current == 0 {
                item_count - 1
            } else {
                current - 1
            }
        } else {
            (current + 1) % item_count
        });
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
    if selection.selected_count() > 1 && selection.contains_path(&entry.path) {
        return false;
    }
    selection.select_only(index, entries)
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

fn submenu(submenu: FileMenuSubmenu, label: &'static str, enabled: bool) -> MenuEntry {
    MenuEntry::Submenu {
        submenu,
        label,
        enabled,
    }
}

fn toolbar_action(
    command: FileMenuCommand,
    label: &'static str,
    enabled: bool,
) -> MenuToolbarAction {
    MenuToolbarAction {
        command,
        label,
        enabled,
    }
}

fn add_contextual_actions(
    entries: &mut Vec<MenuEntry>,
    context: MenuContext,
    writes_enabled: bool,
) {
    if context.all_selected_archives {
        entries.splice(
            2..2,
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
    if !context.favorite_eligible {
        return;
    }
    entries.insert(
        2,
        action(
            FileMenuCommand::ToggleFavorite,
            if context.selected_is_favorite {
                "Remove from Favorites"
            } else {
                "Add to Favorites"
            },
            Some("Ctrl+D"),
            true,
        ),
    );
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
    entry.enabled_command().is_some() || entry.enabled_submenu().is_some()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gnil_core::{EntryMetadata, FileMetadata, SelectionState};
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
            metadata: EntryMetadata::Ready(FileMetadata {
                mode,
                ..FileMetadata::default()
            }),
            git_status: None,
        }
    }

    fn menu_entry(menu: &ActionMenuState, command: FileMenuCommand) -> &MenuEntry {
        menu.entries
            .iter()
            .find(|entry| {
                matches!(entry, MenuEntry::Action { command: candidate, .. } if *candidate == command)
            })
            .expect("command entry")
    }

    fn toolbar_action(menu: &ActionMenuState, command: FileMenuCommand) -> &MenuToolbarAction {
        menu.toolbar_actions
            .iter()
            .find(|action| action.command == command)
            .expect("toolbar action")
    }

    fn menu_submenu(menu: &ActionMenuState, submenu: FileMenuSubmenu) -> &MenuEntry {
        menu.entries
            .iter()
            .find(|entry| {
                matches!(entry, MenuEntry::Submenu { submenu: candidate, .. } if *candidate == submenu)
            })
            .expect("submenu entry")
    }

    fn is_entry_enabled(entry: &MenuEntry) -> bool {
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
        assert!(!is_entry_enabled(menu_entry(&menu, FileMenuCommand::Open)));
        assert!(!toolbar_action(&menu, FileMenuCommand::Copy).enabled);
        assert!(toolbar_action(&menu, FileMenuCommand::Paste).enabled);
        assert!(is_entry_enabled(menu_entry(
            &menu,
            FileMenuCommand::CreateSymlink
        )));
    }

    #[test]
    fn single_and_multi_selection_set_labels_and_permissions() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_only(0, &entries);
        let single = MenuContext::from_selection(&selection, &entries, &[], false, false);
        let single = ActionMenuState::new(ActionMenuPlacement::Header, single, 1);
        assert!(is_entry_enabled(menu_entry(&single, FileMenuCommand::Open)));
        assert!(is_entry_enabled(menu_entry(
            &single,
            FileMenuCommand::Permissions
        )));
        assert!(matches!(
            menu_entry(&single, FileMenuCommand::Permissions),
            MenuEntry::Action {
                label: "Properties",
                shortcut: Some("Alt+Enter"),
                ..
            }
        ));
        assert!(matches!(
            toolbar_action(&single, FileMenuCommand::Rename),
            MenuToolbarAction {
                label: "Rename",
                ..
            }
        ));
        assert!(matches!(
            menu_submenu(&single, FileMenuSubmenu::OpenWith),
            MenuEntry::Submenu { enabled: true, .. }
        ));

        selection.extend_to(1, &entries);
        let multiple = MenuContext::from_selection(&selection, &entries, &[], false, false);
        let multiple = ActionMenuState::new(ActionMenuPlacement::Header, multiple, 2);
        assert!(!is_entry_enabled(menu_entry(
            &multiple,
            FileMenuCommand::Open
        )));
        assert!(matches!(
            toolbar_action(&multiple, FileMenuCommand::Rename),
            MenuToolbarAction {
                label: "Bulk rename",
                enabled: true,
                ..
            }
        ));
        assert!(matches!(
            menu_submenu(&multiple, FileMenuSubmenu::OpenWith),
            MenuEntry::Submenu { enabled: false, .. }
        ));
    }

    #[test]
    fn open_with_accepts_files_and_file_symlinks_but_not_folders() {
        let mut entries = entries();
        if let EntryMetadata::Ready(metadata) = &mut entries[2].metadata {
            metadata.symlink_target_kind = Some(FileKind::File);
        }
        let mut selection = SelectionState::default();

        selection.select_only(2, &entries);
        let symlink = MenuContext::from_selection(&selection, &entries, &[], false, false);
        assert!(symlink.open_with_eligible);

        selection.select_only(1, &entries);
        let folder = MenuContext::from_selection(&selection, &entries, &[], false, false);
        assert!(!folder.open_with_eligible);
    }

    #[test]
    fn open_with_submenu_keyboard_focuses_apps_then_choose_another() {
        let mut menu = ActionMenuState::new(
            ActionMenuPlacement::Header,
            MenuContext {
                selected_count: 1,
                open_with_eligible: true,
                ..MenuContext::default()
            },
            1,
        );
        menu.open_open_with_submenu(PathBuf::from("/tmp/file.txt"), "text/plain".into());
        let submenu = menu.open_with_submenu.as_mut().expect("open submenu");
        submenu.loading = false;
        submenu.applications.push(DesktopApplication {
            desktop_id: "editor.desktop".into(),
            name: "Editor".into(),
            generic_name: Some("Text Editor".into()),
            desktop_file: PathBuf::from("/tmp/editor.desktop"),
            is_default: true,
            compatible: true,
            declared_compatible: true,
        });
        menu.open_open_with_submenu(PathBuf::from("/tmp/file.txt"), "text/plain".into());
        let submenu = menu.open_with_submenu.as_ref().expect("open submenu");
        assert!(!submenu.loading);
        assert_eq!(submenu.applications.len(), 1);

        assert!(matches!(
            menu.focused_activation(),
            Some(ActionMenuActivation::OpenWithApplication(_))
        ));
        menu.move_focus(1);
        assert_eq!(
            menu.focused_activation(),
            Some(ActionMenuActivation::ChooseAnotherApplication)
        );
        assert!(menu.close_submenu());
    }

    #[test]
    fn folder_context_menu_toggles_favorite_label() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_only(1, &entries);

        let add = MenuContext::from_selection(&selection, &entries, &[], false, false);
        let add = ActionMenuState::new(ActionMenuPlacement::Header, add, 1);
        assert!(matches!(
            menu_entry(&add, FileMenuCommand::ToggleFavorite),
            MenuEntry::Action {
                label: "Add to Favorites",
                enabled: true,
                ..
            }
        ));

        let favorites = [entries[1].path.clone()];
        let remove = MenuContext::from_selection(&selection, &entries, &favorites, false, false);
        let remove = ActionMenuState::new(ActionMenuPlacement::Header, remove, 2);
        assert!(matches!(
            menu_entry(&remove, FileMenuCommand::ToggleFavorite),
            MenuEntry::Action {
                label: "Remove from Favorites",
                enabled: true,
                ..
            }
        ));
    }

    #[test]
    fn symlinks_disable_permissions_while_an_active_queue_keeps_writes_available() {
        let entries = entries();
        let mut selection = SelectionState::default();
        selection.select_only(2, &entries);
        let context = MenuContext::from_selection(&selection, &entries, &[], true, true);
        let menu = ActionMenuState::new(ActionMenuPlacement::Header, context, 1);
        assert!(!is_entry_enabled(menu_entry(
            &menu,
            FileMenuCommand::Permissions
        )));
        assert!(toolbar_action(&menu, FileMenuCommand::Paste).enabled);
        assert!(toolbar_action(&menu, FileMenuCommand::Rename).enabled);
        assert!(toolbar_action(&menu, FileMenuCommand::Trash).enabled);
        assert!(toolbar_action(&menu, FileMenuCommand::Copy).enabled);
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
                ..MenuContext::default()
            },
            1,
        );
        assert!(!toolbar_action(&menu, FileMenuCommand::Paste).enabled);
        assert!(matches!(
            menu_entry(&menu, FileMenuCommand::DeletePermanently),
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
        assert!(is_entry_enabled(menu_entry(
            &menu,
            FileMenuCommand::Extract
        )));
        assert!(is_entry_enabled(menu_entry(
            &menu,
            FileMenuCommand::ExtractTo
        )));

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
                ..MenuContext::default()
            },
            2,
        );
        populated.focus_first();
        assert_eq!(populated.focused_command(), Some(FileMenuCommand::Cut));
        populated.move_focus(-1);
        assert_eq!(
            populated.focused_command(),
            Some(FileMenuCommand::DeletePermanently)
        );
        populated.move_focus(1);
        assert_eq!(populated.focused_command(), Some(FileMenuCommand::Cut));
        populated.focus_last();
        assert_eq!(
            populated.focused_command(),
            Some(FileMenuCommand::DeletePermanently)
        );
    }

    #[test]
    fn toolbar_actions_replace_repeated_rows_and_receive_keyboard_focus() {
        let mut menu = ActionMenuState::new(
            ActionMenuPlacement::Header,
            MenuContext {
                selected_count: 1,
                clipboard_valid: true,
                permissions_supported: true,
                ..MenuContext::default()
            },
            1,
        );
        assert_eq!(menu.focused_command(), Some(FileMenuCommand::Cut));
        menu.move_focus(1);
        assert_eq!(menu.focused_command(), Some(FileMenuCommand::Copy));
        assert!(menu.entries.iter().all(|entry| {
            !matches!(
                entry,
                MenuEntry::Action {
                    command: FileMenuCommand::Cut
                        | FileMenuCommand::Copy
                        | FileMenuCommand::Paste
                        | FileMenuCommand::Rename
                        | FileMenuCommand::Trash,
                    ..
                }
            )
        }));
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
