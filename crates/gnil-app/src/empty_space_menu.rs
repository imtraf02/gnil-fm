use gnil_core::{SortDirection, SortField, SortSpec};
use gpui::{Pixels, Point};

use crate::action_menu::MenuAnimationState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EmptySpaceMenuCommand {
    NewFolder,
    NewFile,
    Paste,
    Refresh,
    SortField(SortField),
    SortDirection(SortDirection),
    ToggleHidden,
    ToggleGitStatus,
    SelectAll,
    OpenTerminal,
    FolderProperties,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EmptySpaceSubmenu {
    New,
    SortBy,
    ViewOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EmptySpaceMenuEntry {
    Action {
        command: EmptySpaceMenuCommand,
        label: &'static str,
        shortcut: Option<&'static str>,
        enabled: bool,
        checked: bool,
    },
    Submenu {
        submenu: EmptySpaceSubmenu,
        label: &'static str,
        enabled: bool,
    },
    Separator,
}

impl EmptySpaceMenuEntry {
    pub(crate) fn enabled_command(&self) -> Option<EmptySpaceMenuCommand> {
        match self {
            Self::Action {
                command,
                enabled: true,
                ..
            } => Some(*command),
            Self::Action { .. } | Self::Submenu { .. } | Self::Separator => None,
        }
    }

    pub(crate) fn enabled_submenu(&self) -> Option<EmptySpaceSubmenu> {
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
pub(crate) struct EmptySpaceMenuCapabilities {
    pub(crate) clipboard_valid: bool,
    pub(crate) operation_running: bool,
    pub(crate) has_entries: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EmptySpaceViewState {
    pub(crate) show_hidden: bool,
    pub(crate) git_status_enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EmptySpaceMenuContext {
    pub(crate) capabilities: EmptySpaceMenuCapabilities,
    pub(crate) sort: SortSpec,
    pub(crate) view: EmptySpaceViewState,
}

#[derive(Clone, Debug)]
pub(crate) struct EmptySpaceMenuState {
    pub(crate) position: Point<Pixels>,
    pub(crate) root_entries: Vec<EmptySpaceMenuEntry>,
    pub(crate) submenu: Option<EmptySpaceSubmenu>,
    pub(crate) submenu_entries: Vec<EmptySpaceMenuEntry>,
    pub(crate) focused_root: Option<usize>,
    pub(crate) focused_submenu: Option<usize>,
    pub(crate) animation: MenuAnimationState,
    pub(crate) serial: u64,
    context: EmptySpaceMenuContext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EmptySpaceMenuActivation {
    Command(EmptySpaceMenuCommand),
    Submenu(EmptySpaceSubmenu),
}

impl EmptySpaceMenuState {
    pub(crate) fn new(
        position: Point<Pixels>,
        context: EmptySpaceMenuContext,
        serial: u64,
    ) -> Self {
        let writes_enabled = !context.capabilities.operation_running;
        let root_entries = vec![
            submenu(EmptySpaceSubmenu::New, "New", writes_enabled),
            action(
                EmptySpaceMenuCommand::Paste,
                "Paste",
                Some("Ctrl+V"),
                context.capabilities.clipboard_valid && writes_enabled,
                false,
            ),
            EmptySpaceMenuEntry::Separator,
            action(
                EmptySpaceMenuCommand::Refresh,
                "Refresh",
                Some("F5"),
                true,
                false,
            ),
            submenu(EmptySpaceSubmenu::SortBy, "Sort by", true),
            submenu(EmptySpaceSubmenu::ViewOptions, "View options", true),
            EmptySpaceMenuEntry::Separator,
            action(
                EmptySpaceMenuCommand::SelectAll,
                "Select All",
                Some("Ctrl+A"),
                context.capabilities.has_entries,
                false,
            ),
            action(
                EmptySpaceMenuCommand::OpenTerminal,
                "Open Terminal Here",
                None,
                true,
                false,
            ),
            EmptySpaceMenuEntry::Separator,
            action(
                EmptySpaceMenuCommand::FolderProperties,
                "Folder Properties",
                None,
                true,
                false,
            ),
        ];
        let focused_root = root_entries.iter().position(is_selectable);
        Self {
            position,
            root_entries,
            submenu: None,
            submenu_entries: Vec::new(),
            focused_root,
            focused_submenu: None,
            animation: MenuAnimationState::Opening,
            serial,
            context,
        }
    }

    pub(crate) fn open_submenu(&mut self, submenu: EmptySpaceSubmenu) {
        self.submenu = Some(submenu);
        self.submenu_entries = submenu_entries(submenu, self.context);
        self.focused_submenu = self.submenu_entries.iter().position(is_selectable);
    }

    pub(crate) fn close_submenu(&mut self) -> bool {
        let was_open = self.submenu.take().is_some();
        self.submenu_entries.clear();
        self.focused_submenu = None;
        was_open
    }

    pub(crate) fn move_focus(&mut self, direction: isize) {
        if self.submenu.is_some() {
            move_focus_in(&self.submenu_entries, &mut self.focused_submenu, direction);
        } else {
            move_focus_in(&self.root_entries, &mut self.focused_root, direction);
        }
    }

    pub(crate) fn focus_first(&mut self) {
        let (entries, focused) = self.active_entries_and_focus_mut();
        *focused = entries.iter().position(is_selectable);
    }

    pub(crate) fn focus_last(&mut self) {
        let (entries, focused) = self.active_entries_and_focus_mut();
        *focused = entries.iter().rposition(is_selectable);
    }

    pub(crate) fn focus_root(&mut self, index: usize) {
        if self.root_entries.get(index).is_some_and(is_selectable) {
            self.focused_root = Some(index);
        }
    }

    pub(crate) fn focus_submenu(&mut self, index: usize) {
        if self.submenu_entries.get(index).is_some_and(is_selectable) {
            self.focused_submenu = Some(index);
        }
    }

    pub(crate) fn focused_activation(&self) -> Option<EmptySpaceMenuActivation> {
        if self.submenu.is_some() {
            return self
                .focused_submenu
                .and_then(|index| self.submenu_entries.get(index))
                .and_then(EmptySpaceMenuEntry::enabled_command)
                .map(EmptySpaceMenuActivation::Command);
        }
        let entry = self
            .focused_root
            .and_then(|index| self.root_entries.get(index))?;
        entry
            .enabled_command()
            .map(EmptySpaceMenuActivation::Command)
            .or_else(|| {
                entry
                    .enabled_submenu()
                    .map(EmptySpaceMenuActivation::Submenu)
            })
    }

    fn active_entries_and_focus_mut(&mut self) -> (&[EmptySpaceMenuEntry], &mut Option<usize>) {
        if self.submenu.is_some() {
            (&self.submenu_entries, &mut self.focused_submenu)
        } else {
            (&self.root_entries, &mut self.focused_root)
        }
    }
}

fn submenu_entries(
    submenu: EmptySpaceSubmenu,
    context: EmptySpaceMenuContext,
) -> Vec<EmptySpaceMenuEntry> {
    match submenu {
        EmptySpaceSubmenu::New => vec![
            action(
                EmptySpaceMenuCommand::NewFolder,
                "New Folder",
                Some("Ctrl+Shift+N"),
                !context.capabilities.operation_running,
                false,
            ),
            action(
                EmptySpaceMenuCommand::NewFile,
                "New File",
                None,
                !context.capabilities.operation_running,
                false,
            ),
        ],
        EmptySpaceSubmenu::SortBy => vec![
            checked_action(
                EmptySpaceMenuCommand::SortField(SortField::Name),
                "Name",
                context.sort.field == SortField::Name,
            ),
            checked_action(
                EmptySpaceMenuCommand::SortField(SortField::Size),
                "Size",
                context.sort.field == SortField::Size,
            ),
            checked_action(
                EmptySpaceMenuCommand::SortField(SortField::Modified),
                "Modified",
                context.sort.field == SortField::Modified,
            ),
            checked_action(
                EmptySpaceMenuCommand::SortField(SortField::Kind),
                "Kind",
                context.sort.field == SortField::Kind,
            ),
            EmptySpaceMenuEntry::Separator,
            checked_action(
                EmptySpaceMenuCommand::SortDirection(SortDirection::Ascending),
                "Ascending",
                context.sort.direction == SortDirection::Ascending,
            ),
            checked_action(
                EmptySpaceMenuCommand::SortDirection(SortDirection::Descending),
                "Descending",
                context.sort.direction == SortDirection::Descending,
            ),
        ],
        EmptySpaceSubmenu::ViewOptions => vec![
            checked_action(
                EmptySpaceMenuCommand::ToggleHidden,
                "Show Hidden Files",
                context.view.show_hidden,
            ),
            checked_action(
                EmptySpaceMenuCommand::ToggleGitStatus,
                "Show Git Status",
                context.view.git_status_enabled,
            ),
        ],
    }
}

fn action(
    command: EmptySpaceMenuCommand,
    label: &'static str,
    shortcut: Option<&'static str>,
    enabled: bool,
    checked: bool,
) -> EmptySpaceMenuEntry {
    EmptySpaceMenuEntry::Action {
        command,
        label,
        shortcut,
        enabled,
        checked,
    }
}

fn checked_action(
    command: EmptySpaceMenuCommand,
    label: &'static str,
    checked: bool,
) -> EmptySpaceMenuEntry {
    action(command, label, None, true, checked)
}

fn submenu(submenu: EmptySpaceSubmenu, label: &'static str, enabled: bool) -> EmptySpaceMenuEntry {
    EmptySpaceMenuEntry::Submenu {
        submenu,
        label,
        enabled,
    }
}

fn is_selectable(entry: &EmptySpaceMenuEntry) -> bool {
    entry.enabled_command().is_some() || entry.enabled_submenu().is_some()
}

fn move_focus_in(entries: &[EmptySpaceMenuEntry], focused: &mut Option<usize>, direction: isize) {
    let selectable: Vec<_> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| is_selectable(entry).then_some(index))
        .collect();
    if selectable.is_empty() {
        *focused = None;
        return;
    }
    let current = focused.and_then(|focused| selectable.iter().position(|index| *index == focused));
    let next = match (current, direction.is_negative()) {
        (Some(index), false) => (index + 1) % selectable.len(),
        (Some(0) | None, true) => selectable.len() - 1,
        (Some(index), true) => index - 1,
        (None, false) => 0,
    };
    *focused = Some(selectable[next]);
}

#[cfg(test)]
mod tests {
    use gpui::{point, px};

    use super::*;

    fn context() -> EmptySpaceMenuContext {
        EmptySpaceMenuContext {
            capabilities: EmptySpaceMenuCapabilities {
                clipboard_valid: false,
                operation_running: false,
                has_entries: true,
            },
            sort: SortSpec::default(),
            view: EmptySpaceViewState {
                show_hidden: false,
                git_status_enabled: true,
            },
        }
    }

    fn menu(context: EmptySpaceMenuContext) -> EmptySpaceMenuState {
        EmptySpaceMenuState::new(point(px(10.0), px(20.0)), context, 1)
    }

    #[test]
    fn root_registry_is_folder_scoped_and_keeps_invalid_paste_visible() {
        let menu = menu(context());
        assert!(menu.root_entries.iter().any(|entry| matches!(
            entry,
            EmptySpaceMenuEntry::Action {
                command: EmptySpaceMenuCommand::Paste,
                enabled: false,
                ..
            }
        )));
        assert!(!menu.root_entries.iter().any(|entry| matches!(
            entry,
            EmptySpaceMenuEntry::Action {
                command: EmptySpaceMenuCommand::NewFile | EmptySpaceMenuCommand::NewFolder,
                ..
            }
        )));
    }

    #[test]
    fn submenus_reflect_sort_and_view_state() {
        let mut context = context();
        context.sort.field = SortField::Modified;
        context.sort.direction = SortDirection::Descending;
        context.view.show_hidden = true;
        context.view.git_status_enabled = false;
        let mut menu = menu(context);
        menu.open_submenu(EmptySpaceSubmenu::SortBy);
        assert!(menu.submenu_entries.iter().any(|entry| matches!(
            entry,
            EmptySpaceMenuEntry::Action {
                command: EmptySpaceMenuCommand::SortField(SortField::Modified),
                checked: true,
                ..
            }
        )));
        assert!(menu.submenu_entries.iter().any(|entry| matches!(
            entry,
            EmptySpaceMenuEntry::Action {
                command: EmptySpaceMenuCommand::SortDirection(SortDirection::Descending),
                checked: true,
                ..
            }
        )));
        menu.open_submenu(EmptySpaceSubmenu::ViewOptions);
        assert!(menu.submenu_entries.iter().any(|entry| matches!(
            entry,
            EmptySpaceMenuEntry::Action {
                command: EmptySpaceMenuCommand::ToggleHidden,
                checked: true,
                ..
            }
        )));
        assert!(menu.submenu_entries.iter().any(|entry| matches!(
            entry,
            EmptySpaceMenuEntry::Action {
                command: EmptySpaceMenuCommand::ToggleGitStatus,
                checked: false,
                ..
            }
        )));
    }

    #[test]
    fn keyboard_navigation_wraps_and_skips_disabled_entries() {
        let mut context = context();
        context.capabilities.operation_running = true;
        context.capabilities.has_entries = false;
        let mut menu = menu(context);
        assert_eq!(
            menu.focused_activation(),
            Some(EmptySpaceMenuActivation::Command(
                EmptySpaceMenuCommand::Refresh
            ))
        );
        menu.move_focus(-1);
        assert_eq!(
            menu.focused_activation(),
            Some(EmptySpaceMenuActivation::Command(
                EmptySpaceMenuCommand::FolderProperties
            ))
        );
        menu.focus_last();
        menu.move_focus(1);
        assert_eq!(
            menu.focused_activation(),
            Some(EmptySpaceMenuActivation::Command(
                EmptySpaceMenuCommand::Refresh
            ))
        );
    }
}
