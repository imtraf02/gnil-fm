impl FileManager {
    fn handle_escape(
        &mut self,
        _: &HandleEscape,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.inline_rename.is_some() {
            self.cancel_inline_rename(&CancelInlineRename, window, cx);
            return;
        }
        if self.open_with_chooser.is_some() {
            self.dismiss_open_with(&DismissOpenWith, window, cx);
            return;
        }
        if matches!(
            self.operation_sheet.as_ref(),
            Some(OperationSheet::BulkRename { .. })
        ) {
            self.dismiss_sheet(&DismissSheet, window, cx);
            window.focus(&self.focus_handle(cx));
            return;
        }
        if self.file_search.open {
            self.dismiss_file_search(&DismissFileSearch, window, cx);
            return;
        }
        if self.operation_sheet.is_some() {
            self.dismiss_sheet(&DismissSheet, window, cx);
            window.focus(&self.focus_handle(cx));
            return;
        }
        if self.appearance_menu_open
            || self.action_menu.is_some()
            || self.empty_space_menu.is_some()
            || self.command_bar_menu.is_some()
        {
            self.dismiss_menu(&DismissMenu, window, cx);
            return;
        }
        if self.selection.clear() {
            window.focus(&self.focus_handle(cx));
            self.selection_changed(cx);
        }
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let delta = if self.settings.file_layout == FileLayout::Grid {
            isize::try_from(self.grid_columns).unwrap_or(1)
        } else {
            1
        };
        if self
            .selection
            .move_cursor(delta, &self.snapshot.entries, false)
        {
            self.scroll_keyboard_cursor();
            self.selection_changed_debounced(cx);
        }
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        let delta = if self.settings.file_layout == FileLayout::Grid {
            -isize::try_from(self.grid_columns).unwrap_or(1)
        } else {
            -1
        };
        if self.selection.move_cursor(delta, &self.snapshot.entries, false) {
            self.scroll_keyboard_cursor();
            self.selection_changed_debounced(cx);
        }
    }

    fn select_next_range(&mut self, _: &SelectNextRange, _: &mut Window, cx: &mut Context<Self>) {
        let delta = if self.settings.file_layout == FileLayout::Grid {
            isize::try_from(self.grid_columns).unwrap_or(1)
        } else {
            1
        };
        if self.selection.move_cursor(delta, &self.snapshot.entries, true) {
            self.scroll_keyboard_cursor();
            self.selection_changed_debounced(cx);
        }
    }

    fn select_previous_range(
        &mut self,
        _: &SelectPreviousRange,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = if self.settings.file_layout == FileLayout::Grid {
            -isize::try_from(self.grid_columns).unwrap_or(1)
        } else {
            -1
        };
        if self.selection.move_cursor(delta, &self.snapshot.entries, true) {
            self.scroll_keyboard_cursor();
            self.selection_changed_debounced(cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_grid_cursor(-1, false, cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_grid_cursor(1, false, cx);
    }

    fn select_left_range(&mut self, _: &SelectLeftRange, _: &mut Window, cx: &mut Context<Self>) {
        self.move_grid_cursor(-1, true, cx);
    }

    fn select_right_range(&mut self, _: &SelectRightRange, _: &mut Window, cx: &mut Context<Self>) {
        self.move_grid_cursor(1, true, cx);
    }

    fn move_grid_cursor(&mut self, delta: isize, extend: bool, cx: &mut Context<Self>) {
        if self.settings.file_layout != FileLayout::Grid {
            return;
        }
        if self
            .selection
            .move_cursor(delta, &self.snapshot.entries, extend)
        {
            self.scroll_keyboard_cursor();
            self.selection_changed_debounced(cx);
        }
    }

    fn scroll_keyboard_cursor(&self) {
        let Some(index) = self.selection.cursor else {
            return;
        };
        if self.rendered_file_range.contains(&index) {
            return;
        }
        let list_index = if self.settings.file_layout == FileLayout::Grid {
            index / self.grid_columns.max(1)
        } else {
            index
        };
        self.file_list_scroll
            .scroll_to_item_strict(list_index, ScrollStrategy::Center);
    }

    fn toggle_selection(&mut self, _: &ToggleSelection, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selection.cursor {
            if self.selection.toggle(index, &self.snapshot.entries) {
                self.selection_changed(cx);
            }
        }
    }

    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        self.update_selected_path();
        if self.preview_visible {
            self.load_preview(cx);
        }
        self.invalidate_surfaces(SurfaceMask::FILE_LIST | SurfaceMask::STATUS, cx);
    }

    fn selection_changed_debounced(&mut self, cx: &mut Context<Self>) {
        self.update_selected_path();
        if self.preview_visible {
            self.schedule_preview(cx);
        }
        self.invalidate_surfaces(SurfaceMask::FILE_LIST | SurfaceMask::STATUS, cx);
    }

    fn update_selected_path(&mut self) {
        self.tab.selected_path = self
            .selection
            .cursor
            .and_then(|index| self.snapshot.entries.get(index))
            .map(|entry| entry.path.clone());
    }

    fn selected_paths_cached(&mut self) -> Arc<[PathBuf]> {
        let revision = self.selection.selected_revision();
        if self.selected_paths_cache_revision != revision
            || self.selected_paths_cache_generation != self.generation
        {
            self.selected_paths_cache = self
                .selection
                .effective_paths(&self.snapshot.entries)
                .into();
            self.selected_paths_cache_revision = revision;
            self.selected_paths_cache_generation = self.generation;
        }
        Arc::clone(&self.selected_paths_cache)
    }

    fn open_selected(&mut self, _: &OpenSelected, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selection.cursor {
            self.open_index(index, cx);
        }
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.back() {
            self.load_directory(cx);
        }
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.forward() {
            self.load_directory(cx);
        }
    }

    fn go_up(&mut self, _: &GoUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.up() {
            self.load_directory(cx);
        }
    }

    fn toggle_preview(&mut self, _: &TogglePreview, _: &mut Window, cx: &mut Context<Self>) {
        self.preview_visible = !self.preview_visible;
        if self.preview_visible {
            self.load_preview(cx);
        } else {
            self.cancel_preview_request();
        }
        cx.notify();
    }

    fn toggle_hidden(&mut self, _: &ToggleHidden, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        self.tab.show_hidden = !self.tab.show_hidden;
        self.load_directory(cx);
    }

    fn refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.load_directory(cx);
    }

    fn activate_path_input(
        &mut self,
        _: &ActivatePathInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open_with_chooser.is_some()
            || self.operation_sheet.is_some()
            || self.action_menu.is_some()
            || self.empty_space_menu.is_some()
            || self.appearance_menu_open
            || self.command_bar_menu.is_some()
        {
            return;
        }
        if self.file_search.open {
            self.finish_file_search_close(cx);
        }
        if self.path_input.editing {
            self.path_input.input.update(cx, |input, cx| {
                input.select_all(cx);
                window.focus(&input.focus_handle(cx));
            });
            return;
        }
        let value = self.tab.path.display().to_string();
        self.path_input.begin(self.tab.path.clone());
        self.path_input.expect_programmatic_change();
        self.path_input.input.update(cx, |input, cx| {
            input.set_text(value, cx);
            input.set_invalid(false, cx);
            input.select_all(cx);
            window.focus(&input.focus_handle(cx));
        });
        cx.notify();
    }

    fn dismiss_path_input(
        &mut self,
        _: &DismissPathInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.path_input.editing {
            return;
        }
        self.path_input.dismiss();
        self.path_input
            .input
            .update(cx, |input, cx| input.set_invalid(false, cx));
        window.focus(&self.focus_handle(cx));
        cx.notify();
    }

    fn complete_path_next(&mut self, _: &CompletePathNext, _: &mut Window, cx: &mut Context<Self>) {
        self.complete_path(false, cx);
    }

    fn complete_path_previous(
        &mut self,
        _: &CompletePathPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.complete_path(true, cx);
    }

    fn complete_path(&mut self, reverse: bool, cx: &mut Context<Self>) {
        if !self.path_input.editing || self.path_input.checking {
            return;
        }
        if !self.path_input.suggestions.is_empty() {
            if self.path_input.move_suggestion(reverse) {
                cx.notify();
            }
            return;
        }
        let input = self.path_input.input.read(cx).text().to_owned();
        let base_path = self.path_input.base_path.clone();
        let home_dir = dirs::home_dir();
        let show_hidden = self.tab.show_hidden;
        let generation = self.path_input.begin_request();
        self.path_input
            .input
            .update(cx, |input, cx| input.set_invalid(false, cx));
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            completion_candidates(&input, &base_path, home_dir.as_deref(), show_hidden)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(mut suggestions) if suggestions.len() == 1 => {
                    let suggestion = suggestions.pop().expect("single suggestion");
                    if this
                        .path_input
                        .apply_suggestions(generation, Vec::new(), reverse)
                    {
                        this.path_input.expect_programmatic_change();
                        this.path_input.input.update(cx, |input, cx| {
                            input.set_text(suggestion.input, cx);
                            input.set_invalid(false, cx);
                        });
                        cx.notify();
                    }
                }
                Ok(mut suggestions) => {
                    suggestions.truncate(8);
                    if this
                        .path_input
                        .apply_suggestions(generation, suggestions, reverse)
                    {
                        cx.notify();
                    }
                }
                Err(error) => {
                    if this.path_input.apply_error(generation, error) {
                        this.path_input
                            .input
                            .update(cx, |input, cx| input.set_invalid(true, cx));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn path_history_previous(
        &mut self,
        _: &PathHistoryPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.path_input.editing || self.path_input.checking {
            return;
        }
        let current = self.path_input.input.read(cx).text().to_owned();
        if let Some(value) = self.path_input.history_previous(&current) {
            self.set_path_input_text(value, cx);
        }
    }

    fn path_history_next(&mut self, _: &PathHistoryNext, _: &mut Window, cx: &mut Context<Self>) {
        if !self.path_input.editing || self.path_input.checking {
            return;
        }
        if let Some(value) = self.path_input.history_next() {
            self.set_path_input_text(value, cx);
        }
    }

    fn set_path_input_text(&mut self, value: String, cx: &mut Context<Self>) {
        self.path_input.expect_programmatic_change();
        self.path_input.input.update(cx, |input, cx| {
            input.set_text(value, cx);
            input.set_invalid(false, cx);
        });
        cx.notify();
    }

    fn paste_path(&mut self, _: &PastePath, window: &mut Window, cx: &mut Context<Self>) {
        if !self.path_input.editing || self.path_input.checking {
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        match single_pasted_path(&text) {
            Ok(path) => {
                self.path_input.input.update(cx, |input, cx| {
                    input.replace_selection(&path, window, cx);
                    input.set_invalid(false, cx);
                });
            }
            Err(error) => self.set_path_input_error(error, cx),
        }
    }

    fn submit_path_input(
        &mut self,
        _: &SubmitPathInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.path_input.editing || self.path_input.checking {
            return;
        }
        if let Some(suggestion) = self.path_input.focused_suggestion() {
            self.accept_path_suggestion(suggestion, cx);
            return;
        }
        let raw = self.path_input.input.read(cx).text().to_owned();
        let path = match resolve_path_input(
            &raw,
            &self.path_input.base_path,
            dirs::home_dir().as_deref(),
        ) {
            Ok(path) => path,
            Err(error) => {
                self.set_path_input_error(error, cx);
                return;
            }
        };
        let generation = self.path_input.begin_request();
        self.path_input
            .input
            .update(cx, |input, cx| input.set_invalid(false, cx));
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { validate_path(path) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.path_input.generation || !this.path_input.editing {
                    return;
                }
                this.path_input.checking = false;
                match result {
                    Ok(target) => {
                        this.path_input.record_success(raw);
                        this.path_input.dismiss();
                        this.path_input
                            .input
                            .update(cx, |input, cx| input.set_invalid(false, cx));
                        match target {
                            PathTarget::Directory(path) => {
                                this.pending_reveal = None;
                                this.navigate_path(path);
                            }
                            PathTarget::File { path, parent } => {
                                this.pending_reveal = Some(path);
                                this.navigate_path(parent);
                            }
                        }
                        window.focus(&this.focus_handle(cx));
                        this.load_directory(cx);
                    }
                    Err(error) => {
                        this.path_input.error = Some(error);
                        this.path_input.input.update(cx, |input, cx| {
                            input.set_invalid(true, cx);
                        });
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn accept_path_suggestion(&mut self, suggestion: PathSuggestion, cx: &mut Context<Self>) {
        self.path_input.suggestions.clear();
        self.path_input.focused_suggestion = None;
        self.set_path_input_text(suggestion.input, cx);
    }

    fn set_path_input_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.path_input.set_error(error);
        self.path_input
            .input
            .update(cx, |input, cx| input.set_invalid(true, cx));
        cx.notify();
    }

    fn toggle_actions(&mut self, _: &ToggleActions, _: &mut Window, cx: &mut Context<Self>) {
        if self.action_menu.is_some() {
            self.dismiss_action_menu(cx);
        } else {
            self.command_bar_menu = None;
            self.appearance_menu_open = false;
            self.open_action_menu(ActionMenuPlacement::Header, cx);
        }
    }

    fn toggle_appearance(&mut self, _: &ToggleAppearance, _: &mut Window, cx: &mut Context<Self>) {
        if self.appearance_menu_open && !self.appearance_menu_closing {
            self.dismiss_appearance_menu(cx);
        } else {
            self.action_menu = None;
            self.empty_space_menu = None;
            self.command_bar_menu = None;
            self.appearance_menu_open = true;
            self.appearance_menu_closing = false;
            cx.notify();
        }
    }

    fn toggle_settings(&mut self, _: &ToggleSettings, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window;
        self.action_menu = None;
        self.empty_space_menu = None;
        self.command_bar_menu = None;
        self.appearance_menu_open = false;
        open_preferences_window(SettingsCategory::Appearance, cx);
    }

    fn open_keymap(&mut self, _: &OpenKeymap, _: &mut Window, cx: &mut Context<Self>) {
        self.action_menu = None;
        self.empty_space_menu = None;
        self.command_bar_menu = None;
        self.appearance_menu_open = false;
        open_preferences_window(SettingsCategory::Keymap, cx);
    }

    fn sync_preferences(
        &mut self,
        settings: &AppSettings,
        keymap: &KeymapOverrides,
        cx: &mut Context<Self>,
    ) {
        let show_hidden_changed = self.settings.show_hidden != settings.show_hidden;
        let sort_changed = self.settings.file_sort != settings.file_sort;
        let layout_changed = self.settings.file_layout != settings.file_layout;
        let preview_was_visible = self.preview_visible;
        self.settings = settings.clone();
        self.refresh_favorite_paths();
        self.refresh_favorite_availability(cx);
        self.tab.show_hidden = self.settings.show_hidden;
        self.tab.sort = self.settings.file_sort;
        self.preview_visible = self.settings.preview_enabled;
        self.auto_mount_removable = self.settings.auto_mount_removable;
        self.reduced_motion = self.settings.reduced_motion;
        self.preview_cache
            .lock()
            .expect("preview cache lock poisoned")
            .set_capacity_mib(self.settings.memory_cache_mib);
        if layout_changed {
            self.file_list_scroll = UniformListScrollHandle::new();
            self.rendered_file_range = 0..0;
            if self.settings.file_layout == FileLayout::Details {
                self.reset_grid_previews();
            }
        }

        let theme_appearance = resolve_theme_appearance(self.settings.theme, self.theme_appearance);
        let requested_theme = selected_theme_name(&self.settings, theme_appearance);
        let (active_theme, _) = self.theme_catalog.resolve(requested_theme, theme_appearance);
        self.active_theme_name = active_theme.name.clone();
        theme_runtime::set_active(active_theme.colors);
        PreferencesWindow::install_keybindings(keymap, cx);

        if show_hidden_changed {
            self.load_directory(cx);
        } else if sort_changed {
            self.apply_sort_to_snapshot(cx);
        } else if self.preview_visible && !preview_was_visible {
            self.load_preview(cx);
        } else if !self.preview_visible && preview_was_visible {
            self.cancel_preview_request();
        }
        cx.notify();
    }

    fn dismiss_appearance_menu(&mut self, cx: &mut Context<Self>) {
        if !self.appearance_menu_open || self.appearance_menu_closing {
            return;
        }
        self.appearance_menu_closing = true;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(80));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.appearance_menu_closing {
                    this.appearance_menu_open = false;
                    this.appearance_menu_closing = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn set_theme_mode(&mut self, mode: ThemeMode, window: &Window, cx: &mut Context<Self>) {
        self.settings.theme = mode;
        self.apply_selected_theme(window_theme_appearance(window), cx);
        self.save_settings(cx);
    }

    fn select_theme(&mut self, name: &str, cx: &mut Context<Self>) {
        match self.theme_appearance {
            ThemeAppearance::Light => name.clone_into(&mut self.settings.light_theme),
            ThemeAppearance::Dark => name.clone_into(&mut self.settings.dark_theme),
        }
        self.apply_theme_name(name, cx);
        self.save_settings(cx);
    }

    fn reload_themes(&mut self, cx: &mut Context<Self>) {
        self.theme_catalog = ThemeCatalog::load(&self.config_paths.themes_dir());
        let requested = selected_theme_name(&self.settings, self.theme_appearance).to_owned();
        self.apply_theme_name(&requested, cx);
        if self.theme_catalog.errors.is_empty() {
            self.status_message = Some("Themes reloaded".into());
        } else {
            self.status_message = Some(format!(
                "{} theme file{} could not be loaded",
                self.theme_catalog.errors.len(),
                if self.theme_catalog.errors.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ));
        }
        cx.notify();
    }

    fn apply_selected_theme(&mut self, system_appearance: ThemeAppearance, cx: &mut Context<Self>) {
        self.theme_appearance = resolve_theme_appearance(self.settings.theme, system_appearance);
        let requested = selected_theme_name(&self.settings, self.theme_appearance).to_owned();
        self.apply_theme_name(&requested, cx);
    }

    fn apply_theme_name(&mut self, requested: &str, cx: &mut Context<Self>) {
        let (theme, fallback) = self.theme_catalog.resolve(requested, self.theme_appearance);
        self.active_theme_name.clone_from(&theme.name);
        theme_runtime::set_active(theme.colors);
        if fallback {
            self.status_message = Some(format!(
                "Theme “{requested}” is unavailable; using {}",
                theme.name
            ));
        }
        cx.notify();
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let config_paths = self.config_paths.clone();
        self.preferences.update(cx, |preferences, cx| {
            preferences.config_paths = config_paths;
            if let Err(error) = preferences.save_settings(settings) {
                eprintln!("Could not save settings: {error}");
            }
            cx.notify();
        });
    }

    fn selected_favorite_candidate(&self) -> Option<PathBuf> {
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        let [path] = paths.as_slice() else {
            return None;
        };
        self.snapshot
            .entries
            .iter()
            .find(|entry| entry.path == *path && entry.is_directory_like())
            .map(|entry| entry.path.clone())
    }

    fn toggle_selected_favorite(
        &mut self,
        _: &ToggleFavorite,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self.selected_favorite_candidate() {
            self.toggle_favorite_path(path, cx);
        }
    }

    fn toggle_favorite_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(index) = self
            .settings
            .favorites
            .iter()
            .position(|favorite| favorite == &path)
        {
            self.settings.favorites.remove(index);
            self.favorite_availability.remove(&path);
            self.status_message = Some("Removed from Favorites".into());
        } else {
            self.favorite_availability.insert(path.clone(), true);
            self.settings.favorites.push(path);
            self.status_message = Some("Added to Favorites".into());
        }
        self.refresh_favorite_paths();
        self.save_settings(cx);
        cx.notify();
    }

    fn remove_favorite_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        let count = self.settings.favorites.len();
        self.settings
            .favorites
            .retain(|favorite| favorite != path);
        if self.settings.favorites.len() != count {
            self.refresh_favorite_paths();
            self.favorite_availability.remove(path);
            self.status_message = Some("Removed unavailable favorite".into());
            self.save_settings(cx);
            cx.notify();
        }
    }

    fn reorder_favorite_path(&mut self, source: &Path, target_index: usize, cx: &mut Context<Self>) {
        let Some(source_index) = self
            .settings
            .favorites
            .iter()
            .position(|favorite| favorite == source)
        else {
            return;
        };
        if source_index == target_index {
            return;
        }
        let favorite = self.settings.favorites.remove(source_index);
        let insertion = target_index.min(self.settings.favorites.len());
        self.settings.favorites.insert(insertion, favorite);
        self.refresh_favorite_paths();
        self.save_settings(cx);
        cx.notify();
    }

    fn refresh_favorite_availability(&mut self, cx: &mut Context<Self>) {
        let favorites = self.settings.favorites.clone();
        if favorites.is_empty() {
            self.favorite_availability.clear();
            return;
        }
        let task = cx.background_executor().spawn(async move {
            favorites
                .into_iter()
                .map(|favorite| {
                    let available = favorite.is_dir();
                    (favorite, available)
                })
                .collect::<HashMap<_, _>>()
        });
        cx.spawn(async move |this, cx| {
            let availability = task.await;
            let _ = this.update(cx, |this, cx| {
                let current: HashSet<_> = this.settings.favorites.iter().cloned().collect();
                this.favorite_availability
                    .retain(|path, _| current.contains(path));
                for (path, available) in availability {
                    if current.contains(&path) {
                        this.favorite_availability.insert(path, available);
                    }
                }
                this.invalidate_surfaces(SurfaceMask::SIDEBAR, cx);
            });
        })
        .detach();
    }

    fn refresh_favorite_paths(&mut self) {
        self.favorite_paths = Arc::new(self.settings.favorites.iter().cloned().collect());
    }

    fn open_favorite(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .favorite_availability
            .get(&path)
            .copied()
            .unwrap_or(true)
        {
            self.pending_reveal = None;
            self.tab.navigate(path);
            self.load_directory(cx);
            return;
        }
        let detail = path.display().to_string();
        let answer = window.prompt(
            PromptLevel::Warning,
            "Remove this unavailable favorite?",
            Some(&detail),
            &["Remove from Favorites", "Keep"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.remove_favorite_path(&path, cx);
            });
        })
        .detach();
    }

    fn system_appearance_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.theme == ThemeMode::System {
            self.apply_selected_theme(window_theme_appearance(window), cx);
        }
    }

    fn open_action_menu(&mut self, placement: ActionMenuPlacement, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        self.empty_space_menu = None;
        self.command_bar_menu = None;
        self.appearance_menu_open = false;
        let clipboard_valid = self.file_clipboard_from_system(cx).is_some();
        let context = MenuContext::from_selection(
            &self.selection,
            &self.snapshot.entries,
            &self.settings.favorites,
            clipboard_valid,
            self.operation_running,
        );
        self.action_menu_serial = self.action_menu_serial.wrapping_add(1);
        self.action_menu = Some(ActionMenuState::new(
            placement,
            context,
            self.action_menu_serial,
        ));
        cx.notify();
    }

    fn dismiss_action_menu(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.action_menu.as_mut() else {
            return;
        };
        if menu.animation == MenuAnimationState::Closing {
            return;
        }
        menu.animation = MenuAnimationState::Closing;
        let serial = menu.serial;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(80));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.action_menu.as_ref().is_some_and(|menu| {
                    menu.serial == serial && menu.animation == MenuAnimationState::Closing
                }) {
                    this.action_menu = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn open_empty_space_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.action_menu = None;
        self.command_bar_menu = None;
        self.appearance_menu_open = false;
        self.selection.clear();
        self.tab.selected_path = None;
        self.cancel_preview_request();
        self.preview = None;
        self.preview_path = None;
        let context = EmptySpaceMenuContext {
            capabilities: EmptySpaceMenuCapabilities {
                clipboard_valid: self.file_clipboard_from_system(cx).is_some(),
                operation_running: self.operation_running,
                has_entries: !self.snapshot.entries.is_empty(),
            },
            sort: self.tab.sort,
            view: EmptySpaceViewState {
                show_hidden: self.tab.show_hidden,
            },
        };
        self.empty_space_menu_serial = self.empty_space_menu_serial.wrapping_add(1);
        self.empty_space_menu = Some(EmptySpaceMenuState::new(
            position,
            context,
            self.empty_space_menu_serial,
        ));
        cx.notify();
    }

    fn dismiss_empty_space_menu(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.empty_space_menu.as_mut() else {
            return;
        };
        if menu.animation == MenuAnimationState::Closing {
            return;
        }
        menu.animation = MenuAnimationState::Closing;
        let serial = menu.serial;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(80));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.empty_space_menu.as_ref().is_some_and(|menu| {
                    menu.serial == serial && menu.animation == MenuAnimationState::Closing
                }) {
                    this.empty_space_menu = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn dismiss_menu(&mut self, _: &DismissMenu, _: &mut Window, cx: &mut Context<Self>) {
        if self.appearance_menu_open {
            self.dismiss_appearance_menu(cx);
        } else if self.command_bar_menu.is_some() {
            self.dismiss_command_bar_menu(cx);
        } else if self
            .empty_space_menu
            .as_mut()
            .is_some_and(EmptySpaceMenuState::close_submenu)
        {
            cx.notify();
        } else if self.empty_space_menu.is_some() {
            self.dismiss_empty_space_menu(cx);
        } else if self
            .action_menu
            .as_mut()
            .is_some_and(ActionMenuState::close_submenu)
        {
            cx.notify();
        } else {
            self.dismiss_action_menu(cx);
        }
    }

    fn menu_next(&mut self, _: &MenuNext, _: &mut Window, cx: &mut Context<Self>) {
        if self.command_bar_menu.is_some() {
            let items = self
                .command_bar_menu
                .as_ref()
                .map(|menu| self.command_bar_menu_items(menu.kind))
                .unwrap_or_default();
            if let Some(menu) = self.command_bar_menu.as_mut()
                && menu.animation != MenuAnimationState::Closing
            {
                menu.move_focus(&items, 1);
                cx.notify();
            }
        } else if let Some(menu) = self.empty_space_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.move_focus(1);
            cx.notify();
        } else if let Some(menu) = self.action_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.move_focus(1);
            cx.notify();
        }
    }

    fn menu_previous(&mut self, _: &MenuPrevious, _: &mut Window, cx: &mut Context<Self>) {
        if self.command_bar_menu.is_some() {
            let items = self
                .command_bar_menu
                .as_ref()
                .map(|menu| self.command_bar_menu_items(menu.kind))
                .unwrap_or_default();
            if let Some(menu) = self.command_bar_menu.as_mut()
                && menu.animation != MenuAnimationState::Closing
            {
                menu.move_focus(&items, -1);
                cx.notify();
            }
        } else if let Some(menu) = self.empty_space_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.move_focus(-1);
            cx.notify();
        } else if let Some(menu) = self.action_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.move_focus(-1);
            cx.notify();
        }
    }

    fn menu_first(&mut self, _: &MenuFirst, _: &mut Window, cx: &mut Context<Self>) {
        if self.command_bar_menu.is_some() {
            let items = self
                .command_bar_menu
                .as_ref()
                .map(|menu| self.command_bar_menu_items(menu.kind))
                .unwrap_or_default();
            if let Some(menu) = self.command_bar_menu.as_mut()
                && menu.animation != MenuAnimationState::Closing
            {
                menu.focus_first(&items);
                cx.notify();
            }
        } else if let Some(menu) = self.empty_space_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.focus_first();
            cx.notify();
        } else if let Some(menu) = self.action_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.focus_first();
            cx.notify();
        }
    }

    fn menu_last(&mut self, _: &MenuLast, _: &mut Window, cx: &mut Context<Self>) {
        if self.command_bar_menu.is_some() {
            let items = self
                .command_bar_menu
                .as_ref()
                .map(|menu| self.command_bar_menu_items(menu.kind))
                .unwrap_or_default();
            if let Some(menu) = self.command_bar_menu.as_mut()
                && menu.animation != MenuAnimationState::Closing
            {
                menu.focus_last(&items);
                cx.notify();
            }
        } else if let Some(menu) = self.empty_space_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.focus_last();
            cx.notify();
        } else if let Some(menu) = self.action_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.focus_last();
            cx.notify();
        }
    }

    fn menu_activate(&mut self, _: &MenuActivate, window: &mut Window, cx: &mut Context<Self>) {
        let command_bar_command = self
            .command_bar_menu
            .as_ref()
            .filter(|menu| menu.animation != MenuAnimationState::Closing)
            .and_then(|menu| {
                let items = self.command_bar_menu_items(menu.kind);
                menu.focused
                    .and_then(|index| items.get(index))
                    .filter(|item| item.enabled)
                    .map(|item| item.command)
            });
        if let Some(command) = command_bar_command {
            self.dispatch_command_bar_command(command, window, cx);
            return;
        }
        let empty_space_activation = self
            .empty_space_menu
            .as_ref()
            .filter(|menu| menu.animation != MenuAnimationState::Closing)
            .and_then(EmptySpaceMenuState::focused_activation);
        if let Some(activation) = empty_space_activation {
            match activation {
                EmptySpaceMenuActivation::Command(command) => {
                    self.dispatch_empty_space_command(command, window, cx);
                }
                EmptySpaceMenuActivation::Submenu(submenu) => {
                    if let Some(menu) = self.empty_space_menu.as_mut() {
                        menu.open_submenu(submenu);
                        cx.notify();
                    }
                }
            }
            return;
        }
        let activation = self
            .action_menu
            .as_ref()
            .filter(|menu| menu.animation != MenuAnimationState::Closing)
            .and_then(ActionMenuState::focused_activation);
        if let Some(activation) = activation {
            self.dispatch_action_menu_activation(activation, window, cx);
        }
    }

    fn menu_open_submenu(&mut self, _: &MenuOpenSubmenu, _: &mut Window, cx: &mut Context<Self>) {
        let submenu = self
            .empty_space_menu
            .as_ref()
            .and_then(EmptySpaceMenuState::focused_activation)
            .and_then(|activation| match activation {
                EmptySpaceMenuActivation::Submenu(submenu) => Some(submenu),
                EmptySpaceMenuActivation::Command(_) => None,
            });
        if let Some(submenu) = submenu
            && let Some(menu) = self.empty_space_menu.as_mut()
        {
            menu.open_submenu(submenu);
            cx.notify();
            return;
        }
        if self
            .action_menu
            .as_ref()
            .and_then(ActionMenuState::focused_activation)
            == Some(ActionMenuActivation::Submenu(FileMenuSubmenu::OpenWith))
        {
            self.show_open_with_submenu(cx);
        }
    }

    fn menu_close_submenu(&mut self, _: &MenuCloseSubmenu, _: &mut Window, cx: &mut Context<Self>) {
        let closed_empty_submenu = self
            .empty_space_menu
            .as_mut()
            .is_some_and(EmptySpaceMenuState::close_submenu);
        let closed_action_submenu = !closed_empty_submenu
            && self
                .action_menu
                .as_mut()
                .is_some_and(ActionMenuState::close_submenu);
        if closed_empty_submenu || closed_action_submenu {
            cx.notify();
        }
    }

    fn dispatch_action_menu_activation(
        &mut self,
        activation: ActionMenuActivation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match activation {
            ActionMenuActivation::Command(command) => {
                self.dispatch_menu_command(command, window, cx);
            }
            ActionMenuActivation::Submenu(FileMenuSubmenu::OpenWith) => {
                self.show_open_with_submenu(cx);
            }
            ActionMenuActivation::OpenWithApplication(application) => {
                self.launch_with_application(application, false, window, cx);
            }
            ActionMenuActivation::ChooseAnotherApplication => {
                let target = self.action_menu.as_ref().and_then(|menu| {
                    menu.open_with_submenu
                        .as_ref()
                        .map(|submenu| (submenu.path.clone(), submenu.mime_type.clone()))
                });
                if let Some((path, mime_type)) = target {
                    self.open_open_with_chooser(path, mime_type, window, cx);
                }
            }
        }
    }

    fn dispatch_menu_command(
        &mut self,
        command: FileMenuCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.action_menu = None;
        cx.notify();
        match command {
            FileMenuCommand::Open => {
                let paths = self.selection.effective_paths(&self.snapshot.entries);
                if let [path] = paths.as_slice()
                    && let Some(index) = self
                        .snapshot
                        .entries
                        .iter()
                        .position(|entry| &entry.path == path)
                {
                    self.open_index(index, cx);
                }
            }
            FileMenuCommand::Extract => {
                self.extract_selected(&ExtractSelected, window, cx);
            }
            FileMenuCommand::ExtractTo => {
                self.extract_selected_to(&ExtractSelectedTo, window, cx);
            }
            FileMenuCommand::Copy => self.copy_selected(&CopySelected, window, cx),
            FileMenuCommand::Cut => self.cut_selected(&CutSelected, window, cx),
            FileMenuCommand::Paste => self.paste(&Paste, window, cx),
            FileMenuCommand::Rename => self.open_rename(&OpenRename, window, cx),
            FileMenuCommand::ToggleFavorite => {
                self.toggle_selected_favorite(&ToggleFavorite, window, cx);
            }
            FileMenuCommand::CreateSymlink => {
                self.open_create_symlink(&OpenCreateSymlink, window, cx);
            }
            FileMenuCommand::Permissions => {
                self.open_permissions(&OpenPermissions, window, cx);
            }
            FileMenuCommand::CopyPathAbsolute => {
                self.copy_path_absolute(&CopyPathAbsolute, window, cx);
            }
            FileMenuCommand::CopyPathRelative => {
                self.copy_path_relative(&CopyPathRelative, window, cx);
            }
            FileMenuCommand::Trash => self.trash_selected(&TrashSelected, window, cx),
            FileMenuCommand::DeletePermanently => {
                self.delete_selected(&DeleteSelected, window, cx);
            }
        }
    }

    fn dispatch_empty_space_command(
        &mut self,
        command: EmptySpaceMenuCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.empty_space_menu = None;
        match command {
            EmptySpaceMenuCommand::NewFolder => self.create_folder(&CreateFolder, window, cx),
            EmptySpaceMenuCommand::NewFile => self.create_file(&CreateFile, window, cx),
            EmptySpaceMenuCommand::Paste => self.paste(&Paste, window, cx),
            EmptySpaceMenuCommand::Refresh => self.refresh(&Refresh, window, cx),
            EmptySpaceMenuCommand::SortField(field) => {
                self.set_sort_field(field, cx);
            }
            EmptySpaceMenuCommand::SortDirection(direction) => {
                self.set_sort_direction(direction, cx);
            }
            EmptySpaceMenuCommand::ToggleHidden => {
                self.toggle_hidden(&ToggleHidden, window, cx);
            }
            EmptySpaceMenuCommand::SelectAll => self.select_all_entries(cx),
            EmptySpaceMenuCommand::OpenTerminal => self.open_terminal_here(cx),
            EmptySpaceMenuCommand::FolderProperties => self.open_folder_properties(cx),
        }
    }

    fn toggle_command_bar_menu(
        &mut self,
        kind: CommandBarMenuKind,
        cx: &mut Context<Self>,
    ) {
        if self.command_bar_menu.as_ref().is_some_and(|menu| {
            menu.kind == kind && menu.animation != MenuAnimationState::Closing
        }) {
            self.dismiss_command_bar_menu(cx);
            return;
        }
        self.action_menu = None;
        self.empty_space_menu = None;
        self.appearance_menu_open = false;
        self.command_bar_menu_serial = self.command_bar_menu_serial.wrapping_add(1);
        let items = self.command_bar_menu_items(kind);
        self.command_bar_menu = Some(CommandBarMenuState::new(
            kind,
            &items,
            self.command_bar_menu_serial,
        ));
        cx.notify();
    }

    fn dismiss_command_bar_menu(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.command_bar_menu.as_mut() else {
            return;
        };
        if menu.animation == MenuAnimationState::Closing {
            return;
        }
        menu.animation = MenuAnimationState::Closing;
        let serial = menu.serial;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(80));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.command_bar_menu.as_ref().is_some_and(|menu| {
                    menu.serial == serial && menu.animation == MenuAnimationState::Closing
                }) {
                    this.command_bar_menu = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn dispatch_command_bar_command(
        &mut self,
        command: CommandBarCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.command_bar_menu = None;
        match command {
            CommandBarCommand::Cut => self.cut_selected(&CutSelected, window, cx),
            CommandBarCommand::Copy => self.copy_selected(&CopySelected, window, cx),
            CommandBarCommand::Rename => self.open_rename(&OpenRename, window, cx),
            CommandBarCommand::Trash => self.trash_selected(&TrashSelected, window, cx),
            CommandBarCommand::NewFolder => self.create_folder(&CreateFolder, window, cx),
            CommandBarCommand::NewFile => self.create_file(&CreateFile, window, cx),
            CommandBarCommand::NewSymlink => {
                self.open_create_symlink(&OpenCreateSymlink, window, cx);
            }
            CommandBarCommand::Open => self.open_selected(&OpenSelected, window, cx),
            CommandBarCommand::OpenWith => {
                self.open_with_selected(&OpenWithSelected, window, cx);
            }
            CommandBarCommand::ToggleFavorite => {
                self.toggle_selected_favorite(&ToggleFavorite, window, cx);
            }
            CommandBarCommand::Extract => self.extract_selected(&ExtractSelected, window, cx),
            CommandBarCommand::ExtractTo => {
                self.extract_selected_to(&ExtractSelectedTo, window, cx);
            }
            CommandBarCommand::CopyPathAbsolute => {
                self.copy_path_absolute(&CopyPathAbsolute, window, cx);
            }
            CommandBarCommand::CopyPathRelative => {
                self.copy_path_relative(&CopyPathRelative, window, cx);
            }
            CommandBarCommand::Properties => {
                if self.selection.selected_count() == 0 {
                    self.open_folder_properties(cx);
                } else {
                    self.open_permissions(&OpenPermissions, window, cx);
                }
            }
            CommandBarCommand::DeletePermanently => {
                self.delete_selected(&DeleteSelected, window, cx);
            }
            CommandBarCommand::SortField(field) => self.set_sort_field(field, cx),
            CommandBarCommand::SortDirection(direction) => {
                self.set_sort_direction(direction, cx);
            }
            CommandBarCommand::Layout(layout) => {
                // Keep the old list mounted until the pointer event has finished. Replacing the
                // list under the released pointer can otherwise select the first grid card that
                // appears beneath the menu item.
                cx.defer_in(window, move |this, _, cx| {
                    this.set_file_layout(layout, cx);
                });
            }
        }
    }
}
