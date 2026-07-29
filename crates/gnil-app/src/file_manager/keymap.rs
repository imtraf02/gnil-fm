use gnil_core::{KeymapBinding, KeymapBindingKind};

struct KeymapEditState {
    action: String,
    replacing: Option<String>,
    input: Entity<TextInput>,
    capturing: bool,
}

struct KeymapConflictState {
    edit: KeymapEditState,
    collisions: Vec<KeymapCollision>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct KeymapCollision {
    action: String,
    keystrokes: String,
}

#[derive(Clone, Copy)]
struct KeymapActionDescriptor {
    id: ActionId,
    label: &'static str,
    description: &'static str,
    category: &'static str,
    default_keys: &'static [&'static str],
}

const KEYMAP_ACTIONS: &[KeymapActionDescriptor] = &[
    KeymapActionDescriptor { id: ActionId("selection.next"), label: "Select Next", description: "Move the file cursor down", category: "Navigation", default_keys: &["down"] },
    KeymapActionDescriptor { id: ActionId("selection.previous"), label: "Select Previous", description: "Move the file cursor up", category: "Navigation", default_keys: &["up"] },
    KeymapActionDescriptor { id: ActionId("selection.left"), label: "Select Left", description: "Move left in Grid layout", category: "Navigation", default_keys: &["left"] },
    KeymapActionDescriptor { id: ActionId("selection.right"), label: "Select Right", description: "Move right in Grid layout", category: "Navigation", default_keys: &["right"] },
    KeymapActionDescriptor { id: ActionId("selection.next_range"), label: "Extend Selection Down", description: "Extend the selection to the next item", category: "Selection", default_keys: &["shift-down"] },
    KeymapActionDescriptor { id: ActionId("selection.previous_range"), label: "Extend Selection Up", description: "Extend the selection to the previous item", category: "Selection", default_keys: &["shift-up"] },
    KeymapActionDescriptor { id: ActionId("selection.left_range"), label: "Extend Selection Left", description: "Extend selection left in Grid layout", category: "Selection", default_keys: &["shift-left"] },
    KeymapActionDescriptor { id: ActionId("selection.right_range"), label: "Extend Selection Right", description: "Extend selection right in Grid layout", category: "Selection", default_keys: &["shift-right"] },
    KeymapActionDescriptor { id: ActionId("selection.toggle"), label: "Toggle Selection", description: "Add or remove the cursor item", category: "Selection", default_keys: &["ctrl-space"] },
    KeymapActionDescriptor { id: ActionId("selection.all"), label: "Select All", description: "Select every visible item", category: "Selection", default_keys: &["ctrl-a"] },
    KeymapActionDescriptor { id: ActionId("file.open"), label: "Open", description: "Open the selected item", category: "Navigation", default_keys: &["enter"] },
    KeymapActionDescriptor { id: ActionId("file.open_with"), label: "Open With", description: "Choose an application for the selected file", category: "File Operations", default_keys: &[] },
    KeymapActionDescriptor { id: ActionId("navigation.back"), label: "Back", description: "Go to the previous folder", category: "Navigation", default_keys: &["alt-left"] },
    KeymapActionDescriptor { id: ActionId("navigation.forward"), label: "Forward", description: "Go to the next folder", category: "Navigation", default_keys: &["alt-right"] },
    KeymapActionDescriptor { id: ActionId("navigation.up"), label: "Parent Folder", description: "Go to the parent folder", category: "Navigation", default_keys: &["alt-up"] },
    KeymapActionDescriptor { id: ActionId("view.refresh"), label: "Refresh", description: "Reload the current folder", category: "View & Search", default_keys: &["f5"] },
    KeymapActionDescriptor { id: ActionId("view.toggle_hidden"), label: "Toggle Hidden Files", description: "Show or hide hidden entries", category: "View & Search", default_keys: &["ctrl-h"] },
    KeymapActionDescriptor { id: ActionId("preview.toggle"), label: "Toggle Preview", description: "Show or hide the preview panel", category: "View & Search", default_keys: &["space"] },
    KeymapActionDescriptor { id: ActionId("search.path"), label: "Edit Path", description: "Focus the location field", category: "View & Search", default_keys: &["ctrl-l"] },
    KeymapActionDescriptor { id: ActionId("search.files"), label: "Search Files", description: "Search the current folder", category: "View & Search", default_keys: &["ctrl-f"] },
    KeymapActionDescriptor { id: ActionId("file.copy"), label: "Copy", description: "Copy the selected items", category: "File Operations", default_keys: &["ctrl-c"] },
    KeymapActionDescriptor { id: ActionId("file.cut"), label: "Cut", description: "Cut the selected items", category: "File Operations", default_keys: &["ctrl-x"] },
    KeymapActionDescriptor { id: ActionId("file.paste"), label: "Paste", description: "Paste into the current folder", category: "File Operations", default_keys: &["ctrl-v"] },
    KeymapActionDescriptor { id: ActionId("file.copy_path"), label: "Copy Absolute Path", description: "Copy selected paths", category: "File Operations", default_keys: &["ctrl-shift-c"] },
    KeymapActionDescriptor { id: ActionId("file.copy_relative_path"), label: "Copy Relative Path", description: "Copy paths relative to the current folder", category: "File Operations", default_keys: &["ctrl-alt-c"] },
    KeymapActionDescriptor { id: ActionId("file.rename"), label: "Rename", description: "Rename the selected item", category: "File Operations", default_keys: &["f2"] },
    KeymapActionDescriptor { id: ActionId("file.create_folder"), label: "Create Folder", description: "Create a folder in the current directory", category: "File Operations", default_keys: &["ctrl-shift-n"] },
    KeymapActionDescriptor { id: ActionId("file.create_file"), label: "Create File", description: "Create a file in the current directory", category: "File Operations", default_keys: &[] },
    KeymapActionDescriptor { id: ActionId("file.create_symlink"), label: "Create Symlink", description: "Create a symbolic link", category: "File Operations", default_keys: &["ctrl-shift-l"] },
    KeymapActionDescriptor { id: ActionId("file.set_permissions"), label: "Edit Permissions", description: "Edit POSIX permissions", category: "File Operations", default_keys: &["alt-enter"] },
    KeymapActionDescriptor { id: ActionId("file.extract"), label: "Extract Archive", description: "Extract beside the archive", category: "File Operations", default_keys: &["ctrl-e"] },
    KeymapActionDescriptor { id: ActionId("file.extract_to"), label: "Extract Archive To…", description: "Choose an extraction destination", category: "File Operations", default_keys: &["ctrl-shift-e"] },
    KeymapActionDescriptor { id: ActionId("file.trash"), label: "Move to Trash", description: "Move selected items to Trash", category: "Trash & Safety", default_keys: &["delete"] },
    KeymapActionDescriptor { id: ActionId("file.delete_permanently"), label: "Delete Permanently", description: "Delete after confirmation", category: "Trash & Safety", default_keys: &["shift-delete"] },
    KeymapActionDescriptor { id: ActionId("file.undo"), label: "Undo", description: "Undo the latest reversible operation", category: "Trash & Safety", default_keys: &["ctrl-z"] },
    KeymapActionDescriptor { id: ActionId("favorites.toggle"), label: "Toggle Favorite", description: "Add or remove the selected folder from Favorites", category: "Favorites & Settings", default_keys: &["ctrl-d"] },
    KeymapActionDescriptor { id: ActionId("appearance.toggle"), label: "Open Appearance", description: "Open the theme and appearance menu", category: "Favorites & Settings", default_keys: &["ctrl-shift-t"] },
    KeymapActionDescriptor { id: ActionId("settings.open"), label: "Open Settings", description: "Open Settings", category: "Favorites & Settings", default_keys: &["ctrl-,"] },
    KeymapActionDescriptor { id: ActionId("settings.open_keymap"), label: "Open Keymap Editor", description: "Open Keymap & Controls in Settings", category: "Favorites & Settings", default_keys: &["ctrl-k ctrl-s"] },
];

impl PreferencesWindow {
    fn create_keymap_search(cx: &mut Context<Self>) -> (Entity<TextInput>, Subscription) {
        let input = cx.new(|cx| TextInput::new("Search commands", "", cx));
        let subscription = cx.subscribe(&input, |this, _, event: &TextInputEvent, cx| {
            if matches!(event, TextInputEvent::Changed) && this.settings_category == SettingsCategory::Keymap {
                cx.notify();
            }
        });
        (input, subscription)
    }

    fn install_keymap_capture(cx: &mut Context<Self>) -> Subscription {
        let manager = cx.weak_entity();
        cx.intercept_keystrokes(move |event, _, cx| {
            let _ = manager.update(cx, |manager, cx| {
                manager.capture_keymap_keystroke(&event.keystroke.unparse(), cx);
            });
        })
    }

    fn capture_keymap_keystroke(&mut self, keystroke: &str, cx: &mut Context<Self>) {
        let Some(edit) = self.keymap_edit.as_mut().filter(|edit| edit.capturing) else {
            return;
        };
        if keystroke == "escape" {
            self.keymap_edit = None;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let current = edit.input.read(cx).text().trim().to_owned();
        let value = if current.is_empty() {
            keystroke.to_owned()
        } else {
            format!("{current} {keystroke}")
        };
        edit.input.update(cx, |input, cx| input.set_text(value, cx));
        cx.stop_propagation();
        cx.notify();
    }

    fn begin_keymap_edit(
        &mut self,
        action: &str,
        replacing: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let initial = replacing.clone().unwrap_or_default();
        let input = cx.new(|cx| TextInput::new("ctrl-k ctrl-s", initial, cx));
        self.keymap_edit = Some(KeymapEditState {
            action: action.to_owned(),
            replacing,
            input,
            capturing: false,
        });
        cx.notify();
    }

    fn toggle_keymap_capture(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = self.keymap_edit.as_mut() {
            edit.capturing = !edit.capturing;
            cx.notify();
        }
    }

    fn cancel_keymap_edit(&mut self, cx: &mut Context<Self>) {
        self.keymap_edit = None;
        self.keymap_conflict = None;
        cx.notify();
    }

    fn commit_keymap_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.keymap_edit.take() else {
            return;
        };
        let keystrokes = edit.input.read(cx).text().trim().to_ascii_lowercase();
        if keystrokes.is_empty() || !keystrokes.split_whitespace().all(|key| gpui::Keystroke::parse(key).is_ok()) {
            self.keymap_error = Some("Enter valid GPUI keystrokes, for example ctrl-k ctrl-s.".into());
            self.keymap_edit = Some(edit);
            cx.notify();
            return;
        }
        let collisions = self.keymap_collisions(&edit.action, &keystrokes);
        if collisions.is_empty() {
            self.apply_keymap_edit(edit, keystrokes, cx);
        } else {
            self.keymap_conflict = Some(KeymapConflictState { edit, collisions });
            cx.notify();
        }
    }

    fn keymap_collisions(&self, action: &str, keystrokes: &str) -> Vec<KeymapCollision> {
        let mut collisions = Vec::new();
        for (other_action, other_keys) in self.effective_keymap_bindings() {
            if other_action != action
                && (other_keys == keystrokes
                    || other_keys.starts_with(&format!("{keystrokes} "))
                    || keystrokes.starts_with(&format!("{other_keys} ")))
                && !collisions.iter().any(|collision: &KeymapCollision| {
                    collision.action == other_action && collision.keystrokes == other_keys
                })
            {
                collisions.push(KeymapCollision {
                    action: other_action,
                    keystrokes: other_keys,
                });
            }
        }
        collisions
    }

    fn apply_keymap_edit(&mut self, edit: KeymapEditState, keystrokes: String, cx: &mut Context<Self>) {
        if let Some(previous) = edit.replacing {
            let custom = self.keymap_overrides.bindings.iter().any(|binding| {
                binding.action == edit.action
                    && binding.keystrokes == previous
                    && binding.kind == KeymapBindingKind::Bind
            });
            self.unbind_keymap_binding(&edit.action, &previous, custom, cx);
        }
        self.keymap_overrides.bindings.retain(|binding| {
            !(binding.action == edit.action
                && binding.keystrokes == keystrokes
                && binding.kind == KeymapBindingKind::Bind)
        });
        self.keymap_overrides.bindings.push(KeymapBinding {
            action: edit.action,
            keystrokes,
            kind: KeymapBindingKind::Bind,
        });
        self.save_keymap_overrides(cx);
    }

    fn confirm_keymap_reassign(&mut self, cx: &mut Context<Self>) {
        let Some(conflict) = self.keymap_conflict.take() else {
            return;
        };
        for collision in &conflict.collisions {
            let is_custom = self.keymap_overrides.bindings.iter().any(|binding| {
                binding.action == collision.action
                    && binding.keystrokes == collision.keystrokes
                    && binding.kind == KeymapBindingKind::Bind
            });
            self.unbind_keymap_binding(
                &collision.action,
                &collision.keystrokes,
                is_custom,
                cx,
            );
        }
        let keystrokes = conflict.edit.input.read(cx).text().trim().to_ascii_lowercase();
        self.apply_keymap_edit(conflict.edit, keystrokes, cx);
    }

    fn keymap_descriptor(action: &str) -> Option<&'static KeymapActionDescriptor> {
        KEYMAP_ACTIONS.iter().find(|descriptor| descriptor.id.0 == action)
    }

    fn keymap_binding_is_valid(binding: &KeymapBinding) -> bool {
        Self::keymap_descriptor(&binding.action).is_some()
            && !binding.keystrokes.is_empty()
            && binding
                .keystrokes
                .split_whitespace()
                .all(|key| gpui::Keystroke::parse(key).is_ok())
    }

    fn keymap_validation_message(keymap: &KeymapOverrides) -> Option<String> {
        let invalid = keymap
            .bindings
            .iter()
            .filter(|binding| !Self::keymap_binding_is_valid(binding))
            .count();
        (invalid > 0).then(|| {
            format!(
                "Ignored {invalid} invalid or unsupported keymap binding{}.",
                if invalid == 1 { "" } else { "s" }
            )
        })
    }

    fn default_keymap_entries() -> impl Iterator<Item = (&'static str, &'static str)> {
        KEYMAP_ACTIONS.iter().flat_map(|descriptor| {
            descriptor
                .default_keys
                .iter()
                .map(move |key| (descriptor.id.0, *key))
        })
    }

    fn binding_for_keymap_action(action: &str, keystrokes: &str, context: &'static str) -> Option<KeyBinding> {
        let binding = match action {
            "selection.next" => KeyBinding::new(keystrokes, SelectNext, Some(context)),
            "selection.previous" => KeyBinding::new(keystrokes, SelectPrevious, Some(context)),
            "selection.left" => KeyBinding::new(keystrokes, SelectLeft, Some(context)),
            "selection.right" => KeyBinding::new(keystrokes, SelectRight, Some(context)),
            "selection.next_range" => KeyBinding::new(keystrokes, SelectNextRange, Some(context)),
            "selection.previous_range" => KeyBinding::new(keystrokes, SelectPreviousRange, Some(context)),
            "selection.left_range" => KeyBinding::new(keystrokes, SelectLeftRange, Some(context)),
            "selection.right_range" => KeyBinding::new(keystrokes, SelectRightRange, Some(context)),
            "selection.toggle" => KeyBinding::new(keystrokes, ToggleSelection, Some(context)),
            "selection.all" => KeyBinding::new(keystrokes, SelectAllEntries, Some(context)),
            "file.open" => KeyBinding::new(keystrokes, OpenSelected, Some(context)),
            "file.open_with" => KeyBinding::new(keystrokes, OpenWithSelected, Some(context)),
            "navigation.back" => KeyBinding::new(keystrokes, GoBack, Some(context)),
            "navigation.forward" => KeyBinding::new(keystrokes, GoForward, Some(context)),
            "navigation.up" => KeyBinding::new(keystrokes, GoUp, Some(context)),
            "view.refresh" => KeyBinding::new(keystrokes, Refresh, Some(context)),
            "view.toggle_hidden" => KeyBinding::new(keystrokes, ToggleHidden, Some(context)),
            "preview.toggle" => KeyBinding::new(keystrokes, TogglePreview, Some(context)),
            "search.path" => KeyBinding::new(keystrokes, ActivatePathInput, Some(context)),
            "search.files" => KeyBinding::new(keystrokes, ActivateFileSearch, Some(context)),
            "file.copy" => KeyBinding::new(keystrokes, CopySelected, Some(context)),
            "file.cut" => KeyBinding::new(keystrokes, CutSelected, Some(context)),
            "file.paste" => KeyBinding::new(keystrokes, Paste, Some(context)),
            "file.copy_path" => KeyBinding::new(keystrokes, CopyPathAbsolute, Some(context)),
            "file.copy_relative_path" => KeyBinding::new(keystrokes, CopyPathRelative, Some(context)),
            "file.rename" => KeyBinding::new(keystrokes, OpenRename, Some(context)),
            "file.create_folder" => KeyBinding::new(keystrokes, CreateFolder, Some(context)),
            "file.create_file" => KeyBinding::new(keystrokes, CreateFile, Some(context)),
            "file.create_symlink" => KeyBinding::new(keystrokes, OpenCreateSymlink, Some(context)),
            "file.set_permissions" => KeyBinding::new(keystrokes, OpenPermissions, Some(context)),
            "file.extract" => KeyBinding::new(keystrokes, ExtractSelected, Some(context)),
            "file.extract_to" => KeyBinding::new(keystrokes, ExtractSelectedTo, Some(context)),
            "file.trash" => KeyBinding::new(keystrokes, TrashSelected, Some(context)),
            "file.delete_permanently" => KeyBinding::new(keystrokes, DeleteSelected, Some(context)),
            "file.undo" => KeyBinding::new(keystrokes, Undo, Some(context)),
            "favorites.toggle" => KeyBinding::new(keystrokes, ToggleFavorite, Some(context)),
            "appearance.toggle" => KeyBinding::new(keystrokes, ToggleAppearance, Some(context)),
            "settings.open" => KeyBinding::new(keystrokes, ToggleSettings, Some(context)),
            "settings.open_keymap" => KeyBinding::new(keystrokes, OpenKeymap, Some(context)),
            _ => return None,
        };
        Some(binding)
    }

    fn install_keybindings(keymap: &KeymapOverrides, cx: &mut App) {
        cx.clear_key_bindings();
        text_input::bind_keys(cx);
        cx.bind_keys([
            KeyBinding::new("down", MenuNext, Some("ActionMenu")),
            KeyBinding::new("j", MenuNext, Some("ActionMenu")),
            KeyBinding::new("up", MenuPrevious, Some("ActionMenu")),
            KeyBinding::new("k", MenuPrevious, Some("ActionMenu")),
            KeyBinding::new("home", MenuFirst, Some("ActionMenu")),
            KeyBinding::new("end", MenuLast, Some("ActionMenu")),
            KeyBinding::new("enter", MenuActivate, Some("ActionMenu")),
            KeyBinding::new("space", MenuActivate, Some("ActionMenu")),
            KeyBinding::new("right", MenuOpenSubmenu, Some("ActionMenu")),
            KeyBinding::new("left", MenuCloseSubmenu, Some("ActionMenu")),
            KeyBinding::new("enter", ConfirmOpenWith, Some("OpenWithChooser")),
            KeyBinding::new("down", OpenWithNext, Some("OpenWithChooser")),
            KeyBinding::new("up", OpenWithPrevious, Some("OpenWithChooser")),
            KeyBinding::new("escape", DismissOpenWith, Some("OpenWithChooser")),
            KeyBinding::new("enter", ConfirmOpenWith, Some("OpenWithInput")),
            KeyBinding::new("down", OpenWithNext, Some("OpenWithInput")),
            KeyBinding::new("up", OpenWithPrevious, Some("OpenWithInput")),
            KeyBinding::new("escape", DismissOpenWith, Some("OpenWithInput")),
            KeyBinding::new("escape", CancelPointerInteraction, Some("PointerInteraction")),
            KeyBinding::new(
                "enter",
                ConfirmInlineRename,
                Some("InlineRenameInput"),
            ),
            KeyBinding::new(
                "escape",
                CancelInlineRename,
                Some("InlineRenameInput"),
            ),
            KeyBinding::new("enter", SubmitPathInput, Some("PathInput")),
            KeyBinding::new("escape", DismissPathInput, Some("PathInput")),
            KeyBinding::new("tab", CompletePathNext, Some("PathInput")),
            KeyBinding::new("shift-tab", CompletePathPrevious, Some("PathInput")),
            KeyBinding::new("up", PathHistoryPrevious, Some("PathInput")),
            KeyBinding::new("down", PathHistoryNext, Some("PathInput")),
            KeyBinding::new("ctrl-v", PastePath, Some("PathInput")),
            KeyBinding::new("ctrl-l", ActivatePathInput, Some("PathInput")),
            KeyBinding::new("enter", OpenFileSearchResult, Some("SearchInput")),
            KeyBinding::new("down", FileSearchNext, Some("SearchInput")),
            KeyBinding::new("up", FileSearchPrevious, Some("SearchInput")),
            KeyBinding::new("ctrl-f", ActivateFileSearch, Some("SearchInput")),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("escape", HandleEscape, None),
            KeyBinding::new("escape", DismissSettings, Some("PreferencesWindow")),
            KeyBinding::new("ctrl-f", FocusSettingsSearch, Some("PreferencesWindow")),
            KeyBinding::new("ctrl-enter", ApplySheet, None),
        ]);

        let unbound: HashSet<_> = keymap
            .bindings
            .iter()
            .filter(|binding| binding.kind == KeymapBindingKind::Unbind)
            .map(|binding| (binding.action.as_str(), binding.keystrokes.as_str()))
            .collect();
        let defaults = Self::default_keymap_entries().filter_map(|(action, key)| {
            (!unbound.contains(&(action, key)))
                .then(|| Self::binding_for_keymap_action(action, key, "FileManager"))
                .flatten()
        });
        cx.bind_keys(defaults);

        for binding in keymap
            .bindings
            .iter()
            .filter(|binding| binding.kind == KeymapBindingKind::Bind)
            .filter(|binding| Self::keymap_binding_is_valid(binding))
        {
            if let Some(binding) = Self::binding_for_keymap_action(
                &binding.action,
                &binding.keystrokes,
                "FileManager",
            ) {
                cx.bind_keys([binding]);
            }
        }
    }

    fn effective_keymap_bindings(&self) -> Vec<(String, String)> {
        let unbound: HashSet<_> = self
            .keymap_overrides
            .bindings
            .iter()
            .filter(|binding| binding.kind == KeymapBindingKind::Unbind)
            .map(|binding| (binding.action.as_str(), binding.keystrokes.as_str()))
            .collect();
        let mut entries: Vec<_> = Self::default_keymap_entries()
            .filter(|entry| !unbound.contains(entry))
            .map(|(action, keys)| (action.to_owned(), keys.to_owned()))
            .collect();
        entries.extend(
            self.keymap_overrides
                .bindings
                .iter()
                .filter(|binding| binding.kind == KeymapBindingKind::Bind)
                .filter(|binding| Self::keymap_binding_is_valid(binding))
                .map(|binding| (binding.action.clone(), binding.keystrokes.clone())),
        );
        entries
    }

    fn save_keymap_overrides(&mut self, cx: &mut Context<Self>) {
        self.keymap_overrides.normalize();
        let keymap = self.keymap_overrides.clone();
        let result = self.preferences.update(cx, |preferences, cx| {
            let result = preferences.save_keymap(keymap);
            cx.notify();
            result
        });
        match result {
            Ok(()) => self.keymap_error = Self::keymap_validation_message(&self.keymap_overrides),
            Err(error) => self.keymap_error = Some(format!("Could not save keymap: {error}")),
        }
        Self::install_keybindings(&self.keymap_overrides, cx);
        cx.notify();
    }

    fn reload_keymap_overrides(&mut self, cx: &mut Context<Self>) {
        match self.config_paths.load_keymap() {
            Ok(keymap) => {
                let error = Self::keymap_validation_message(&keymap);
                self.keymap_overrides = keymap.clone();
                self.keymap_error = error;
                self.preferences.update(cx, |preferences, cx| {
                    preferences.keymap = keymap;
                    preferences.keymap_error = Self::keymap_validation_message(&preferences.keymap);
                    cx.notify();
                });
                Self::install_keybindings(&self.keymap_overrides, cx);
            }
            Err(error) => self.keymap_error = Some(format!("Could not reload keymap: {error}")),
        }
        cx.notify();
    }

    fn reset_all_keymap_overrides(&mut self, cx: &mut Context<Self>) {
        self.keymap_overrides = KeymapOverrides::default();
        self.save_keymap_overrides(cx);
    }

    fn reset_keymap_action(&mut self, action: &str, cx: &mut Context<Self>) {
        self.keymap_overrides.bindings.retain(|binding| binding.action != action);
        self.save_keymap_overrides(cx);
    }

    fn unbind_keymap_binding(&mut self, action: &str, keystrokes: &str, custom: bool, cx: &mut Context<Self>) {
        if custom {
            self.keymap_overrides.bindings.retain(|binding| {
                !(binding.action == action
                    && binding.keystrokes == keystrokes
                    && binding.kind == KeymapBindingKind::Bind)
            });
        } else if !self.keymap_overrides.bindings.iter().any(|binding| {
            binding.action == action
                && binding.keystrokes == keystrokes
                && binding.kind == KeymapBindingKind::Unbind
        }) {
            self.keymap_overrides.bindings.push(KeymapBinding {
                action: action.to_owned(),
                keystrokes: keystrokes.to_owned(),
                kind: KeymapBindingKind::Unbind,
            });
        }
        self.save_keymap_overrides(cx);
    }

    #[allow(clippy::too_many_lines)]
    fn render_keymap_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.keymap_search.read(cx).text().trim().to_ascii_lowercase();
        let effective = self.effective_keymap_bindings();
        let descriptors = KEYMAP_ACTIONS
            .iter()
            .copied()
            .filter(|descriptor| {
                query.is_empty()
                    || [
                        descriptor.label,
                        descriptor.description,
                        descriptor.category,
                        descriptor.id.0,
                    ]
                    .iter()
                    .any(|value| value.to_ascii_lowercase().contains(&query))
                    || effective.iter().any(|(action, keys)| {
                        action == descriptor.id.0 && keys.to_ascii_lowercase().contains(&query)
                    })
            })
            .collect::<Vec<_>>();
        let mut rows = Vec::new();
        let mut category = "";
        for descriptor in descriptors {
            if category != descriptor.category {
                category = descriptor.category;
                rows.push(
                    div()
                        .pt_2()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(text_muted()))
                        .child(category)
                        .into_any_element(),
                );
            }
            rows.push(self.render_keymap_action_row(descriptor, cx));
        }

        let error = self.keymap_error.clone();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(setting_section_header(
                "Keymap & Controls",
                "Customize the default file-manager commands. Changes apply immediately.",
            ))
            .when_some(error, |view, error| {
                view.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(rgb(error_color()))
                        .text_xs()
                        .text_color(rgb(text_emphasized()))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.keymap_search.clone())
                    .child(
                        div()
                            .id("reload-keymap")
                            .h_7()
                            .px_3()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(border()))
                            .text_xs()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .with_focus_ring()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.reload_keymap_overrides(cx);
                            }))
                            .child("Reload"),
                    )
                    .child(
                        div()
                            .id("reset-all-keymap")
                            .h_7()
                            .px_3()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(border()))
                            .text_xs()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .with_focus_ring()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.reset_all_keymap_overrides(cx);
                            }))
                            .child("Reset all"),
                    ),
            )
            .child(div().flex().flex_col().gap_2().children(rows))
            .into_any_element()
    }

    #[allow(clippy::too_many_lines)]
    fn render_keymap_action_row(
        &mut self,
        descriptor: KeymapActionDescriptor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let action = descriptor.id.0;
        let bindings = self
            .effective_keymap_bindings()
            .into_iter()
            .filter(|(binding_action, _)| binding_action == action)
            .collect::<Vec<_>>();
        let binding_chips = bindings.into_iter().map(|(_, keystrokes)| {
            let custom = self.keymap_overrides.bindings.iter().any(|binding| {
                binding.action == action
                    && binding.keystrokes == keystrokes
                    && binding.kind == KeymapBindingKind::Bind
            });
            let edit_action = action.to_owned();
            let edit_keys = keystrokes.clone();
            let remove_action = action.to_owned();
            let remove_keys = keystrokes.clone();
            let binding_id = keystrokes.clone();
            div()
                .id(SharedString::from(format!("keymap-binding-{action}-{keystrokes}")))
                .h_6()
                .rounded_md()
                .border_1()
                .border_color(rgb(border()))
                .flex()
                .items_center()
                .overflow_hidden()
                .child(
                    div()
                        .id(SharedString::from(format!("edit-keymap-binding-{action}-{binding_id}")))
                        .h_full()
                        .px_2()
                        .bg(rgb(if custom { accent_background() } else { surface() }))
                        .text_xs()
                        .text_color(rgb(if custom { text_emphasized() } else { text_muted() }))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(border())))
                        .with_focus_ring()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.begin_keymap_edit(&edit_action, Some(edit_keys.clone()), cx);
                        }))
                        .child(keystrokes),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("remove-keymap-binding-{action}-{binding_id}")))
                        .size_6()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(rgb(text_muted()))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(border())))
                        .with_focus_ring()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.unbind_keymap_binding(&remove_action, &remove_keys, custom, cx);
                        }))
                        .child("×"),
                )
        });
        let add_action = action.to_owned();
        let reset_action = action.to_owned();
        div()
            .id(SharedString::from(format!("keymap-action-{action}")))
            .min_h(px(56.0))
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(rgb(border()))
            .bg(rgb(surface_elevated()))
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .truncate()
                            .child(descriptor.label),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .truncate()
                            .child(descriptor.description),
                    ),
            )
            .child(div().flex().flex_wrap().justify_end().gap_1().children(binding_chips))
            .child(
                div()
                    .id(SharedString::from(format!("add-keymap-binding-{action}")))
                    .size_6()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_1()
                    .border_color(rgb(border()))
                    .text_xs()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(border())))
                    .with_focus_ring()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.begin_keymap_edit(&add_action, None, cx);
                    }))
                    .child("+"),
            )
            .child(
                div()
                    .id(SharedString::from(format!("reset-keymap-action-{action}")))
                    .size_6()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(border())))
                    .with_focus_ring()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.reset_keymap_action(&reset_action, cx);
                    }))
                    .child("↺"),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_lines)]
    fn render_keymap_edit_sheet(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(edit) = self.keymap_edit.as_ref() else {
            return div().into_any_element();
        };
        let action = Self::keymap_descriptor(&edit.action)
            .map_or(edit.action.as_str(), |descriptor| descriptor.label);
        let capturing = edit.capturing;
        let input = edit.input.clone();
        div()
            .id("keymap-edit-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .bg(Hsla::from(rgb(background())).opacity(0.65))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.cancel_keymap_edit(cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .w(px(420.0))
                    .max_w(px(520.0))
                    .p_5()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(border()))
                    .bg(rgb(surface()))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(format!("Set keybinding for {action}")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child("Record any number of keystrokes, then save. Escape cancels recording; type escape manually when needed."),
                    )
                    .child(input)
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .id("toggle-keymap-capture")
                                    .h_7()
                                    .px_3()
                                    .rounded_md()
                                    .bg(rgb(if capturing { accent_background() } else { surface_elevated() }))
                                    .border_1()
                                    .border_color(rgb(border()))
                                    .text_xs()
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(border())))
                                    .with_focus_ring()
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_keymap_capture(cx)))
                                    .child(if capturing { "Stop recording" } else { "Record keys" }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("cancel-keymap-edit")
                                            .h_7()
                                            .px_3()
                                            .rounded_md()
                                            .text_xs()
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(border())))
                                            .with_focus_ring()
                                            .on_click(cx.listener(|this, _, _, cx| this.cancel_keymap_edit(cx)))
                                            .child("Cancel"),
                                    )
                                    .child(
                                        div()
                                            .id("save-keymap-edit")
                                            .h_7()
                                            .px_3()
                                            .rounded_md()
                                            .bg(rgb(accent_background()))
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(accent_hover())))
                                            .with_focus_ring()
                                            .on_click(cx.listener(|this, _, _, cx| this.commit_keymap_edit(cx)))
                                            .child("Save"),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_keymap_conflict_sheet(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(conflict) = self.keymap_conflict.as_ref() else {
            return div().into_any_element();
        };
        let details = conflict
            .collisions
            .iter()
            .map(|collision| {
                let label = Self::keymap_descriptor(&collision.action)
                    .map_or(collision.action.as_str(), |descriptor| descriptor.label);
                format!("{label} ({})", collision.keystrokes)
            })
            .collect::<Vec<_>>()
            .join(", ");
        div()
            .id("keymap-conflict-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .bg(Hsla::from(rgb(background())).opacity(0.65))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .w(px(420.0))
                    .max_w(px(520.0))
                    .p_5()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(border()))
                    .bg(rgb(surface()))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Reassign keybinding?"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child(format!("This conflicts with {details}. Reassigning removes those bindings in both profiles.")),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                div()
                                    .id("cancel-keymap-conflict")
                                    .h_7()
                                    .px_3()
                                    .rounded_md()
                                    .text_xs()
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(border())))
                                    .with_focus_ring()
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel_keymap_edit(cx)))
                                    .child("Cancel"),
                            )
                            .child(
                                div()
                                    .id("confirm-keymap-conflict")
                                    .h_7()
                                    .px_3()
                                    .rounded_md()
                                    .bg(rgb(accent_background()))
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(accent_hover())))
                                    .with_focus_ring()
                                    .on_click(cx.listener(|this, _, _, cx| this.confirm_keymap_reassign(cx)))
                                    .child("Reassign"),
                            ),
                    ),
            )
            .into_any_element()
    }
}
