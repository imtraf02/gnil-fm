impl OpenWithChooserState {
    fn application_ids(&self, cx: &App) -> Vec<String> {
        let Some(catalog) = &self.catalog else {
            return Vec::new();
        };
        let query = self.search.read(cx).text();
        let mut seen = HashSet::new();
        let applications = catalog
            .suggested
            .iter()
            .chain(&catalog.all)
            .filter(|application| seen.insert(application.desktop_id.clone()))
            .cloned()
            .collect::<Vec<_>>();
        filter_applications(&applications, query)
            .into_iter()
            .map(|application| application.desktop_id.clone())
            .collect()
    }

    fn application(&self, desktop_id: &str) -> Option<&DesktopApplication> {
        let catalog = self.catalog.as_ref()?;
        catalog
            .suggested
            .iter()
            .chain(&catalog.all)
            .find(|application| application.desktop_id == desktop_id)
    }
}

impl FileManager {
    fn selected_open_with_target(&self) -> Option<(PathBuf, String)> {
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        let [path] = paths.as_slice() else {
            return None;
        };
        let entry = self
            .snapshot
            .entries
            .iter()
            .find(|entry| entry.path == *path)?;
        let eligible = entry.kind == FileKind::File
            || (entry.kind == FileKind::Symlink
                && entry
                    .metadata()
                    .and_then(|metadata| metadata.symlink_target_kind)
                    == Some(FileKind::File));
        eligible.then(|| {
            (
                path.clone(),
                crate::open_with::mime_type_for_path(
                    path,
                    entry.metadata().and_then(|metadata| metadata.mime.as_deref()),
                ),
            )
        })
    }

    fn show_open_with_submenu(&mut self, cx: &mut Context<Self>) {
        if self
            .action_menu
            .as_ref()
            .is_some_and(|menu| menu.open_with_submenu.is_some())
        {
            return;
        }
        let Some((path, mime_type)) = self.selected_open_with_target() else {
            return;
        };
        let Some(menu) = self.action_menu.as_mut() else {
            return;
        };
        menu.open_open_with_submenu(path.clone(), mime_type.clone());
        if let Some(catalog) = self.open_with_catalogs.get(&mime_type).cloned() {
            Self::finish_open_with_submenu(menu, &catalog);
            cx.notify();
            return;
        }

        let menu_serial = menu.serial;
        let metadata_mime = mime_type.clone();
        let task = cx
            .background_executor()
            .spawn(async move { discover_applications(&path, Some(&metadata_mime)) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(menu) = this
                    .action_menu
                    .as_mut()
                    .filter(|menu| menu.serial == menu_serial)
                else {
                    return;
                };
                let Some(submenu) = menu.open_with_submenu.as_mut() else {
                    return;
                };
                submenu.loading = false;
                Self::finish_open_with_submenu(menu, &result);
                this.open_with_catalogs
                    .insert(result.mime_type.clone(), result);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_open_with_submenu(menu: &mut ActionMenuState, catalog: &OpenWithCatalog) {
        if let Some(submenu) = menu.open_with_submenu.as_mut() {
            submenu.mime_type.clone_from(&catalog.mime_type);
            submenu.applications = catalog.suggested.iter().take(5).cloned().collect();
            submenu.loading = false;
            submenu.error = None;
            submenu.focused = Some(0);
        }
    }

    fn open_with_selected(
        &mut self,
        _: &OpenWithSelected,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((path, mime_type)) = self.selected_open_with_target() else {
            return;
        };
        self.open_open_with_chooser(path, mime_type, window, cx);
    }

    fn open_open_with_chooser(
        &mut self,
        path: PathBuf,
        mime_type: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.action_menu = None;
        self.empty_space_menu = None;
        self.open_with_serial = self.open_with_serial.wrapping_add(1);
        let serial = self.open_with_serial;
        let file_name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        let search = cx.new(|cx| {
            TextInput::new("Search applications", "", cx)
                .with_key_context("OpenWithInput")
                .with_leading_icon("icons/action-search.svg")
        });
        let search_subscription =
            cx.subscribe(&search, |this, _, event: &TextInputEvent, cx| {
                if matches!(event, TextInputEvent::Changed) {
                    let ids = this
                        .open_with_chooser
                        .as_ref()
                        .map_or_else(Vec::new, |chooser| chooser.application_ids(cx));
                    if let Some(chooser) = this.open_with_chooser.as_mut() {
                        chooser.selected_desktop_id = ids.into_iter().next();
                    }
                    cx.notify();
                }
            });
        let catalog = self.open_with_catalogs.get(&mime_type).cloned();
        let selected_desktop_id = catalog.as_ref().and_then(|catalog| {
            catalog
                .suggested
                .first()
                .or_else(|| catalog.all.first())
                .map(|application| application.desktop_id.clone())
        });
        self.open_with_chooser = Some(OpenWithChooserState {
            path: path.clone(),
            file_name,
            mime_type: mime_type.clone(),
            catalog,
            search: search.clone(),
            _search_subscription: search_subscription,
            selected_desktop_id,
            always_use: false,
            loading: !self.open_with_catalogs.contains_key(&mime_type),
            error: None,
            motion: OverlayMotionState::Opening,
            serial,
        });
        search.update(cx, |search, cx| {
            window.focus(&search.focus_handle(cx));
        });

        if self.open_with_catalogs.contains_key(&mime_type) {
            cx.notify();
            return;
        }
        let metadata_mime = mime_type;
        let task = cx
            .background_executor()
            .spawn(async move { discover_applications(&path, Some(&metadata_mime)) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(chooser) = this
                    .open_with_chooser
                    .as_mut()
                    .filter(|chooser| chooser.serial == serial)
                else {
                    return;
                };
                chooser.loading = false;
                chooser.mime_type.clone_from(&result.mime_type);
                chooser.selected_desktop_id = result
                    .suggested
                    .first()
                    .or_else(|| result.all.first())
                    .map(|application| application.desktop_id.clone());
                chooser.catalog = Some(result.clone());
                this.open_with_catalogs
                    .insert(result.mime_type.clone(), result);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn dismiss_open_with(
        &mut self,
        _: &DismissOpenWith,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chooser) = self.open_with_chooser.as_mut() else {
            return;
        };
        if chooser.motion == OverlayMotionState::Closing {
            return;
        }
        chooser.motion = OverlayMotionState::Closing;
        let serial = chooser.serial;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(80));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.open_with_chooser.as_ref().is_some_and(|chooser| {
                    chooser.serial == serial && chooser.motion == OverlayMotionState::Closing
                }) {
                    this.open_with_chooser = None;
                    cx.notify();
                }
            });
        })
        .detach();
        window.focus(&self.focus_handle(cx));
    }

    fn confirm_open_with(
        &mut self,
        _: &ConfirmOpenWith,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let application = self.open_with_chooser.as_ref().and_then(|chooser| {
            chooser
                .selected_desktop_id
                .as_deref()
                .and_then(|id| chooser.application(id))
                .cloned()
        });
        if let Some(application) = application {
            self.launch_with_application(application, true, window, cx);
        }
    }

    fn launch_with_application(
        &mut self,
        application: DesktopApplication,
        from_chooser: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (path, always_use, mime_type) = if from_chooser {
            let Some(chooser) = self.open_with_chooser.as_ref() else {
                return;
            };
            (
                chooser.path.clone(),
                chooser.always_use,
                chooser.mime_type.clone(),
            )
        } else {
            let Some(menu) = self.action_menu.as_ref() else {
                return;
            };
            let Some(submenu) = menu.open_with_submenu.as_ref() else {
                return;
            };
            (submenu.path.clone(), false, submenu.mime_type.clone())
        };
        match launch_application(&application, &path) {
            Ok(()) => {
                self.status_message = Some(format!("Opened with {}", application.name));
                if from_chooser {
                    self.dismiss_open_with(&DismissOpenWith, window, cx);
                } else {
                    self.action_menu = None;
                }
                if always_use && mime_type != "application/octet-stream" {
                    self.open_with_catalogs.remove(&mime_type);
                    let app_name = application.name.clone();
                    let task = cx.background_executor().spawn(async move {
                        set_default_application(&application, &mime_type)
                    });
                    cx.spawn(async move |this, cx| {
                        let result = task.await;
                        let _ = this.update(cx, |this, cx| {
                            match result {
                                Ok(()) => {
                                    this.status_message =
                                        Some(format!("{app_name} is now the default app"));
                                }
                                Err(error) => this.error = Some(error),
                            }
                            cx.notify();
                        });
                    })
                    .detach();
                }
                cx.notify();
            }
            Err(error) => {
                if let Some(chooser) = self.open_with_chooser.as_mut() {
                    chooser.error = Some(error);
                } else {
                    self.error = Some(error);
                }
                cx.notify();
            }
        }
    }

    fn open_with_next(
        &mut self,
        _: &OpenWithNext,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_open_with_selection(1, cx);
    }

    fn open_with_previous(
        &mut self,
        _: &OpenWithPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_open_with_selection(-1, cx);
    }

    fn move_open_with_selection(&mut self, direction: isize, cx: &mut Context<Self>) {
        let Some(chooser) = self.open_with_chooser.as_ref() else {
            return;
        };
        let applications = chooser.application_ids(cx);
        if applications.is_empty() {
            return;
        }
        let current = chooser
            .selected_desktop_id
            .as_ref()
            .and_then(|selected| applications.iter().position(|id| id == selected));
        let index = match (current, direction.is_negative()) {
            (Some(index), false) => (index + 1) % applications.len(),
            (Some(0) | None, true) => applications.len() - 1,
            (Some(index), true) => index - 1,
            (None, false) => 0,
        };
        if let Some(chooser) = self.open_with_chooser.as_mut() {
            chooser.selected_desktop_id = Some(applications[index].clone());
            cx.notify();
        }
    }

    fn toggle_open_with_default(
        &mut self,
        _: &ToggleOpenWithDefault,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(chooser) = self
            .open_with_chooser
            .as_mut()
            .filter(|chooser| chooser.mime_type != "application/octet-stream")
        {
            chooser.always_use = !chooser.always_use;
            cx.notify();
        }
    }
}
