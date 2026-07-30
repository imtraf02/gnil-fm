const FILE_SEARCH_DEBOUNCE: Duration = Duration::from_millis(120);
const FILE_SEARCH_CLOSE_DURATION: Duration = Duration::from_millis(80);
const FILE_SEARCH_RESULT_LIMIT: usize = 80;

impl FileSearchState {
    fn new(input: Entity<TextInput>, root: PathBuf, home_root: PathBuf) -> Self {
        Self {
            input,
            root,
            home_root,
            scope: FileSearchScope::CurrentFolder,
            open: false,
            closing: false,
            loading: false,
            generation: 0,
            cancel: None,
            results: Vec::new(),
            focused: None,
            error: None,
        }
    }
}

impl FileManager {
    fn activate_file_search(
        &mut self,
        _: &ActivateFileSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open_with_chooser.is_some() || self.operation_sheet.is_some() {
            return;
        }
        if self.path_input.editing {
            self.path_input.dismiss();
            self.path_input
                .input
                .update(cx, |input, cx| input.set_invalid(false, cx));
        }
        self.action_menu = None;
        self.empty_space_menu = None;
        self.command_bar_menu = None;
        self.appearance_menu_open = false;
        if self.tab.root == TabRoot::Trash {
            self.file_search.scope = FileSearchScope::Home;
        }
        self.file_search.root = match self.file_search.scope {
            FileSearchScope::CurrentFolder => self.tab.path.clone(),
            FileSearchScope::Home => self.file_search.home_root.clone(),
        };
        self.file_search.open = true;
        self.file_search.closing = false;
        self.file_search.input.update(cx, |input, cx| {
            input.select_all(cx);
            window.focus(&input.focus_handle(cx));
        });
        cx.notify();
    }

    fn select_file_search_scope(&mut self, scope: FileSearchScope, cx: &mut Context<Self>) {
        if scope == FileSearchScope::CurrentFolder && self.tab.root == TabRoot::Trash {
            return;
        }
        let root = match scope {
            FileSearchScope::CurrentFolder => self.tab.path.clone(),
            FileSearchScope::Home => self.file_search.home_root.clone(),
        };
        if self.file_search.scope == scope && self.file_search.root == root {
            return;
        }
        self.file_search.scope = scope;
        self.file_search.root = root;
        let query = self.file_search.input.read(cx).text().to_owned();
        self.update_file_search(&query, cx);
    }

    fn dismiss_file_search(
        &mut self,
        _: &DismissFileSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.file_search.open || self.file_search.closing {
            return;
        }
        if let Some(cancelled) = self.file_search.cancel.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.file_search.generation = self.file_search.generation.wrapping_add(1);
        self.file_search.loading = false;
        window.focus(&self.focus_handle(cx));
        if self.reduced_motion {
            self.finish_file_search_close(cx);
            return;
        }
        self.file_search.closing = true;
        cx.notify();
        let timer = cx
            .background_executor()
            .timer(FILE_SEARCH_CLOSE_DURATION);
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.file_search.closing {
                    this.finish_file_search_close(cx);
                }
            });
        })
        .detach();
    }

    fn finish_file_search_close(&mut self, cx: &mut Context<Self>) {
        if let Some(cancelled) = self.file_search.cancel.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.file_search.open = false;
        self.file_search.closing = false;
        self.file_search.loading = false;
        self.file_search.results.clear();
        self.file_search.focused = None;
        self.file_search.error = None;
        if !self.file_search.input.read(cx).text().is_empty() {
            self.file_search
                .input
                .update(cx, |input, cx| input.set_text("", cx));
        }
        cx.notify();
    }

    fn update_file_search(&mut self, query: &str, cx: &mut Context<Self>) {
        if let Some(cancelled) = self.file_search.cancel.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.file_search.generation = self.file_search.generation.wrapping_add(1);
        let generation = self.file_search.generation;
        self.file_search.results.clear();
        self.file_search.focused = None;
        self.file_search.error = None;
        let query = query.trim().to_owned();
        if query.is_empty() {
            self.file_search.loading = false;
            cx.notify();
            return;
        }

        let root = self.file_search.root.clone();
        let options = SearchOptions {
            show_hidden: self.tab.show_hidden,
            respect_gitignore: self.settings.hide_gitignored,
            max_results: FILE_SEARCH_RESULT_LIMIT,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        self.file_search.cancel = Some(Arc::clone(&cancelled));
        self.file_search.loading = true;
        cx.notify();

        let timer = cx.background_executor().timer(FILE_SEARCH_DEBOUNCE);
        let task = cx.background_executor().spawn(async move {
            timer.await;
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            if !root.is_dir() {
                return Some(Err("Search location is unavailable".to_owned()));
            }
            let entries = search_paths(&root, &query, options, &cancelled)
                .into_iter()
                .map(|hit| file_search_entry(hit.path))
                .collect();
            Some(Ok(entries))
        });
        cx.spawn(async move |this, cx| {
            let Some(result) = task.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if generation != this.file_search.generation
                    || !this.file_search.open
                    || this.file_search.closing
                {
                    return;
                }
                this.file_search.loading = false;
                match result {
                    Ok(results) => {
                        this.file_search.results = results;
                        this.file_search.focused =
                            (!this.file_search.results.is_empty()).then_some(0);
                    }
                    Err(error) => this.file_search.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn file_search_next(
        &mut self,
        _: &FileSearchNext,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.file_search.results.len();
        if count == 0 {
            return;
        }
        self.file_search.focused =
            Some(self.file_search.focused.map_or(0, |index| (index + 1) % count));
        cx.notify();
    }

    fn file_search_previous(
        &mut self,
        _: &FileSearchPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.file_search.results.len();
        if count == 0 {
            return;
        }
        self.file_search.focused = Some(
            self.file_search
                .focused
                .map_or(count - 1, |index| index.checked_sub(1).unwrap_or(count - 1)),
        );
        cx.notify();
    }

    fn open_file_search_result(
        &mut self,
        _: &OpenFileSearchResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self
            .file_search
            .focused
            .or_else(|| (!self.file_search.results.is_empty()).then_some(0))
        {
            self.activate_file_search_result(index, window, cx);
        }
    }

    fn activate_file_search_result(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.file_search.results.get(index).cloned() else {
            return;
        };
        self.finish_file_search_close(cx);
        window.focus(&self.focus_handle(cx));
        if entry.is_directory_like() {
            self.pending_reveal.clear();
            self.navigate_path(entry.path);
        } else if let Some(parent) = entry.path.parent() {
            self.pending_reveal = vec![entry.path.clone()];
            self.navigate_path(parent.to_path_buf());
        } else {
            self.status_message = Some("Search result is no longer available".into());
            return;
        }
        self.load_directory(cx);
    }
}

fn file_search_entry(path: PathBuf) -> FileEntry {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let symlink_metadata = std::fs::symlink_metadata(&path).ok();
    let kind = symlink_metadata.as_ref().map_or(FileKind::Other, |metadata| {
        if metadata.file_type().is_symlink() {
            FileKind::Symlink
        } else if metadata.is_dir() {
            FileKind::Directory
        } else if metadata.is_file() {
            FileKind::File
        } else {
            FileKind::Other
        }
    });
    let metadata = if kind == FileKind::Symlink {
        let target_kind = std::fs::metadata(&path).ok().map(|metadata| {
            if metadata.is_dir() {
                FileKind::Directory
            } else if metadata.is_file() {
                FileKind::File
            } else {
                FileKind::Other
            }
        });
        EntryMetadata::Ready(FileMetadata {
            symlink_target_kind: target_kind,
            ..FileMetadata::default()
        })
    } else {
        EntryMetadata::Pending
    };
    FileEntry {
        path,
        hidden: name.starts_with('.'),
        name,
        kind,
        metadata,
        git_status: None,
    }
}
