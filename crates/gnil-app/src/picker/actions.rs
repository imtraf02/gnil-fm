impl Picker {
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        if !self.allows_multiple() {
            return;
        }
        let visible = self.visible_indices(cx);
        self.selected = visible
            .iter()
            .filter_map(|index| self.snapshot.entries.get(*index))
            .filter(|entry| self.entry_selectable(entry))
            .map(|entry| entry.path.clone())
            .collect();
        cx.notify();
    }

    fn click_entry(
        &mut self,
        index: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let visible = self.visible_indices(cx);
        let Some(entry) = visible
            .get(index)
            .and_then(|entry_index| self.snapshot.entries.get(*entry_index))
            .cloned()
        else {
            return;
        };
        if !self.entry_selectable(&entry) {
            if entry_is_directory(&entry) && event.click_count() >= 2 {
                self.navigate(entry.path, cx);
            }
            return;
        }
        let modifiers = event.modifiers();
        if self.allows_multiple() && modifiers.shift {
            let last_visible = visible.len().saturating_sub(1);
            let anchor = self.anchor.unwrap_or(index).min(last_visible);
            let start = anchor.min(index);
            let end = anchor.max(index);
            if !modifiers.control && !modifiers.platform {
                self.selected.clear();
            }
            let paths: Vec<_> = visible[start..=end]
                .iter()
                .filter_map(|entry_index| self.snapshot.entries.get(*entry_index))
                .filter(|entry| self.entry_selectable(entry))
                .map(|entry| entry.path.clone())
                .collect();
            for path in paths {
                self.selected.insert(path);
            }
        } else if self.allows_multiple() && (modifiers.control || modifiers.platform) {
            if !self.selected.remove(&entry.path) {
                self.selected.insert(entry.path.clone());
            }
            self.anchor = Some(index);
        } else {
            self.selected.clear();
            self.selected.insert(entry.path.clone());
            self.anchor = Some(index);
        }

        if matches!(self.request.kind, PickerRequestKind::Save(_)) && !entry_is_directory(&entry) {
            if let Some(input) = &self.name_input {
                input.update(cx, |input, cx| input.set_text(entry.name.clone(), cx));
            }
        }
        if event.click_count() >= 2 {
            if entry_is_directory(&entry) {
                self.navigate(entry.path, cx);
            } else if matches!(self.request.kind, PickerRequestKind::Open(_))
                && self.finish_accept(cx)
            {
                window.remove_window();
            }
        } else {
            cx.notify();
        }
    }

    fn entry_selectable(&self, entry: &FileEntry) -> bool {
        match &self.request.kind {
            PickerRequestKind::Open(options) if options.directory => entry_is_directory(entry),
            PickerRequestKind::Open(_) => !entry_is_directory(entry),
            PickerRequestKind::Save(_) => true,
            PickerRequestKind::SaveMany(_) => entry_is_directory(entry),
        }
    }

    fn allows_multiple(&self) -> bool {
        matches!(&self.request.kind, PickerRequestKind::Open(options) if options.multiple)
    }

    fn accept(&mut self, _: &Accept, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_editing {
            self.submit_path(cx);
            window.focus(&self.focus_handle(cx));
            return;
        }
        if self.finish_accept(cx) {
            window.remove_window();
        }
    }

    fn finish_accept(&mut self, cx: &mut Context<Self>) -> bool {
        let paths = match &self.request.kind {
            PickerRequestKind::Open(options) if options.directory => {
                let mut paths: Vec<_> = self.selected.iter().cloned().collect();
                if paths.is_empty() {
                    paths.push(self.current_dir.clone());
                }
                paths.sort();
                paths
            }
            PickerRequestKind::Open(options) => {
                let mut paths: Vec<_> = self.selected.iter().cloned().collect();
                paths.sort();
                if !options.multiple {
                    paths.truncate(1);
                }
                if paths.is_empty() {
                    self.status = Some("Select a file to continue".into());
                    cx.notify();
                    return false;
                }
                paths
            }
            PickerRequestKind::Save(_) => {
                let Some(input) = &self.name_input else {
                    return false;
                };
                let name = input.read(cx).text().to_owned();
                if !valid_save_name(&name) {
                    input.update(cx, |input, cx| input.set_invalid(true, cx));
                    self.status = Some("Enter a valid file name without /".into());
                    cx.notify();
                    return false;
                }
                let name = self.save_name(cx).unwrap_or_else(|| OsString::from(name));
                vec![self.current_dir.join(name)]
            }
            PickerRequestKind::SaveMany(options) => {
                let directory = self
                    .selected
                    .iter()
                    .find(|path| path.is_dir())
                    .cloned()
                    .unwrap_or_else(|| self.current_dir.clone());
                options
                    .files
                    .iter()
                    .map(|name| directory.join(name))
                    .collect()
            }
        };
        self.complete(PickerOutcome::Accepted {
            paths,
            choices: self
                .choices
                .iter()
                .map(|choice| (choice.id.clone(), choice.selected.clone()))
                .collect(),
            current_filter: self.active_filter().cloned(),
        });
        true
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if self.filter_open || self.choice_open.is_some() {
            self.dismiss_picker_menus(cx);
            return;
        }
        if self.path_editing {
            self.path_editing = false;
            self.error = None;
            window.focus(&self.focus_handle(cx));
            cx.notify();
            return;
        }
        self.complete(PickerOutcome::Cancelled);
        window.remove_window();
    }

    fn dismiss_picker_menus(&mut self, cx: &mut Context<Self>) {
        if self.menu_closing || (!self.filter_open && self.choice_open.is_none()) {
            return;
        }
        if self.reduced_motion {
            self.filter_open = false;
            self.choice_open = None;
            cx.notify();
            return;
        }
        self.menu_closing = true;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(80));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.menu_closing {
                    this.filter_open = false;
                    this.choice_open = None;
                    this.menu_closing = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn complete(&mut self, outcome: PickerOutcome) {
        self.windows.borrow_mut().remove(&self.request.handle);
        if let Some(sender) = self.response.take() {
            let _ = sender.try_send(outcome);
        }
    }

    fn show_recent(&mut self, cx: &mut Context<Self>) {
        if let Some(cancelled) = self.scan_cancel.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.directory_watcher = None;
        self.watched_path = None;
        self.location = PickerLocation::Recent;
        self.generation = self.generation.wrapping_add(1);
        self.loading = true;
        self.error = None;
        self.selected.clear();
        let generation = self.generation;
        let task = cx
            .background_executor()
            .spawn(async move { recent_entries() });
        cx.spawn(async move |this, cx| {
            let entries = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.loading = false;
                    this.snapshot = Arc::new(DirectorySnapshot {
                        generation,
                        path: PathBuf::from("recent:///"),
                        entries,
                        unreadable_entries: 0,
                        phase: DirectoryLoadPhase::Complete,
                    });
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn visible_indices(&mut self, cx: &App) -> Arc<[usize]> {
        let query = self.search_input.read(cx).text().trim().to_owned();
        let key = VisibleEntriesKey {
            generation: self.snapshot.generation,
            phase: self.snapshot.phase,
            query: query.clone(),
            filter_index: self.filter_index,
        };
        if self.visible_entries_key.as_ref() == Some(&key) {
            return Arc::clone(&self.visible_indices);
        }
        let active_filter = self.active_filter().cloned();
        let mut entries: Vec<_> = self
            .snapshot
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                active_filter
                    .as_ref()
                    .is_none_or(|filter| filter.matches(&entry.path))
            })
            .filter_map(|(index, entry)| {
                if query.is_empty() {
                    Some((index, 0))
                } else {
                    fuzzy_match_score(&entry.name, &query).map(|score| (index, score))
                }
            })
            .collect();
        if !query.is_empty() {
            entries.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
        }
        self.visible_entries_key = Some(key);
        self.visible_indices = entries
            .into_iter()
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
            .into();
        Arc::clone(&self.visible_indices)
    }

}
