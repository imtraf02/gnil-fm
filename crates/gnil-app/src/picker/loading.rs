impl Picker {
    fn new(
        request: PickerRequest,
        response: async_channel::Sender<PickerOutcome>,
        windows: OpenWindows,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        load_picker_theme(window);
        let settings = ConfigPaths::discover().load_settings().unwrap_or_default();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let (suggested_dir, suggested_name, filters, active_filter, choices) =
            request_initial_state(&request, &home);
        let (current_dir, status) = accessible_directory(&suggested_dir)
            .map_or_else(|message| (home.clone(), Some(message)), |path| (path, None));
        let path_input = cx.new(|cx| {
            TextInput::new("Enter a path", current_dir.display().to_string(), cx)
                .with_key_context("PathInput")
        });
        let search_input = cx.new(|cx| TextInput::new("Search this folder", "", cx));
        let initial_name_display = suggested_name.as_ref().map(|name| name.display.clone());
        let initial_raw_name = suggested_name.as_ref().and_then(|name| name.raw.clone());
        let name_input = suggested_name.map(|name| {
            cx.new(|cx| TextInput::new("File name", name.display, cx).with_key_context("TextInput"))
        });
        let mut subscriptions = Vec::new();
        for input in [&path_input, &search_input]
            .into_iter()
            .chain(name_input.iter())
        {
            subscriptions.push(cx.subscribe(input, |_, _, _: &TextInputEvent, cx| cx.notify()));
        }
        let filter_index = active_filter
            .as_ref()
            .and_then(|active| filters.iter().position(|filter| filter == active));
        Self {
            focus: cx.focus_handle(),
            request,
            response: Some(response),
            windows,
            current_dir: current_dir.clone(),
            back: Vec::new(),
            forward: Vec::new(),
            location: PickerLocation::Directory,
            snapshot: Arc::new(DirectorySnapshot {
                generation: 0,
                path: current_dir,
                entries: Vec::new(),
                unreadable_entries: 0,
                phase: DirectoryLoadPhase::Complete,
            }),
            selected: HashSet::new(),
            anchor: None,
            loading: false,
            error: None,
            status,
            generation: 0,
            show_hidden: settings.show_hidden,
            path_editing: false,
            path_input,
            search_input,
            name_input,
            initial_name_display,
            initial_raw_name,
            _subscriptions: subscriptions,
            devices: Vec::new(),
            device_refresh: DeviceRefresh::new(),
            device_scan_error: None,
            filters,
            filter_index,
            filter_open: false,
            choices,
            choice_open: None,
            menu_closing: false,
            reduced_motion: settings.reduced_motion,
            respect_gitignore: settings.hide_gitignored,
            metadata_workers: settings.worker_threads,
            scan_cancel: None,
            visible_entries_key: None,
            visible_indices: Arc::from([]),
            directory_watcher: None,
            watched_path: None,
        }
    }

    fn active_filter(&self) -> Option<&PortalFilter> {
        self.filter_index.and_then(|index| self.filters.get(index))
    }

    fn load_directory(&mut self, cx: &mut Context<Self>) {
        if let Some(cancelled) = self.scan_cancel.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.location = PickerLocation::Directory;
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let path = self.current_dir.clone();
        let show_hidden = self.show_hidden;
        let respect_gitignore = self.respect_gitignore;
        let metadata_workers = self.metadata_workers;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.scan_cancel = Some(Arc::clone(&cancelled));
        self.loading = true;
        self.error = None;
        self.selected.clear();
        if self.snapshot.path != path {
            self.snapshot = Arc::new(DirectorySnapshot {
                generation,
                path: path.clone(),
                entries: Vec::new(),
                unreadable_entries: 0,
                phase: DirectoryLoadPhase::Discovered,
            });
        }
        cx.notify();
        self.ensure_directory_watcher(&path, cx);
        let (sender, receiver) = async_channel::bounded(3);
        let task = cx.background_executor().spawn(async move {
            let result = scan_directory_progressive(
                &path,
                ScanOptions {
                    generation,
                    show_hidden,
                    sort: SortSpec::default(),
                    respect_gitignore,
                    metadata_workers,
                    include_git_status: false,
                },
                &cancelled,
                |snapshot| {
                    let _ = sender.send_blocking(Ok(snapshot));
                },
            );
            let _ = sender.send_blocking(result);
        });
        cx.spawn(async move |this, cx| {
            while let Ok(result) = receiver.recv().await {
                let _ = this.update(cx, |this, cx| {
                    if this.generation != generation {
                        return;
                    }
                    match result {
                        Ok(snapshot) => {
                            this.loading = snapshot.phase != DirectoryLoadPhase::Complete;
                            this.snapshot = Arc::new(snapshot);
                            this.selected.retain(|path| {
                                this.snapshot
                                    .entries
                                    .iter()
                                    .any(|entry| &entry.path == path)
                            });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) => {
                            this.loading = false;
                            Arc::make_mut(&mut this.snapshot).entries.clear();
                            this.error = Some(permission_message(&this.current_dir, &error));
                        }
                    }
                    cx.notify();
                });
            }
            let () = task.await;
        })
        .detach();
    }

    fn ensure_directory_watcher(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.watched_path.as_deref() == Some(path) {
            return;
        }
        let generation = self.generation;
        let path = path.to_path_buf();
        let previous = self.directory_watcher.take();
        self.watched_path = None;
        let task = cx.background_executor().spawn(async move {
            drop(previous);
            DirectoryWatcher::watch(&path).map(|watcher| (path, watcher))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok((path, watcher)) if this.current_dir == path => {
                        let events = watcher.events();
                        this.directory_watcher = Some(watcher);
                        this.watched_path = Some(path);
                        Self::wait_for_directory_watcher_event(events, generation, cx);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        this.status = Some(format!("Automatic refresh unavailable: {error}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn wait_for_directory_watcher_event(
        events: async_channel::Receiver<WatchEvent>,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let timer = cx.background_executor().timer(Duration::from_millis(25));
        cx.spawn(async move |this, cx| {
            let Ok(first_event) = events.recv().await else {
                return;
            };
            timer.await;
            let mut batch = vec![first_event];
            while let Ok(event) = events.try_recv() {
                batch.push(event);
            }
            let next_events = events.clone();
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                let mut changed = false;
                for event in batch {
                    match event {
                        WatchEvent::Upsert(_)
                        | WatchEvent::Remove(_)
                        | WatchEvent::Rename { .. }
                        | WatchEvent::Rescan => changed = true,
                        WatchEvent::Error(error) => {
                            this.status = Some(format!("Folder watcher error: {error}"));
                            cx.notify();
                        }
                    }
                }
                if changed && this.location == PickerLocation::Directory {
                    this.load_directory(cx);
                }
                Self::wait_for_directory_watcher_event(next_events, this.generation, cx);
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
                        this.devices = devices;
                        this.device_scan_error = None;
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

    fn open_device(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(device) = self.devices.get(index).cloned() else {
            return;
        };
        if let Some(path) = device.mount_path {
            self.navigate(path, cx);
            return;
        }
        self.status = Some(format!("Mounting {}…", device.label));
        let task = cx
            .background_executor()
            .spawn(async move { mount_device(&device.id) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(path) => this.navigate(path, cx),
                Err(error) => {
                    this.status = None;
                    this.error = Some(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn navigate(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if path != self.current_dir {
            self.back.push(self.current_dir.clone());
            self.forward.clear();
            self.current_dir = path;
        }
        self.status = None;
        self.load_directory(cx);
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.back.pop() else { return };
        self.forward.push(self.current_dir.clone());
        self.current_dir = path;
        self.load_directory(cx);
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.forward.pop() else {
            return;
        };
        self.back.push(self.current_dir.clone());
        self.current_dir = path;
        self.load_directory(cx);
    }

    fn go_up(&mut self, _: &GoUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(parent) = self.current_dir.parent().map(Path::to_path_buf) {
            self.navigate(parent, cx);
        }
    }

    fn activate_path(&mut self, _: &ActivatePath, window: &mut Window, cx: &mut Context<Self>) {
        self.path_editing = true;
        let value = self.current_dir.display().to_string();
        self.path_input.update(cx, |input, cx| {
            input.set_text(value, cx);
            input.set_invalid(false, cx);
            input.select_all(cx);
            window.focus(&input.focus_handle(cx));
        });
        cx.notify();
    }

    fn complete_path(&mut self, _: &CompletePath, window: &mut Window, cx: &mut Context<Self>) {
        if !self.path_editing {
            return;
        }
        let text = self.path_input.read(cx).text().to_owned();
        match completion_candidates(
            &text,
            &self.current_dir,
            dirs::home_dir().as_deref(),
            self.show_hidden,
        ) {
            Ok(candidates) if !candidates.is_empty() => {
                let replacement = candidates[0].input.clone();
                self.path_input.update(cx, |input, cx| {
                    input.set_text(replacement, cx);
                    window.focus(&input.focus_handle(cx));
                });
            }
            Ok(_) => self.status = Some("No matching folder".into()),
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn submit_path(&mut self, cx: &mut Context<Self>) {
        let text = self.path_input.read(cx).text().trim().to_owned();
        let result = resolve_path_input(&text, &self.current_dir, dirs::home_dir().as_deref())
            .and_then(validate_path);
        match result {
            Ok(PathTarget::Directory(path)) => {
                self.path_editing = false;
                self.navigate(path, cx);
            }
            Ok(PathTarget::File { path, parent }) => match &self.request.kind {
                PickerRequestKind::Open(options) if options.directory => {
                    self.path_input
                        .update(cx, |input, cx| input.set_invalid(true, cx));
                    self.error = Some("Select a folder, not a regular file".into());
                    cx.notify();
                }
                PickerRequestKind::Open(_)
                    if self
                        .active_filter()
                        .is_some_and(|filter| !filter.matches(&path)) =>
                {
                    self.path_input
                        .update(cx, |input, cx| input.set_invalid(true, cx));
                    self.error = Some("This file does not match the active filter".into());
                    cx.notify();
                }
                PickerRequestKind::Open(_) => {
                    self.path_editing = false;
                    self.navigate(parent, cx);
                    self.selected.insert(path);
                }
                PickerRequestKind::Save(_) => {
                    let raw_name = path.file_name().map(OsStr::to_owned);
                    let display_name = raw_name
                        .as_deref()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.initial_name_display = Some(display_name.clone());
                    self.initial_raw_name = raw_name;
                    if let Some(input) = &self.name_input {
                        input.update(cx, |input, cx| input.set_text(display_name, cx));
                    }
                    self.path_editing = false;
                    self.navigate(parent, cx);
                }
                PickerRequestKind::SaveMany(_) => {
                    self.path_input
                        .update(cx, |input, cx| input.set_invalid(true, cx));
                    self.error = Some("Choose a destination folder".into());
                    cx.notify();
                }
            },
            Err(error) => {
                self.path_input
                    .update(cx, |input, cx| input.set_invalid(true, cx));
                self.error = Some(error);
                cx.notify();
            }
        }
    }

}
