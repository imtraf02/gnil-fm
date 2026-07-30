impl FileManager {
    #[allow(clippy::too_many_lines)]
    fn load_directory(&mut self, cx: &mut Context<Self>) {
        let navigation_started = self.perf_trace.start();
        if let Some(cancelled) = self.scan_cancel.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.cancel_preview_request();
        self.reset_grid_previews();
        self.action_menu = None;
        self.empty_space_menu = None;
        self.command_bar_menu = None;
        if self.snapshot.path != self.tab.path {
            self.inline_rename = None;
        }
        if self.file_search.open
            && self.file_search.scope == FileSearchScope::CurrentFolder
            && self.file_search.root != self.tab.path
        {
            self.finish_file_search_close(cx);
        }
        match std::mem::take(&mut self.pointer_interaction) {
            PointerInteraction::RowArmed(press) => {
                press.payload.visual.set_lifted(false);
                press.payload.visual.set_valid_target(false);
                self.selection = press.baseline;
            }
            PointerInteraction::FileDrag(drag) => {
                drag.press.payload.visual.set_lifted(false);
                drag.press.payload.visual.set_valid_target(false);
                self.selection = drag.press.baseline;
            }
            PointerInteraction::RubberBand(state) => self.selection = state.baseline,
            PointerInteraction::Idle => {}
        }
        if self.path_input.editing {
            self.path_input.dismiss();
            self.path_input
                .input
                .update(cx, |input, cx| input.set_invalid(false, cx));
        }
        if self.tab.root == TabRoot::Trash {
            self.directory_watcher = None;
            self.watched_path = None;
            self.watcher_refresh_at = None;
            self.pending_watch_changes = PendingWatchChanges::default();
            self.watcher_apply_running = false;
            self.load_trash(cx);
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let path = self.tab.path.clone();
        let preserve_selection = self.snapshot.path == path;
        let path_id = stable_path_id(&path);
        let show_hidden = self.tab.show_hidden;
        let sort = self.tab.sort;
        let respect_gitignore = self.settings.hide_gitignored;
        let metadata_workers = self.settings.worker_threads;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.scan_cancel = Some(Arc::clone(&cancelled));
        self.loading = true;
        self.error = None;
        if !preserve_selection {
            self.selection.clear();
            self.snapshot = Arc::new(DirectorySnapshot {
                generation,
                path: path.clone(),
                entries: Vec::new(),
                unreadable_entries: 0,
                phase: DirectoryLoadPhase::Discovered,
            });
            self.file_list_scroll = UniformListScrollHandle::new();
            self.rendered_file_range = 0..0;
        }
        self.preview = None;
        self.preview_path = None;
        self.invalidate_surfaces(SurfaceMask::NAVIGATION, cx);
        self.ensure_directory_watcher(&path, cx);
        self.perf_trace
            .record_folder_navigation(generation, path_id, navigation_started);

        let (sender, receiver) = async_channel::bounded(3);
        let scan_started = self.perf_trace.start();
        let task = cx.background_executor().spawn(async move {
            let result = scan_directory_progressive(
                &path,
                ScanOptions {
                    generation,
                    show_hidden,
                    sort,
                    respect_gitignore,
                    metadata_workers,
                    include_git_status: false,
                },
                &cancelled,
                |snapshot| {
                    let elapsed = scan_started.map(|started| started.elapsed());
                    let _ = sender.send_blocking((Ok(snapshot), elapsed));
                },
            );
            let elapsed = scan_started.map(|started| started.elapsed());
            let _ = sender.send_blocking((result, elapsed));
        });
        cx.spawn(async move |this, cx| {
            while let Ok((result, scan_elapsed)) = receiver.recv().await {
                let _ = this.update(cx, |this, cx| {
                    if this.generation != generation {
                        return;
                    }
                    match result {
                        Ok(snapshot) => {
                            let complete = snapshot.phase == DirectoryLoadPhase::Complete;
                            let phase = snapshot.phase;
                            let entry_count = snapshot.entries.len();
                            let cursor_path = this
                                .selection
                                .cursor
                                .and_then(|index| this.snapshot.entries.get(index))
                                .map(|entry| entry.path.clone());
                            let properties_target_items = !this.pending_reveal.is_empty();
                            this.loading = !complete;
                            this.snapshot = Arc::new(snapshot);
                            if this.pending_reveal.is_empty() {
                                this.selection.retain_existing(&this.snapshot.entries);
                                if let Some(index) = cursor_path.and_then(|path| {
                                    this.snapshot
                                        .entries
                                        .iter()
                                        .position(|entry| entry.path == path)
                                }) {
                                    this.selection.cursor = Some(index);
                                    if this.selection.anchor.is_some() {
                                        this.selection.anchor = Some(index);
                                    }
                                }
                            } else {
                                let indices: Vec<_> = this
                                    .pending_reveal
                                    .iter()
                                    .filter_map(|path| {
                                        this.snapshot
                                            .entries
                                            .iter()
                                            .position(|entry| entry.path == *path)
                                    })
                                    .collect();
                                if let Some(first) = indices.first().copied() {
                                    let baseline = SelectionState::default();
                                    this.selection.apply_indices(
                                        &baseline,
                                        indices.iter().copied(),
                                        gnil_core::SelectionMerge::Replace,
                                        Some(first),
                                        &this.snapshot.entries,
                                    );
                                    let path = this.snapshot.entries[first].path.clone();
                                    this.tab.selected_path = Some(path);
                                    this.file_list_scroll
                                        .scroll_to_item_strict(first, ScrollStrategy::Center);
                                    this.load_preview(cx);
                                }
                                let all_found = indices.len() == this.pending_reveal.len();
                                if complete || (all_found && !this.pending_show_properties) {
                                    if complete && !all_found {
                                        this.error = Some(if indices.is_empty() {
                                            "The requested item is no longer in this folder".into()
                                        } else {
                                            "Some requested items are no longer in this folder"
                                                .into()
                                        });
                                    }
                                    this.pending_reveal.clear();
                                }
                            }
                            if complete && this.pending_show_properties {
                                this.pending_show_properties = false;
                                if !properties_target_items {
                                    this.open_folder_properties(cx);
                                } else if this.selection.selected_count() > 0 {
                                    this.open_selected_properties(cx);
                                }
                            }
                            this.perf_trace.record_folder_phase(
                                generation,
                                path_id,
                                phase,
                                scan_elapsed,
                                entry_count,
                            );
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) => {
                            this.loading = false;
                            this.pending_reveal.clear();
                            this.pending_show_properties = false;
                            this.error = Some(error.to_string());
                        }
                    }
                    this.invalidate_surfaces(
                        SurfaceMask::FILE_LIST
                            | SurfaceMask::STATUS
                            | SurfaceMask::COMMAND_BAR,
                        cx,
                    );
                });
            }
            let () = task.await;
        })
        .detach();
    }

    fn load_trash(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.loading = true;
        self.error = None;
        self.selection.clear();
        self.cancel_preview_request();
        self.preview = None;
        self.preview_path = None;
        cx.notify();
        let task = cx.background_executor().spawn(async move { scan_trash() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation || this.tab.root != TabRoot::Trash {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(trash) => {
                        let snapshot = Arc::make_mut(&mut this.snapshot);
                        snapshot.entries = trash
                            .entries
                            .iter()
                            .map(trash_entry_as_file_entry)
                            .collect();
                        snapshot.unreadable_entries = trash.unreadable_entries;
                        snapshot.phase = DirectoryLoadPhase::Complete;
                        this.trash_entries = Arc::new(trash.entries);
                        this.sort_trash_entries();
                        Arc::make_mut(&mut this.snapshot).entries = this
                            .trash_entries
                            .iter()
                            .map(trash_entry_as_file_entry)
                            .collect();
                    }
                    Err(error) => {
                        Arc::make_mut(&mut this.snapshot).entries.clear();
                        Arc::make_mut(&mut this.trash_entries).clear();
                        this.error = Some(format!("Could not read Trash: {error}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn refresh_devices(&mut self, cx: &mut Context<Self>) {
        if !self.device_refresh.request() {
            return;
        }
        Self::start_device_scan(cx);
    }

    fn start_device_scan(cx: &mut Context<Self>) {
        let task = cx.background_executor().spawn(async { scan_devices() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let scan_succeeded = result.is_ok();
                match result {
                    Ok(devices) => {
                        this.device_scan_error = None;
                        if let TabRoot::Device { id, .. } = &this.tab.root
                            && !devices
                                .iter()
                                .any(|device| &device.id == id && device.mount_path.is_some())
                        {
                            Arc::make_mut(&mut this.snapshot).entries.clear();
                            this.selection.clear();
                            this.cancel_preview_request();
                            this.preview = None;
                            this.preview_path = None;
                            this.error = Some("Location no longer available".into());
                        }
                        let auto_mount: Vec<_> = devices
                            .iter()
                            .filter(|device| {
                                this.auto_mount_removable
                                    && device.removable
                                    && device.mount_path.is_none()
                                    && !this.auto_mount_attempted.contains(&device.id)
                            })
                            .map(|device| device.id.clone())
                            .collect();
                        this.auto_mount_attempted
                            .retain(|id| devices.iter().any(|device| &device.id == id));
                        this.devices = devices;
                        for id in auto_mount {
                            this.auto_mount_attempted.insert(id.clone());
                            Self::mount_device_in_background(id, false, cx);
                        }
                    }
                    Err(error) => {
                        this.device_scan_error =
                            Some(format!("Device service unavailable · retrying ({error})"));
                    }
                }
                let refresh_again = this.device_refresh.finished(scan_succeeded, Instant::now());
                cx.notify();
                if refresh_again {
                    Self::start_device_scan(cx);
                }
            });
        })
        .detach();
    }

    fn schedule_device_monitor(cx: &mut Context<Self>) {
        let timer = cx.background_executor().timer(DEVICE_MONITOR_INTERVAL);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.device_refresh.tick(Instant::now()) {
                    Self::start_device_scan(cx);
                }
                Self::schedule_device_monitor(cx);
            });
        })
        .detach();
    }

    fn open_device(&mut self, id: String, cx: &mut Context<Self>) {
        let mount_path = self
            .devices
            .iter()
            .find(|device| device.id == id)
            .and_then(|device| device.mount_path.clone());
        if let Some(mount_path) = mount_path {
            self.tab.navigate_device(id, mount_path);
            self.load_directory(cx);
        } else {
            Self::mount_device_in_background(id, true, cx);
        }
    }

    fn navigate_path(&mut self, path: PathBuf) {
        let stays_on_device = matches!(
            &self.tab.root,
            TabRoot::Device { mount_root, .. } if path.starts_with(mount_root)
        );
        if stays_on_device {
            self.tab.navigate_within_root(path);
        } else {
            self.tab.navigate(path);
        }
    }

    fn mount_device_in_background(id: String, navigate: bool, cx: &mut Context<Self>) {
        let task_id = id.clone();
        let task = cx
            .background_executor()
            .spawn(async move { mount_device(&task_id) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(path) => {
                    if navigate {
                        this.tab.navigate_device(id, path);
                        this.load_directory(cx);
                    } else {
                        this.refresh_devices(cx);
                    }
                }
                Err(error) => {
                    this.error = Some(format!("Could not mount device: {error}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn disconnect_device(id: String, drive_id: String, eject: bool, cx: &mut Context<Self>) {
        let task = cx.background_executor().spawn(async move {
            unmount_device(&id)?;
            if eject {
                eject_device(&drive_id)?;
            }
            Ok::<_, gnil_fs::DeviceError>(())
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => this.refresh_devices(cx),
                Err(error) => {
                    this.error = Some(format!("Could not disconnect device: {error}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn select_from_click(
        &mut self,
        index: usize,
        event: &ClickEvent,
        load_preview: bool,
        cx: &mut Context<Self>,
    ) {
        self.pointer_interaction = PointerInteraction::Idle;
        if self.select_from_modifiers(index, event.modifiers()) {
            if load_preview {
                self.selection_changed(cx);
            } else {
                // A directory double-click immediately navigates away. Updating the right-side
                // preview first would start a redundant directory count that is cancelled a few
                // microseconds later.
                self.update_selected_path();
                self.invalidate_surfaces(SurfaceMask::FILE_LIST | SurfaceMask::STATUS, cx);
            }
        }
    }

    fn select_from_modifiers(&mut self, index: usize, modifiers: gpui::Modifiers) -> bool {
        if modifiers.shift && (modifiers.control || modifiers.platform) {
            self.selection
                .extend_to_additive(index, &self.snapshot.entries)
        } else if modifiers.shift {
            self.selection.extend_to(index, &self.snapshot.entries)
        } else if modifiers.control || modifiers.platform {
            self.selection.toggle(index, &self.snapshot.entries)
        } else {
            self.selection.select_only(index, &self.snapshot.entries)
        }
    }

    fn arm_row_drag(&mut self, index: usize, payload: FileDragPayload, event: &MouseDownEvent) {
        self.pointer_interaction = PointerInteraction::RowArmed(RowPressState {
            origin: event.position,
            index,
            baseline: self.selection.clone(),
            payload,
        });
    }

    fn handle_file_drag_move(
        &mut self,
        event: &DragMoveEvent<FileDragPayload>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let copy = event.event.modifiers.control || event.event.modifiers.platform;
        event.drag(cx).visual.set_valid_target(false);
        let mut became_active = None;
        let mut visual_changed = false;
        match &mut self.pointer_interaction {
            PointerInteraction::RowArmed(press)
                if movement_crossed_threshold(press.origin, event.event.position) =>
            {
                press.payload.visual.set_lifted(true);
                press.payload.visual.set_copy(copy);
                became_active = Some(press.clone());
                visual_changed = true;
            }
            PointerInteraction::FileDrag(drag) if drag.copy != copy => {
                drag.copy = copy;
                drag.press.payload.visual.set_copy(copy);
                visual_changed = true;
            }
            _ => {}
        }
        if let Some(press) = became_active {
            let pressed_is_selected = self
                .snapshot
                .entries
                .get(press.index)
                .is_some_and(|entry| press.baseline.is_highlighted(press.index, entry));
            let selection_changed = !pressed_is_selected;
            if selection_changed {
                self.selection
                    .select_only(press.index, &self.snapshot.entries);
            }
            let native_drag_started = window
                .start_external_file_drag(ExternalPaths::new(
                    press.payload.paths.iter().cloned(),
                ))
                .is_ok();
            self.pointer_interaction = PointerInteraction::FileDrag(ActiveFileDrag { press, copy });
            if !native_drag_started {
                self.status_message =
                    Some("External drag is unavailable in this Wayland session".into());
            }
            if selection_changed {
                self.selection_changed(cx);
                visual_changed = false;
            }
        }
        let cursor = if matches!(&self.pointer_interaction, PointerInteraction::FileDrag(_)) {
            if copy {
                CursorStyle::DragCopy
            } else {
                CursorStyle::OperationNotAllowed
            }
        } else {
            CursorStyle::Arrow
        };
        cx.set_active_drag_cursor_style(cursor, window);
        if visual_changed {
            cx.notify();
        }
    }

    fn handle_drag_modifiers(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let PointerInteraction::FileDrag(drag) = &mut self.pointer_interaction {
            drag.copy = event.modifiers.control || event.modifiers.platform;
            drag.press.payload.visual.set_copy(drag.copy);
            cx.set_active_drag_cursor_style(
                if !drag.press.payload.visual.valid_target() {
                    CursorStyle::OperationNotAllowed
                } else if drag.copy {
                    CursorStyle::DragCopy
                } else {
                    CursorStyle::ClosedHand
                },
                window,
            );
            cx.notify();
        }
    }

    fn finish_rubber_band(&mut self, state: RubberBandState, cx: &mut Context<Self>) {
        let changed = if state.crossed_threshold {
            if let Some(indices) = state.hit_indices {
                let cursor = if state.current_content_y >= state.origin_content_y {
                    indices.last().copied()
                } else {
                    indices.first().copied()
                };
                self.selection.apply_indices(
                    &state.baseline,
                    indices,
                    state.merge,
                    cursor,
                    &self.snapshot.entries,
                )
            } else {
            let cursor = endpoint_index(
                state.hit_span.as_ref(),
                state.origin_content_y,
                state.current_content_y,
            );
            if let Some(span) = state.hit_span {
                self.selection.apply_indices(
                    &state.baseline,
                    span,
                    state.merge,
                    cursor,
                    &self.snapshot.entries,
                )
            } else {
                self.selection.apply_indices(
                    &state.baseline,
                    std::iter::empty(),
                    state.merge,
                    None,
                    &self.snapshot.entries,
                )
            }
            }
        } else if state.merge == gnil_core::SelectionMerge::Replace {
            self.selection.clear()
        } else {
            let changed = self.selection != state.baseline;
            self.selection = state.baseline;
            changed
        };
        if changed {
            self.selection_changed(cx);
        } else {
            cx.notify();
        }
    }

    fn finish_pointer_interaction(
        &mut self,
        _event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.finish_column_resize(cx) {
            return;
        }
        self.flush_rubber_band_update(cx);
        let interaction = std::mem::take(&mut self.pointer_interaction);
        match interaction {
            PointerInteraction::Idle => {}
            PointerInteraction::RowArmed(press) => {
                press.payload.visual.set_lifted(false);
                press.payload.visual.set_valid_target(false);
                cx.stop_active_drag(window);
            }
            PointerInteraction::FileDrag(drag) => {
                drag.press.payload.visual.set_lifted(false);
                drag.press.payload.visual.set_valid_target(false);
                let changed = self.selection != drag.press.baseline;
                self.selection = drag.press.baseline;
                if changed {
                    self.selection_changed(cx);
                } else {
                    cx.notify();
                }
                cx.stop_active_drag(window);
            }
            PointerInteraction::RubberBand(state) => self.finish_rubber_band(state, cx),
        }
    }

    fn cancel_pointer_interaction(
        &mut self,
        _: &CancelPointerInteraction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(resize) = self.column_resize.take() {
            self.set_detail_column_width(resize.column, resize.initial_width);
            cx.notify();
            return;
        }
        let interaction = std::mem::take(&mut self.pointer_interaction);
        let changed = match interaction {
            PointerInteraction::RowArmed(press) => {
                press.payload.visual.set_lifted(false);
                press.payload.visual.set_valid_target(false);
                let changed = self.selection != press.baseline;
                self.selection = press.baseline;
                changed
            }
            PointerInteraction::FileDrag(drag) => {
                drag.press.payload.visual.set_lifted(false);
                drag.press.payload.visual.set_valid_target(false);
                let changed = self.selection != drag.press.baseline;
                self.selection = drag.press.baseline;
                changed
            }
            PointerInteraction::RubberBand(state) => {
                let changed = self.selection != state.baseline;
                self.selection = state.baseline;
                changed
            }
            PointerInteraction::Idle => return,
        };
        cx.stop_active_drag(window);
        if changed {
            self.selection_changed(cx);
        } else {
            cx.notify();
        }
    }

    fn can_internal_drop(&self, payload: &FileDragPayload, target: &DropTarget) -> bool {
        can_internal_drop_value(payload, target, self.operation_running)
    }

    fn external_paths(paths: &ExternalPaths) -> Vec<PathBuf> {
        sanitized_external_paths(paths)
    }

    fn can_external_drop(&self, paths: &ExternalPaths, target: &DropTarget) -> bool {
        can_external_drop_value(paths, target, self.operation_running)
    }

    fn update_internal_drop_cursor(
        &mut self,
        event: &DragMoveEvent<FileDragPayload>,
        target: &DropTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.bounds.contains(&event.event.position) {
            return;
        }
        let payload = event.drag(cx);
        if !payload.visual.lifted() {
            payload.visual.set_valid_target(false);
            cx.set_active_drag_cursor_style(CursorStyle::Arrow, window);
            return;
        }
        let valid = self.can_internal_drop(payload, target);
        payload.visual.set_valid_target(valid);
        let cursor = if valid {
            if payload.visual.copy() {
                CursorStyle::DragCopy
            } else {
                CursorStyle::ClosedHand
            }
        } else {
            CursorStyle::OperationNotAllowed
        };
        cx.set_active_drag_cursor_style(cursor, window);
    }

    fn update_external_drop_cursor(
        &mut self,
        event: &DragMoveEvent<ExternalPaths>,
        target: &DropTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.bounds.contains(&event.event.position) {
            return;
        }
        let cursor = if self.can_external_drop(event.drag(cx), target) {
            CursorStyle::DragCopy
        } else {
            CursorStyle::OperationNotAllowed
        };
        cx.set_active_drag_cursor_style(cursor, window);
    }

    fn perform_internal_drop(
        &mut self,
        payload: &FileDragPayload,
        target: DropTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_internal_drop(payload, &target) {
            return;
        }
        let Ok(intent) = internal_drop_intent(
            payload,
            &target,
            payload.visual.copy(),
            self.operation_running,
        ) else {
            return;
        };
        payload.visual.set_lifted(false);
        payload.visual.set_valid_target(false);
        self.pointer_interaction = PointerInteraction::Idle;
        cx.stop_active_drag(window);
        let operation = match (intent, target) {
            (DropIntent::Move, DropTarget::Directory(destination)) => FsOperation::Move {
                sources: payload.paths.to_vec(),
                destination,
                conflict: ConflictDecision::Ask,
            },
            (DropIntent::Copy, DropTarget::Directory(destination)) => FsOperation::Copy {
                sources: payload.paths.to_vec(),
                destination,
                conflict: ConflictDecision::KeepBoth,
            },
            (DropIntent::Trash, DropTarget::Trash) => FsOperation::Trash {
                paths: payload.paths.to_vec(),
            },
            _ => return,
        };
        let message = match intent {
            DropIntent::Move => "Moving…",
            DropIntent::Copy => "Copying…",
            DropIntent::Trash => "Moving to Trash…",
        };
        self.start_operation(operation, message.into(), false, cx);
    }

    fn perform_external_drop(
        &mut self,
        paths: &ExternalPaths,
        target: DropTarget,
        cx: &mut Context<Self>,
    ) {
        if !self.can_external_drop(paths, &target) {
            return;
        }
        let paths = Self::external_paths(paths);
        let DropTarget::Directory(destination) = target else {
            return;
        };
        self.start_operation(
            FsOperation::Copy {
                sources: paths,
                destination,
                conflict: ConflictDecision::KeepBoth,
            },
            "Copying dropped files…".into(),
            false,
            cx,
        );
    }

    fn cancel_preview_request(&mut self) {
        if let Some(cancelled) = self.preview_cancel.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.preview_request_path = None;
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.preview_debounce_generation = self.preview_debounce_generation.wrapping_add(1);
    }

    fn schedule_preview(&mut self, cx: &mut Context<Self>) {
        self.cancel_preview_request();
        let generation = self.preview_debounce_generation;
        let timer = cx.background_executor().timer(Duration::from_millis(50));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.preview_debounce_generation == generation {
                    this.load_preview(cx);
                }
            });
        })
        .detach();
    }

    fn load_preview(&mut self, cx: &mut Context<Self>) {
        self.preview_debounce_generation = self.preview_debounce_generation.wrapping_add(1);
        if !self.preview_visible {
            return;
        }

        let path = self.selection.cursor.and_then(|index| {
            if self.tab.root == TabRoot::Trash {
                self.trash_entries
                    .get(index)
                    .map(|entry| entry.reference.trashed_path.clone())
                    .or_else(|| {
                        self.snapshot
                            .entries
                            .get(index)
                            .map(|entry| entry.path.clone())
                    })
            } else {
                self.snapshot
                    .entries
                    .get(index)
                    .map(|entry| entry.path.clone())
            }
        });
        let Some(path) = path else {
            self.cancel_preview_request();
            self.preview = None;
            self.preview_path = None;
            self.invalidate_surfaces(SurfaceMask::PREVIEW, cx);
            return;
        };

        if self.preview_request_path.as_ref() == Some(&path) {
            return;
        }
        if self.preview_path.as_ref() == Some(&path) && self.preview.is_some() {
            self.cancel_preview_request();
            return;
        }

        self.cancel_preview_request();
        let generation = self.preview_generation;
        self.preview_request_path = Some(path.clone());
        self.invalidate_surfaces(SurfaceMask::PREVIEW, cx);
        let service = Arc::clone(&self.preview_service);
        let cache = Arc::clone(&self.preview_cache);
        let cancelled = Arc::new(AtomicBool::new(false));
        self.preview_cancel = Some(Arc::clone(&cancelled));
        let requested_path = path.clone();
        let task = cx.background_executor().spawn(async move {
            if let Some(preview) = cache
                .lock()
                .expect("preview cache lock poisoned")
                .get(&path)
            {
                return Ok((path, preview));
            }
            service
                .preview_cancellable(&PreviewRequest::initial(path.clone()), &cancelled)
                .map(|preview| {
                    cache
                        .lock()
                        .expect("preview cache lock poisoned")
                        .insert(&path, preview.clone());
                    (path, preview)
                })
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.preview_generation != generation
                    || this.preview_request_path.as_ref() != Some(&requested_path)
                {
                    return;
                }
                match result {
                    Ok((path, preview)) if this.preview_request_path.as_ref() == Some(&path) => {
                        this.preview_path = Some(path);
                        this.preview = Some(preview);
                        this.preview_request_path = None;
                        this.preview_cancel = None;
                        this.invalidate_surfaces(SurfaceMask::PREVIEW, cx);
                    }
                    Err(PreviewError::Cancelled) => {}
                    Err(error) => {
                        this.preview_request_path = None;
                        this.preview_cancel = None;
                        this.error = Some(error.to_string());
                        this.invalidate_surfaces(
                            SurfaceMask::FILE_LIST | SurfaceMask::PREVIEW | SurfaceMask::STATUS,
                            cx,
                        );
                    }
                    _ => {}
                }
            });
        })
        .detach();
    }

    fn open_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            if let Some(entry) = self.trash_entries.get(index)
                && let Err(error) = std::process::Command::new("xdg-open")
                    .arg(&entry.reference.trashed_path)
                    .spawn()
            {
                self.error = Some(error.to_string());
                cx.notify();
            }
            return;
        }
        let Some(entry) = self.snapshot.entries.get(index).cloned() else {
            return;
        };
        match entry.kind {
            FileKind::Directory => {
                self.tab.navigate_within_root(entry.path);
                self.load_directory(cx);
            }
            FileKind::Symlink if entry.is_directory_like() => {
                self.tab.navigate_within_root(entry.path);
                self.load_directory(cx);
            }
            _ => {
                if let Err(error) = std::process::Command::new("xdg-open")
                    .arg(&entry.path)
                    .spawn()
                {
                    self.error = Some(error.to_string());
                    cx.notify();
                }
            }
        }
    }
}
