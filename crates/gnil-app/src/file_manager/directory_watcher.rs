#[derive(Default)]
struct PendingWatchChanges {
    upserts: HashSet<PathBuf>,
    removes: HashSet<PathBuf>,
    full_rescan: bool,
}

impl PendingWatchChanges {
    fn is_empty(&self) -> bool {
        !self.full_rescan && self.upserts.is_empty() && self.removes.is_empty()
    }
}

impl FileManager {
    fn ensure_directory_watcher(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.watched_path.as_deref() == Some(path) {
            return;
        }
        let generation = self.generation;
        let path = path.to_path_buf();
        let previous = self.directory_watcher.take();
        self.watched_path = None;
        self.watcher_refresh_at = None;
        self.pending_watch_changes = PendingWatchChanges::default();
        self.watcher_apply_running = false;
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
                    Ok((path, watcher)) if this.tab.path == path => {
                        let events = watcher.events();
                        this.directory_watcher = Some(watcher);
                        this.watched_path = Some(path);
                        Self::wait_for_directory_watcher_event(events, generation, cx);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        this.status_message =
                            Some(format!("Automatic folder refresh unavailable: {error}"));
                        this.invalidate_surfaces(SurfaceMask::STATUS, cx);
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
        cx.spawn(async move |this, cx| {
            let Ok(event) = events.recv().await else {
                return;
            };
            let next_events = events.clone();
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match event {
                    WatchEvent::Upsert(paths) => {
                        this.pending_watch_changes.upserts.extend(paths);
                    }
                    WatchEvent::Remove(paths) => {
                        this.pending_watch_changes.removes.extend(paths);
                    }
                    WatchEvent::Rename { from, to } => {
                        this.pending_watch_changes.removes.insert(from);
                        this.pending_watch_changes.upserts.insert(to);
                    }
                    WatchEvent::Rescan => this.pending_watch_changes.full_rescan = true,
                    WatchEvent::Error(error) => {
                        this.status_message = Some(format!("Folder watcher error: {error}"));
                        this.invalidate_surfaces(SurfaceMask::STATUS, cx);
                    }
                }
                if this.pending_watch_changes.is_empty() {
                    Self::wait_for_directory_watcher_event(next_events, generation, cx);
                    return;
                }
                if this.watcher_refresh_at.is_none() {
                    this.watcher_refresh_at =
                        Some(Instant::now() + Duration::from_millis(25));
                    Self::schedule_directory_watcher_batch(generation, cx);
                }
                Self::wait_for_directory_watcher_event(next_events, generation, cx);
            });
        })
        .detach();
    }

    fn schedule_directory_watcher_batch(generation: u64, cx: &mut Context<Self>) {
        let timer = cx.background_executor().timer(Duration::from_millis(25));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                let now = Instant::now();
                let refresh_ready = this
                    .watcher_refresh_at
                    .is_some_and(|refresh_at| now >= refresh_at);
                if refresh_ready && this.tab.root != TabRoot::Trash {
                    if this.loading || this.watcher_apply_running {
                        this.watcher_refresh_at = Some(now + Duration::from_millis(25));
                        Self::schedule_directory_watcher_batch(generation, cx);
                    } else {
                        this.watcher_refresh_at = None;
                        let changes = std::mem::take(&mut this.pending_watch_changes);
                        if changes.full_rescan || this.settings.hide_gitignored {
                            this.load_directory(cx);
                        } else {
                            this.apply_watcher_changes(changes, cx);
                        }
                    }
                }
            });
        })
        .detach();
    }

    fn apply_watcher_changes(
        &mut self,
        changes: PendingWatchChanges,
        cx: &mut Context<Self>,
    ) {
        let generation = self.generation;
        let directory = self.tab.path.clone();
        let show_hidden = self.tab.show_hidden;
        if changes.removes.contains(&directory) || changes.upserts.contains(&directory) {
            self.load_directory(cx);
            return;
        }
        let mut removes: HashSet<_> = changes
            .removes
            .into_iter()
            .filter(|path| path.parent() == Some(directory.as_path()))
            .collect();
        let mut upserts = Vec::new();
        for path in changes.upserts {
            if path.parent() != Some(directory.as_path()) {
                continue;
            }
            let hidden = path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'));
            if hidden && !show_hidden {
                removes.insert(path);
            } else {
                upserts.push(path);
            }
        }

        if !removes.is_empty() {
            Arc::make_mut(&mut self.snapshot)
                .entries
                .retain(|entry| !removes.contains(&entry.path));
            if self.selection.retain_existing(&self.snapshot.entries) {
                self.update_selected_path();
                self.cancel_preview_request();
                self.preview = None;
                self.preview_path = None;
            }
            self.invalidate_surfaces(
                SurfaceMask::FILE_LIST | SurfaceMask::PREVIEW | SurfaceMask::STATUS,
                cx,
            );
        }
        if upserts.is_empty() {
            return;
        }

        self.watcher_apply_running = true;
        let task = cx.background_executor().spawn(async move {
            upserts
                .into_iter()
                .map(|path| {
                    let result = scan_entry(path.clone());
                    (path, result)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let results = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation || this.tab.path != directory {
                    return;
                }
                this.watcher_apply_running = false;
                let mut full_rescan = false;
                let snapshot = Arc::make_mut(&mut this.snapshot);
                for (path, result) in results {
                    snapshot.entries.retain(|entry| entry.path != path);
                    match result {
                        Ok(entry) => snapshot.entries.push(entry),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(_) => full_rescan = true,
                    }
                }
                if full_rescan {
                    this.load_directory(cx);
                    return;
                }
                this.tab.sort.sort(&mut snapshot.entries);
                if this.selection.retain_existing(&snapshot.entries) {
                    this.update_selected_path();
                    this.cancel_preview_request();
                    this.preview = None;
                    this.preview_path = None;
                }
                this.invalidate_surfaces(
                    SurfaceMask::FILE_LIST | SurfaceMask::PREVIEW | SurfaceMask::STATUS,
                    cx,
                );
            });
        })
        .detach();
    }
}
