impl FileManager {
    fn create_folder(&mut self, _: &CreateFolder, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        self.open_create_entry(CreateEntryKind::Folder, window, cx);
    }

    fn create_file(&mut self, _: &CreateFile, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        self.open_create_entry(CreateEntryKind::File, window, cx);
    }

    fn selected_archives(&self) -> Vec<PathBuf> {
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if !paths.is_empty() && paths.iter().all(|path| gnil_fs::is_archive_candidate(path)) {
            paths
        } else {
            Vec::new()
        }
    }

    fn extract_selected(&mut self, _: &ExtractSelected, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash || self.operation_running {
            return;
        }
        let sources = self.selected_archives();
        if sources.is_empty() {
            return;
        }
        self.start_operation(
            FsOperation::ExtractArchives {
                sources,
                destination: self.tab.path.clone(),
            },
            "Preparing archives…".into(),
            false,
            cx,
        );
    }

    fn extract_selected_to(
        &mut self,
        _: &ExtractSelectedTo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab.root == TabRoot::Trash || self.operation_running {
            return;
        }
        let sources = self.selected_archives();
        if sources.is_empty() {
            return;
        }
        let destination = cx.new(|cx| {
            TextInput::new(
                "Destination folder",
                self.tab.path.display().to_string(),
                cx,
            )
        });
        destination.update(cx, |input, cx| {
            input.select_all(cx);
            window.focus(&input.focus_handle(cx));
        });
        self.operation_sheet_closing = false;
        self.operation_sheet = Some(OperationSheet::Extract {
            sources,
            destination,
        });
        self.action_menu = None;
        cx.notify();
    }

    fn cancel_operation(&mut self, _: &CancelOperation, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.operation_cancel {
            cancel.store(true, Ordering::Relaxed);
            self.status_message = Some("Cancelling…".into());
            cx.notify();
        }
    }

    fn open_create_entry(
        &mut self,
        kind: CreateEntryKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_running {
            return;
        }
        let placeholder = match kind {
            CreateEntryKind::Folder => "Folder name",
            CreateEntryKind::File => "File name",
        };
        let name = cx.new(|cx| TextInput::new(placeholder, "", cx));
        name.update(cx, |input, cx| window.focus(&input.focus_handle(cx)));
        self.operation_sheet_closing = false;
        self.operation_sheet = Some(OperationSheet::CreateEntry { kind, name });
        self.action_menu = None;
        self.empty_space_menu = None;
        cx.notify();
    }

    fn select_all_entries(&mut self, cx: &mut Context<Self>) {
        if self.selection.select_all(&self.snapshot.entries) {
            self.selection_changed(cx);
        }
    }

    fn sort_from_header(&mut self, field: SortField, cx: &mut Context<Self>) {
        if self.tab.sort.field == field {
            self.tab.sort.direction = match self.tab.sort.direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.tab.sort.field = field;
            self.tab.sort.direction = SortDirection::Ascending;
        }
        self.settings.file_sort = self.tab.sort;
        self.apply_sort_to_snapshot(cx);
        self.save_settings(cx);
    }

    fn set_sort_field(&mut self, field: SortField, cx: &mut Context<Self>) {
        self.tab.sort.field = field;
        self.settings.file_sort = self.tab.sort;
        self.apply_sort_to_snapshot(cx);
        self.save_settings(cx);
    }

    fn set_sort_direction(&mut self, direction: SortDirection, cx: &mut Context<Self>) {
        self.tab.sort.direction = direction;
        self.settings.file_sort = self.tab.sort;
        self.apply_sort_to_snapshot(cx);
        self.save_settings(cx);
    }

    fn set_file_layout(&mut self, layout: FileLayout, cx: &mut Context<Self>) {
        if self.settings.file_layout == layout {
            return;
        }
        self.settings.file_layout = layout;
        self.file_list_scroll = UniformListScrollHandle::new();
        self.rendered_file_range = 0..0;
        if layout == FileLayout::Details {
            self.reset_grid_previews();
        }
        self.save_settings(cx);
        cx.notify();
    }

    fn apply_sort_to_snapshot(&mut self, cx: &mut Context<Self>) {
        if self.snapshot.phase != DirectoryLoadPhase::Complete {
            self.load_directory(cx);
            return;
        }
        let cursor_path = self
            .selection
            .cursor
            .and_then(|index| self.snapshot.entries.get(index))
            .map(|entry| entry.path.clone());
        let anchor_path = self
            .selection
            .anchor
            .and_then(|index| self.snapshot.entries.get(index))
            .map(|entry| entry.path.clone());

        if self.tab.root == TabRoot::Trash {
            self.sort_trash_entries();
            Arc::make_mut(&mut self.snapshot).entries = self
                .trash_entries
                .iter()
                .map(trash_entry_as_file_entry)
                .collect();
        } else {
            self.tab
                .sort
                .sort(&mut Arc::make_mut(&mut self.snapshot).entries);
        }

        self.selection.cursor = cursor_path.and_then(|path| {
            self.snapshot
                .entries
                .iter()
                .position(|entry| entry.path == path)
        });
        self.selection.anchor = anchor_path.and_then(|path| {
            self.snapshot
                .entries
                .iter()
                .position(|entry| entry.path == path)
        });
        cx.notify();
    }

    fn sort_trash_entries(&mut self) {
        let sort = self.tab.sort;
        Arc::make_mut(&mut self.trash_entries).sort_by(|left, right| {
            if sort.directories_first {
                let directory_order = (right.kind == FileKind::Directory)
                    .cmp(&(left.kind == FileKind::Directory));
                if !directory_order.is_eq() {
                    return directory_order;
                }
            }
            let mut order = match sort.field {
                SortField::Name => natural_cmp(&left.name, &right.name),
                SortField::Size => left.len.cmp(&right.len),
                SortField::Modified => left.deletion_unix.cmp(&right.deletion_unix),
                SortField::Kind => trash_kind_rank(left.kind)
                    .cmp(&trash_kind_rank(right.kind))
                    .then_with(|| natural_cmp(&left.name, &right.name)),
            };
            if sort.direction == SortDirection::Descending {
                order = order.reverse();
            }
            order
        });
    }

    fn open_terminal_here(&mut self, cx: &mut Context<Self>) {
        let mut last_error = None;
        for (program, arguments) in terminal_candidates() {
            match std::process::Command::new(&program)
                .args(arguments)
                .current_dir(&self.tab.path)
                .spawn()
            {
                Ok(_) => {
                    self.status_message = Some("Opened terminal in this folder".into());
                    self.error = None;
                    cx.notify();
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    last_error = Some(error);
                }
                Err(error) => {
                    last_error = Some(error);
                    break;
                }
            }
        }
        self.error = Some(last_error.map_or_else(
            || "No terminal command is configured".into(),
            |error| format!("Could not open a terminal: {error}"),
        ));
        cx.notify();
    }

    fn open_folder_properties(&mut self, cx: &mut Context<Self>) {
        let metadata = std::fs::symlink_metadata(&self.tab.path).ok();
        #[cfg(unix)]
        let mode = metadata.as_ref().map(|metadata| {
            use std::os::unix::fs::MetadataExt as _;
            metadata.mode() & 0o7777
        });
        #[cfg(not(unix))]
        let mode = None;
        self.operation_sheet_closing = false;
        self.operation_sheet = Some(OperationSheet::FolderProperties {
            path: self.tab.path.clone(),
            item_count: self.snapshot.entries.len(),
            file_bytes: self
                .snapshot
                .entries
                .iter()
                .filter(|entry| entry.kind == FileKind::File)
                .fold(0_u64, |total, entry| {
                    total.saturating_add(entry.metadata().map_or(0, |metadata| metadata.len))
                }),
            mode,
            readonly: metadata.is_some_and(|metadata| metadata.permissions().readonly()),
        });
        cx.notify();
    }

    fn open_create_symlink(
        &mut self,
        _: &OpenCreateSymlink,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let target = cx.new(|cx| TextInput::new("Target path", "", cx));
        let name = cx.new(|cx| TextInput::new("Link name", "", cx));
        self.operation_sheet_closing = false;
        self.operation_sheet = Some(OperationSheet::Symlink {
            target,
            name,
            relative: true,
        });
        self.action_menu = None;
        cx.notify();
    }

    fn open_permissions(&mut self, _: &OpenPermissions, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let selected = self.selection.effective_paths(&self.snapshot.entries);
        let mut paths = Vec::new();
        let mut modes = Vec::new();
        let mut skipped_links = 0;
        for path in selected {
            let Some(entry) = self
                .snapshot
                .entries
                .iter()
                .find(|entry| entry.path == path)
            else {
                continue;
            };
            if entry.kind == FileKind::Symlink {
                skipped_links += 1;
                continue;
            }
            if let Some(mode) = entry.metadata().and_then(|metadata| metadata.mode) {
                paths.push(path);
                modes.push(mode & 0o7777);
            }
        }
        if paths.is_empty() {
            self.error = Some("Select at least one non-symlink item".into());
            cx.notify();
            return;
        }
        let common_mode = modes
            .first()
            .copied()
            .filter(|first| modes.iter().all(|mode| mode == first));
        let value = common_mode.map_or_else(String::new, |mode| format!("{mode:04o}"));
        let octal = cx.new(|cx| TextInput::new("mixed — use permission toggles", value, cx));
        self.operation_sheet_closing = false;
        self.operation_sheet = Some(OperationSheet::Permissions {
            paths,
            original_modes: modes.clone(),
            current_modes: modes,
            octal,
        });
        self.action_menu = None;
        if skipped_links > 0 {
            self.status_message = Some(format!(
                "Excluded {skipped_links} symlink{} from chmod",
                if skipped_links == 1 { "" } else { "s" }
            ));
        }
        cx.notify();
    }

    fn open_rename(&mut self, _: &OpenRename, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash || self.operation_running {
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        self.inline_rename = None;
        self.action_menu = None;
        self.command_bar_menu = None;
        self.error = None;

        if let [path] = paths.as_slice() {
            let Some(value) = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
            else {
                self.error = Some("This file name is not valid UTF-8".into());
                cx.notify();
                return;
            };
            let input_value = value.clone();
            let input = cx.new(move |cx| {
                TextInput::new("File name", input_value, cx)
                    .with_key_context("InlineRenameInput")
            });
            let input_subscription =
                cx.subscribe(&input, |this, input, event: &TextInputEvent, cx| {
                    if matches!(event, TextInputEvent::Changed) {
                        input.update(cx, |input, cx| input.set_invalid(false, cx));
                        this.error = None;
                        cx.notify();
                    }
                });
            let input_focus = input.focus_handle(cx);
            let blur_subscription = cx.on_blur(&input_focus, window, |this, window, cx| {
                this.finish_inline_rename(window, cx);
            });
            self.inline_rename = Some(InlineRenameState {
                path: path.clone(),
                input: input.clone(),
                _input_subscription: input_subscription,
                _blur_subscription: blur_subscription,
            });

            let selection_end = self
                .snapshot
                .entries
                .iter()
                .find(|entry| entry.path == *path)
                .filter(|entry| !entry.is_directory_like())
                .and_then(|_| Path::new(&value).file_stem())
                .and_then(|stem| stem.to_str())
                .map_or(value.len(), str::len);
            input.update(cx, |input, cx| {
                input.select_range(0..selection_end, cx);
                window.focus(&input.focus_handle(cx));
            });
        } else {
            self.operation_sheet_closing = false;
            self.operation_sheet = Some(OperationSheet::BulkRename {
                paths,
                find: cx.new(|cx| TextInput::new("Find", "", cx)),
                replace: cx.new(|cx| TextInput::new("Replace", "", cx)),
                prefix: cx.new(|cx| TextInput::new("Prefix", "", cx)),
                suffix: cx.new(|cx| TextInput::new("Suffix", "", cx)),
                start: cx.new(|cx| TextInput::new("Start", "1", cx)),
                padding: cx.new(|cx| TextInput::new("Padding", "2", cx)),
                regex: false,
                numbering: false,
                scope: RenameScope::Stem,
            });
        }
        cx.notify();
    }

    fn confirm_inline_rename(
        &mut self,
        _: &ConfirmInlineRename,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_inline_rename(window, cx);
    }

    fn cancel_inline_rename(
        &mut self,
        _: &CancelInlineRename,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.inline_rename.take().is_some() {
            self.error = None;
            window.focus(&self.focus_handle(cx));
            cx.notify();
        }
    }

    fn finish_inline_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_running {
            return;
        }
        let Some(rename) = self.inline_rename.take() else {
            return;
        };
        let name = rename.input.read(cx).text().trim().to_owned();
        let operation = (|| {
            validate_file_name(&name)?;
            let destination = rename
                .path
                .parent()
                .ok_or_else(|| "Cannot rename a filesystem root".to_owned())?
                .join(&name);
            if destination == rename.path {
                return Ok(None);
            }
            match std::fs::symlink_metadata(&destination) {
                Ok(_) => {
                    return Err(format!("“{name}” already exists in this folder"));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {}
            }
            Ok(Some(FsOperation::Rename {
                from: rename.path.clone(),
                to: destination,
            }))
        })();

        match operation {
            Ok(Some(operation)) => {
                window.focus(&self.focus_handle(cx));
                self.start_operation(operation, "Renaming…".into(), false, cx);
            }
            Ok(None) => {
                self.error = None;
                window.focus(&self.focus_handle(cx));
                cx.notify();
            }
            Err(error) => {
                rename
                    .input
                    .update(cx, |input, cx| input.set_invalid(true, cx));
                window.focus(&rename.input.focus_handle(cx));
                self.inline_rename = Some(rename);
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn dismiss_sheet(&mut self, _: &DismissSheet, _: &mut Window, cx: &mut Context<Self>) {
        if self.operation_sheet.is_none() || self.operation_sheet_closing {
            return;
        }
        if self.reduced_motion {
            self.operation_sheet = None;
            self.error = None;
            cx.notify();
            return;
        }
        self.operation_sheet_closing = true;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(80));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.operation_sheet_closing {
                    this.operation_sheet = None;
                    this.operation_sheet_closing = false;
                    this.error = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn toggle_permission_bit(&mut self, bit: u32, cx: &mut Context<Self>) {
        let Some(OperationSheet::Permissions {
            current_modes,
            octal,
            ..
        }) = self.operation_sheet.as_mut()
        else {
            return;
        };
        let all_set = current_modes.iter().all(|mode| mode & bit != 0);
        for mode in current_modes.iter_mut() {
            if all_set {
                *mode &= !bit;
            } else {
                *mode |= bit;
            }
        }
        let common = current_modes
            .first()
            .copied()
            .filter(|first| current_modes.iter().all(|mode| mode == first));
        octal.update(cx, |input, cx| {
            input.set_text(
                common.map_or_else(String::new, |mode| format!("{mode:04o}")),
                cx,
            );
        });
        cx.notify();
    }

    fn apply_sheet(&mut self, _: &ApplySheet, _: &mut Window, cx: &mut Context<Self>) {
        if self.operation_running
            || matches!(
                self.operation_sheet,
                Some(OperationSheet::FolderProperties { .. })
            )
        {
            return;
        }
        let Some(sheet) = self.operation_sheet.take() else {
            return;
        };
        let operation = operation_from_sheet(&sheet, &self.tab.path, cx);
        match operation {
            Ok(operation) => {
                self.error = None;
                self.start_operation(operation, "Applying changes…".into(), false, cx);
            }
            Err(error) => {
                if let OperationSheet::Extract { destination, .. } = &sheet {
                    destination.update(cx, |input, cx| input.set_invalid(true, cx));
                }
                self.operation_sheet_closing = false;
                self.operation_sheet = Some(sheet);
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn start_operation(
        &mut self,
        operation: FsOperation,
        progress_message: String,
        clear_clipboard_on_success: bool,
        cx: &mut Context<Self>,
    ) {
        if self.operation_running {
            return;
        }
        self.operation_running = true;
        let cancel = Arc::new(AtomicBool::new(false));
        let (progress_tx, progress_rx) = crossbeam_channel::unbounded();
        self.operation_cancel = Some(cancel.clone());
        self.operation_progress = None;
        self.operation_progress_rx = Some(progress_rx);
        self.error = None;
        self.status_message = Some(progress_message);
        cx.notify();

        let extraction = matches!(operation, FsOperation::ExtractArchives { .. });
        let task = cx.background_executor().spawn(async move {
            OperationExecutor.execute_with_progress(&operation, &cancel, &mut |progress| {
                let _ = progress_tx.send(progress);
            })
        });
        self.poll_operation_progress(cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.operation_running = false;
                this.operation_cancel = None;
                this.operation_progress_rx = None;
                match result {
                    Ok(outcome) => {
                        if clear_clipboard_on_success {
                            this.clipboard = None;
                        }
                        let undo_available = outcome.undo.is_some();
                        if let Some(undo) = outcome.undo {
                            this.undo_stack.push(undo);
                        }
                        if extraction && !undo_available {
                            this.status_message = Some(format!(
                                "Extracted {} archive{} · too many entries to undo",
                                outcome.affected_paths.len(),
                                if outcome.affected_paths.len() == 1 {
                                    ""
                                } else {
                                    "s"
                                }
                            ));
                        } else {
                            this.status_message = Some(format!(
                                "Updated {} item{}",
                                outcome.affected_paths.len(),
                                if outcome.affected_paths.len() == 1 {
                                    ""
                                } else {
                                    "s"
                                }
                            ));
                        }
                        this.pending_reveal = outcome.affected_paths.first().cloned();
                        this.operation_progress = None;
                        this.load_directory(cx);
                    }
                    Err(error) => {
                        this.status_message = None;
                        this.operation_progress = None;
                        this.error = Some(error.to_string());
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn poll_operation_progress(&mut self, cx: &mut Context<Self>) {
        if let Some(receiver) = &self.operation_progress_rx {
            while let Ok(progress) = receiver.try_recv() {
                self.operation_progress = Some(progress);
            }
        }
        if !self.operation_running {
            return;
        }
        let timer = cx.background_executor().timer(Duration::from_millis(50));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                this.poll_operation_progress(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn copy_selected(&mut self, _: &CopySelected, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        self.set_file_clipboard(FileClipboardMode::Copy, paths, cx);
    }

    fn cut_selected(&mut self, _: &CutSelected, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        self.set_file_clipboard(FileClipboardMode::Cut, paths, cx);
    }

    fn set_file_clipboard(
        &mut self,
        mode: FileClipboardMode,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let clipboard = FileClipboard::new(mode, paths);
        match clipboard_text(&clipboard) {
            Ok(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                let action = match mode {
                    FileClipboardMode::Copy => "Copied",
                    FileClipboardMode::Cut => "Cut",
                };
                self.status_message = Some(format!(
                    "{action} {} item{}",
                    clipboard.paths.len(),
                    if clipboard.paths.len() == 1 { "" } else { "s" }
                ));
                self.clipboard = Some(clipboard);
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn copy_path_absolute(&mut self, _: &CopyPathAbsolute, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_paths_as_text(false, cx);
    }

    fn copy_path_relative(&mut self, _: &CopyPathRelative, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_paths_as_text(true, cx);
    }

    fn copy_paths_as_text(&mut self, relative: bool, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            let selected: HashSet<_> = self
                .selection
                .effective_paths(&self.snapshot.entries)
                .into_iter()
                .collect();
            let paths = self
                .trash_entries
                .iter()
                .filter(|entry| selected.contains(&entry.reference.info_path))
                .map(|entry| entry.original_path.display().to_string())
                .collect::<Vec<_>>();
            if !paths.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(paths.join("\n")));
                self.status_message = Some(format!(
                    "Copied {} original path{}",
                    paths.len(),
                    if paths.len() == 1 { "" } else { "s" }
                ));
                cx.notify();
            }
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        let result = paths
            .iter()
            .map(|path| {
                let path = if relative {
                    path.strip_prefix(&self.tab.path)
                        .unwrap_or(path)
                        .to_path_buf()
                } else {
                    std::path::absolute(path).map_err(|error| error.to_string())?
                };
                path.into_os_string().into_string().map_err(|path| {
                    format!("Path is not valid UTF-8: {}", PathBuf::from(path).display())
                })
            })
            .collect::<Result<Vec<_>, _>>();
        match result {
            Ok(paths) => {
                cx.write_to_clipboard(ClipboardItem::new_string(paths.join("\n")));
                self.status_message = Some(format!(
                    "Copied {} {}path{}",
                    paths.len(),
                    if relative { "relative " } else { "" },
                    if paths.len() == 1 { "" } else { "s" }
                ));
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn file_clipboard_from_system(&self, cx: &mut Context<Self>) -> Option<FileClipboard> {
        let system_text = cx.read_from_clipboard().and_then(|item| item.text());
        let internal = self
            .clipboard
            .clone()
            .filter(|clipboard| clipboard_text(clipboard).ok().as_ref() == system_text.as_ref());
        internal.or_else(|| system_text.as_deref().and_then(clipboard_from_text))
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let Some(clipboard) = self.file_clipboard_from_system(cx) else {
            self.status_message = Some("Clipboard contains no local files".into());
            cx.notify();
            return;
        };
        let (operation, message, clear_clipboard) = match clipboard.mode {
            FileClipboardMode::Copy => (
                FsOperation::Copy {
                    sources: clipboard.paths,
                    destination: self.tab.path.clone(),
                    conflict: ConflictDecision::KeepBoth,
                },
                "Copying…".into(),
                false,
            ),
            FileClipboardMode::Cut => (
                FsOperation::Move {
                    sources: clipboard.paths,
                    destination: self.tab.path.clone(),
                    conflict: ConflictDecision::Ask,
                },
                "Moving…".into(),
                true,
            ),
        };
        self.start_operation(operation, message, clear_clipboard, cx);
    }

    fn trash_selected(&mut self, _: &TrashSelected, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            self.purge_selected_trash(window, cx);
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        if self.operation_running {
            return;
        }
        let count = paths.len();
        let subject = selection_subject(&paths);
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Move {subject} to Trash?"),
            Some("You can restore it with Ctrl+Z while gnil-fm remains open."),
            &["Move to Trash", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.start_operation(
                    FsOperation::Trash { paths },
                    format!(
                        "Moving {count} item{} to Trash…",
                        if count == 1 { "" } else { "s" }
                    ),
                    false,
                    cx,
                );
            });
        })
        .detach();
    }

    fn delete_selected(&mut self, _: &DeleteSelected, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            self.purge_selected_trash(window, cx);
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        if self.operation_running {
            return;
        }
        let count = paths.len();
        let subject = selection_subject(&paths);
        let answer = window.prompt(
            PromptLevel::Critical,
            &format!("Permanently delete {subject}?"),
            Some("This action cannot be undone."),
            &["Delete Permanently", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.start_operation(
                    FsOperation::DeletePermanently { paths },
                    format!(
                        "Deleting {count} item{}…",
                        if count == 1 { "" } else { "s" }
                    ),
                    false,
                    cx,
                );
            });
        })
        .detach();
    }

    fn restore_trash_selected(
        &mut self,
        _: &RestoreTrashSelected,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.selected_trash_entries();
        if entries.is_empty() || self.operation_running {
            return;
        }
        let conflicts = entries
            .iter()
            .filter(|entry| entry.original_path.exists())
            .count();
        if conflicts == 0 {
            self.start_operation(
                FsOperation::RestoreTrash {
                    entries,
                    replace_existing: false,
                },
                "Restoring from Trash…".into(),
                false,
                cx,
            );
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Replace {conflicts} existing item{}?",
                if conflicts == 1 { "" } else { "s" }
            ),
            Some("Restoring will permanently replace the items currently at their original paths."),
            &["Replace and Restore", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.start_operation(
                    FsOperation::RestoreTrash {
                        entries,
                        replace_existing: true,
                    },
                    "Replacing and restoring…".into(),
                    false,
                    cx,
                );
            });
        })
        .detach();
    }

    fn purge_selected_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.selected_trash_entries();
        if entries.is_empty() || self.operation_running {
            return;
        }
        let count = entries.len();
        let answer = window.prompt(
            PromptLevel::Critical,
            &format!(
                "Permanently delete {count} trash item{}?",
                if count == 1 { "" } else { "s" }
            ),
            Some("This action cannot be undone."),
            &["Delete Permanently", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.start_operation(
                    FsOperation::PurgeTrash { entries },
                    "Deleting from Trash…".into(),
                    false,
                    cx,
                );
            });
        })
        .detach();
    }

    fn empty_trash(&mut self, _: &EmptyTrash, window: &mut Window, cx: &mut Context<Self>) {
        if self.trash_entries.is_empty() || self.operation_running {
            return;
        }
        let entries: Vec<_> = self
            .trash_entries
            .iter()
            .map(|entry| entry.reference.clone())
            .collect();
        let answer = window.prompt(
            PromptLevel::Critical,
            "Empty Trash?",
            Some("Every item in every available trash location will be permanently deleted."),
            &["Empty Trash", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.start_operation(
                    FsOperation::PurgeTrash { entries },
                    "Emptying Trash…".into(),
                    false,
                    cx,
                );
            });
        })
        .detach();
    }

    fn selected_trash_entries(&self) -> Vec<TrashEntryRef> {
        let selected: HashSet<_> = self
            .selection
            .effective_paths(&self.snapshot.entries)
            .into_iter()
            .collect();
        self.trash_entries
            .iter()
            .filter(|entry| selected.contains(&entry.reference.info_path))
            .map(|entry| entry.reference.clone())
            .collect()
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.operation_running {
            return;
        }
        let Some(record) = self.undo_stack.last().cloned() else {
            self.status_message = Some("Nothing to undo".into());
            cx.notify();
            return;
        };
        let label = record.label.clone();
        self.operation_running = true;
        self.error = None;
        self.status_message = Some(format!("Undoing {label}…"));
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { OperationExecutor.undo(&record) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.operation_running = false;
                match result {
                    Ok(()) => {
                        this.undo_stack.pop();
                        this.status_message = Some(format!("Undid {label}"));
                        this.load_directory(cx);
                    }
                    Err(error) => {
                        this.status_message = None;
                        this.error = Some(format!("Could not undo: {error}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}
