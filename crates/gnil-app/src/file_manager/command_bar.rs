#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandBarMenuKind {
    New,
    Sort,
    Layout,
    More,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandBarCommand {
    Cut,
    Copy,
    Rename,
    Trash,
    NewFolder,
    NewFile,
    NewSymlink,
    Open,
    OpenWith,
    ToggleFavorite,
    Extract,
    ExtractTo,
    CopyPathAbsolute,
    CopyPathRelative,
    Properties,
    DeletePermanently,
    SortField(SortField),
    SortDirection(SortDirection),
    Layout(FileLayout),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CommandBarWidthTier {
    Narrow,
    Compact,
    #[default]
    Full,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CommandBarMenuItem {
    command: CommandBarCommand,
    label: &'static str,
    shortcut: Option<&'static str>,
    enabled: bool,
    checked: bool,
    danger: bool,
}

#[derive(Clone, Debug)]
struct CommandBarMenuState {
    kind: CommandBarMenuKind,
    focused: Option<usize>,
    animation: MenuAnimationState,
    serial: u64,
}

impl CommandBarMenuState {
    fn new(kind: CommandBarMenuKind, items: &[CommandBarMenuItem], serial: u64) -> Self {
        Self {
            kind,
            focused: items.iter().position(|item| item.enabled),
            animation: MenuAnimationState::Opening,
            serial,
        }
    }

    fn move_focus(&mut self, items: &[CommandBarMenuItem], direction: isize) {
        let enabled = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.enabled.then_some(index))
            .collect::<Vec<_>>();
        if enabled.is_empty() {
            self.focused = None;
            return;
        }
        let current = self
            .focused
            .and_then(|focused| enabled.iter().position(|index| *index == focused));
        let next = match (current, direction.is_negative()) {
            (Some(index), false) => (index + 1) % enabled.len(),
            (Some(0) | None, true) => enabled.len() - 1,
            (Some(index), true) => index - 1,
            (None, false) => 0,
        };
        self.focused = Some(enabled[next]);
    }

    fn focus_first(&mut self, items: &[CommandBarMenuItem]) {
        self.focused = items.iter().position(|item| item.enabled);
    }

    fn focus_last(&mut self, items: &[CommandBarMenuItem]) {
        self.focused = items.iter().rposition(|item| item.enabled);
    }
}

fn command_bar_item(
    command: CommandBarCommand,
    label: &'static str,
    shortcut: Option<&'static str>,
    enabled: bool,
) -> CommandBarMenuItem {
    CommandBarMenuItem {
        command,
        label,
        shortcut,
        enabled,
        checked: false,
        danger: false,
    }
}

fn command_bar_checked_item(
    command: CommandBarCommand,
    label: &'static str,
    checked: bool,
) -> CommandBarMenuItem {
    CommandBarMenuItem {
        command,
        label,
        shortcut: None,
        enabled: true,
        checked,
        danger: false,
    }
}

impl FileManager {
    fn command_bar_menu_items(&self, kind: CommandBarMenuKind) -> Vec<CommandBarMenuItem> {
        let has_selection = self.selection.selected_count() > 0;
        let writes_enabled = true;
        match kind {
            CommandBarMenuKind::New => vec![
                command_bar_item(
                    CommandBarCommand::NewFolder,
                    "Folder",
                    Some("Ctrl+Shift+N"),
                    writes_enabled && self.tab.root != TabRoot::Trash,
                ),
                command_bar_item(
                    CommandBarCommand::NewFile,
                    "File",
                    None,
                    writes_enabled && self.tab.root != TabRoot::Trash,
                ),
                command_bar_item(
                    CommandBarCommand::NewSymlink,
                    "Symlink",
                    Some("Ctrl+Shift+L"),
                    writes_enabled && self.tab.root != TabRoot::Trash,
                ),
            ],
            CommandBarMenuKind::Sort => vec![
                command_bar_checked_item(
                    CommandBarCommand::SortField(SortField::Name),
                    "Name",
                    self.tab.sort.field == SortField::Name,
                ),
                command_bar_checked_item(
                    CommandBarCommand::SortField(SortField::Modified),
                    if self.tab.root == TabRoot::Trash {
                        "Date deleted"
                    } else {
                        "Date modified"
                    },
                    self.tab.sort.field == SortField::Modified,
                ),
                command_bar_checked_item(
                    CommandBarCommand::SortField(SortField::Kind),
                    "Type",
                    self.tab.sort.field == SortField::Kind,
                ),
                command_bar_checked_item(
                    CommandBarCommand::SortField(SortField::Size),
                    "Size",
                    self.tab.sort.field == SortField::Size,
                ),
                command_bar_checked_item(
                    CommandBarCommand::SortDirection(SortDirection::Ascending),
                    "Ascending",
                    self.tab.sort.direction == SortDirection::Ascending,
                ),
                command_bar_checked_item(
                    CommandBarCommand::SortDirection(SortDirection::Descending),
                    "Descending",
                    self.tab.sort.direction == SortDirection::Descending,
                ),
            ],
            CommandBarMenuKind::Layout => vec![
                command_bar_checked_item(
                    CommandBarCommand::Layout(FileLayout::Details),
                    "Details",
                    self.settings.file_layout == FileLayout::Details,
                ),
                command_bar_checked_item(
                    CommandBarCommand::Layout(FileLayout::Grid),
                    "Grid",
                    self.settings.file_layout == FileLayout::Grid,
                ),
            ],
            CommandBarMenuKind::More => self.command_bar_more_items(has_selection, writes_enabled),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn command_bar_more_items(
        &self,
        has_selection: bool,
        writes_enabled: bool,
    ) -> Vec<CommandBarMenuItem> {
        let mut items = Vec::new();
        if self.tab.root == TabRoot::Trash {
            if has_selection {
                items.push(command_bar_item(
                    CommandBarCommand::CopyPathAbsolute,
                    "Copy original path",
                    Some("Ctrl+Shift+C"),
                    true,
                ));
            }
            return items;
        }

        if self.command_bar_width_tier == CommandBarWidthTier::Narrow {
            items.push(command_bar_item(
                CommandBarCommand::Cut,
                "Cut",
                Some("Ctrl+X"),
                has_selection,
            ));
            items.push(command_bar_item(
                CommandBarCommand::Copy,
                "Copy",
                Some("Ctrl+C"),
                has_selection,
            ));
        }
        if self.command_bar_width_tier != CommandBarWidthTier::Full {
            items.push(command_bar_item(
                CommandBarCommand::Rename,
                "Rename",
                Some("F2"),
                has_selection && writes_enabled,
            ));
            let mut trash = command_bar_item(
                CommandBarCommand::Trash,
                "Move to Trash",
                Some("Delete"),
                has_selection && writes_enabled,
            );
            trash.danger = true;
            items.push(trash);
        }

        let context = MenuContext::from_selection(
            &self.selection,
            &self.snapshot.entries,
            &self.settings.favorites,
            false,
            false,
        );
        if context.selected_count == 1 {
            items.push(command_bar_item(
                CommandBarCommand::Open,
                "Open",
                Some("Enter"),
                true,
            ));
        }
        if context.open_with_eligible {
            items.push(command_bar_item(
                CommandBarCommand::OpenWith,
                "Open with…",
                None,
                true,
            ));
        }
        if context.favorite_eligible {
            items.push(command_bar_item(
                CommandBarCommand::ToggleFavorite,
                if context.selected_is_favorite {
                    "Remove from Favorites"
                } else {
                    "Add to Favorites"
                },
                Some("Ctrl+D"),
                true,
            ));
        }
        if context.all_selected_archives {
            items.push(command_bar_item(
                CommandBarCommand::Extract,
                "Extract",
                Some("Ctrl+E"),
                writes_enabled,
            ));
            items.push(command_bar_item(
                CommandBarCommand::ExtractTo,
                "Extract to…",
                Some("Ctrl+Shift+E"),
                writes_enabled,
            ));
        }
        if has_selection {
            items.push(command_bar_item(
                CommandBarCommand::CopyPathAbsolute,
                "Copy absolute path",
                Some("Ctrl+Shift+C"),
                true,
            ));
            items.push(command_bar_item(
                CommandBarCommand::CopyPathRelative,
                "Copy relative path",
                Some("Ctrl+Alt+C"),
                true,
            ));
        }
        items.push(command_bar_item(
            CommandBarCommand::Properties,
            if has_selection {
                "Properties"
            } else {
                "Folder Properties"
            },
            Some("Alt+Enter"),
            if has_selection {
                context.permissions_supported && writes_enabled
            } else {
                true
            },
        ));
        if has_selection {
            let mut delete = command_bar_item(
                CommandBarCommand::DeletePermanently,
                "Delete Permanently",
                Some("Shift+Delete"),
                writes_enabled,
            );
            delete.danger = true;
            items.push(delete);
        }
        items
    }
}

#[cfg(test)]
mod command_bar_tests {
    use super::*;

    fn items() -> Vec<CommandBarMenuItem> {
        vec![
            command_bar_item(
                CommandBarCommand::NewFolder,
                "Folder",
                None,
                false,
            ),
            command_bar_item(
                CommandBarCommand::NewFile,
                "File",
                None,
                true,
            ),
            command_bar_item(
                CommandBarCommand::NewSymlink,
                "Symlink",
                None,
                true,
            ),
        ]
    }

    #[test]
    fn keyboard_focus_skips_disabled_items_and_wraps() {
        let items = items();
        let mut state = CommandBarMenuState::new(CommandBarMenuKind::New, &items, 1);
        assert_eq!(state.focused, Some(1));
        state.move_focus(&items, 1);
        assert_eq!(state.focused, Some(2));
        state.move_focus(&items, 1);
        assert_eq!(state.focused, Some(1));
        state.focus_last(&items);
        assert_eq!(state.focused, Some(2));
    }
}
