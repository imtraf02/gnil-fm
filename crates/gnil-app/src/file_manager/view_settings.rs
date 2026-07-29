impl PreferencesWindow {
    #[allow(clippy::too_many_lines)]
    fn render_settings_workspace(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let category = self.settings_category;

        let sidebar = div()
            .w(px(232.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .p_4()
            .border_r_1()
            .border_color(rgb(border()))
            .bg(rgb(surface_elevated()))
            .child(
                div()
                    .px_2()
                    .pb_2()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(text_muted()))
                    .child("CATEGORIES"),
            )
            .child(Self::settings_nav_item(
                "Appearance",
                "icons/settings-appearance.svg",
                SettingsCategory::Appearance,
                category,
                cx,
            ))
            .child(Self::settings_nav_item(
                "Keymap & Controls",
                "icons/settings-keymap.svg",
                SettingsCategory::Keymap,
                category,
                cx,
            ))
            .child(Self::settings_nav_item(
                "File View & Git",
                "icons/settings-file-view.svg",
                SettingsCategory::FileView,
                category,
                cx,
            ))
            .child(Self::settings_nav_item(
                "Performance & Storage",
                "icons/settings-performance.svg",
                SettingsCategory::Performance,
                category,
                cx,
            ));

        let query = self.settings_search.read(cx).text().trim().to_ascii_lowercase();
        let content = if query.is_empty() {
            match category {
                SettingsCategory::Appearance => self.render_settings_appearance(cx),
                SettingsCategory::Keymap => self.render_settings_keymap(cx),
                SettingsCategory::FileView => self.render_settings_file_view(cx),
                SettingsCategory::Performance => self.render_settings_performance(cx),
            }
        } else {
            Self::render_settings_search_results(&query, cx)
        };

        let workspace = div()
            .id("settings-workspace")
            .size_full()
            .relative()
            .bg(rgb(background()))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(58.0))
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(border()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("dismiss-settings")
                                    .with_focus_ring()
                                    .size_7()
                                    .rounded_md()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .text_color(rgb(text_muted()))
                                    .hover(|style| {
                                        style
                                            .bg(rgb(border()))
                                            .text_color(rgb(text_emphasized()))
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.dismiss_settings(&DismissSettings, window, cx);
                                    }))
                                    .child(ui_icon(
                                        "icons/action-back.svg",
                                        IconSize::Small,
                                        IconTone::Muted,
                                    )),
                            )
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_sm()
                                    .child("Settings"),
                            )
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(rgb(surface_elevated()))
                                    .border_1()
                                    .border_color(rgb(border()))
                                    .text_xs()
                                    .text_color(rgb(text_muted()))
                                    .child("Ctrl+,"),
                            ),
                    )
                    .child(
                        div()
                            .w(px(300.0))
                            .child(self.settings_search.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child("Preferences"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(sidebar)
                    .child(
                        div()
                            .id("settings-content-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_6()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(900.0))
                                    .flex()
                                    .flex_col()
                                    .child(content),
                            ),
                    ),
            );
        let keymap_edit_sheet = self.render_keymap_edit_sheet(cx);
        let keymap_conflict_sheet = self.render_keymap_conflict_sheet(cx);

        div()
            .size_full()
            .relative()
            .child(workspace)
            .child(keymap_edit_sheet)
            .child(keymap_conflict_sheet)
            .into_any_element()
    }

    fn settings_nav_item(
        label: &'static str,
        icon: &'static str,
        cat: SettingsCategory,
        active_cat: SettingsCategory,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let active = cat == active_cat;
        div()
            .id(SharedString::from(format!("settings-nav-{label}")))
            .h_8()
            .px_3()
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .cursor_pointer()
            .when(active, |row| {
                row.bg(rgb(accent_background()))
                    .text_color(rgb(text_emphasized()))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
            })
            .when(!active, |row| {
                row.text_color(rgb(theme_text()))
                    .hover(|style| style.bg(rgb(border())))
            })
            .with_focus_ring()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.settings_category = cat;
                this.settings_search
                    .update(cx, |input, cx| input.set_text("", cx));
                cx.notify();
            }))
            .child(ui_icon(
                icon,
                IconSize::Small,
                if active {
                    IconTone::Accent
                } else {
                    IconTone::Muted
                },
            ))
            .child(label)
    }

    fn render_settings_search_results(query: &str, cx: &mut Context<Self>) -> AnyElement {
        const SETTINGS: &[(SettingsCategory, &str, &str)] = &[
            (SettingsCategory::Appearance, "Theme Mode", "Light, Dark, or System"),
            (SettingsCategory::Appearance, "Reduced Motion", "Minimize interface animation"),
            (SettingsCategory::Keymap, "Keymap & Controls", "Search and customize shortcuts"),
            (SettingsCategory::FileView, "Show Hidden Files", "Display hidden entries"),
            (SettingsCategory::FileView, "Hide Gitignored Files", "Filter ignored files"),
            (SettingsCategory::FileView, "Preview Panel", "Show file previews"),
            (SettingsCategory::FileView, "Auto-mount Removable Drives", "Mount external drives"),
            (SettingsCategory::Performance, "Worker Threads", "Background scan concurrency"),
            (SettingsCategory::Performance, "Preview Cache", "Memory used for previews"),
        ];
        let matches = SETTINGS
            .iter()
            .copied()
            .filter(|(_, label, description)| {
                format!("{label} {description}").to_ascii_lowercase().contains(query)
            })
            .collect::<Vec<_>>();
        let empty = matches.is_empty();
        let rows = matches.into_iter().enumerate().map(|(index, (category, label, description))| {
            div()
                .id(("settings-search-result", index))
                .h(px(52.0))
                .px_3()
                .rounded_md()
                .border_1()
                .border_color(rgb(border()))
                .bg(rgb(surface_elevated()))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|row| row.bg(rgb(border())))
                .with_focus_ring()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings_category = category;
                    this.settings_search.update(cx, |input, cx| input.set_text("", cx));
                    cx.notify();
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(label))
                        .child(div().text_xs().text_color(rgb(text_muted())).child(description)),
                )
        });
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(setting_section_header("Search Preferences", "Matching settings and controls"))
            .when(empty, |view| {
                view.child(
                    div()
                        .p_5()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(border()))
                        .text_sm()
                        .text_color(rgb(text_muted()))
                        .child("No preferences match this search."),
                )
            })
            .children(rows)
            .into_any_element()
    }

    fn render_settings_appearance(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let current_mode = self.settings.theme;
        let reduced = self.settings.reduced_motion;

        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(setting_section_header(
                "Appearance",
                "Customize themes and visual effects",
            ))
            .child(setting_row(
                "Theme Mode",
                "Choose between Light, Dark, or System preference",
                div()
                    .flex()
                    .gap_2()
                    .child(
                        setting_chip("Light", current_mode == ThemeMode::Light).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.settings.theme = ThemeMode::Light;
                                this.save_and_apply_settings(cx);
                            }),
                        ),
                    )
                    .child(
                        setting_chip("Dark", current_mode == ThemeMode::Dark).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.settings.theme = ThemeMode::Dark;
                                this.save_and_apply_settings(cx);
                            }),
                        ),
                    )
                    .child(
                        setting_chip("System", current_mode == ThemeMode::System).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.settings.theme = ThemeMode::System;
                                this.save_and_apply_settings(cx);
                            }),
                        ),
                    ),
            ))
            .child(setting_row(
                "Reduced Motion",
                "Minimize animations for interface transitions",
                div()
                    .id("toggle-reduced-motion")
                    .with_focus_ring()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.reduced_motion = !this.settings.reduced_motion;
                        this.save_and_apply_settings(cx);
                    }))
                    .child(setting_switch(reduced)),
            ))
            .into_any_element()
    }

    fn render_settings_keymap(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.render_keymap_editor(cx)
    }

    fn render_settings_file_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let show_hidden = self.settings.show_hidden;
        let hide_gitignored = self.settings.hide_gitignored;
        let preview = self.settings.preview_enabled;
        let auto_mount = self.settings.auto_mount_removable;

        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(setting_section_header(
                "File View",
                "Configure file browser behavior and previews",
            ))
            .child(setting_row(
                "Show Hidden Files",
                "Display dotfiles and hidden system entries",
                div()
                    .id("toggle-show-hidden")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.show_hidden = !this.settings.show_hidden;
                        this.save_and_apply_settings(cx);
                    }))
                    .child(setting_switch(show_hidden)),
            ))
            .child(setting_row(
                "Hide Gitignored Files",
                "Filter out entries ignored by .gitignore rules",
                div()
                    .id("toggle-hide-gitignored")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.hide_gitignored = !this.settings.hide_gitignored;
                        this.save_and_apply_settings(cx);
                    }))
                    .child(setting_switch(hide_gitignored)),
            ))
            .child(setting_row(
                "Preview Panel",
                "Enable side preview panel by default",
                div()
                    .id("toggle-preview-panel")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.preview_enabled = !this.settings.preview_enabled;
                        this.save_and_apply_settings(cx);
                    }))
                    .child(setting_switch(preview)),
            ))
            .child(setting_row(
                "Auto-mount Removable Drives",
                "Automatically mount external drives and USB devices",
                div()
                    .id("toggle-auto-mount")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.auto_mount_removable = !this.settings.auto_mount_removable;
                        this.save_and_apply_settings(cx);
                    }))
                    .child(setting_switch(auto_mount)),
            ))
            .into_any_element()
    }

    #[allow(clippy::too_many_lines)]
    fn render_settings_performance(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let workers = self.settings.worker_threads;
        let mem_cache = self.settings.memory_cache_mib;
        let config_path = self.config_paths.config.display().to_string();

        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(setting_section_header(
                "Performance & Storage",
                "Configure background threads, caching, and config paths",
            ))
            .child(setting_row(
                "Worker Threads",
                "Number of parallel background threads for file scanning",
                div()
                    .flex()
                    .gap_2()
                    .child(setting_chip("2", workers == 2).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.settings.worker_threads = 2;
                            this.save_and_apply_settings(cx);
                        },
                    )))
                    .child(setting_chip("4", workers == 4).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.settings.worker_threads = 4;
                            this.save_and_apply_settings(cx);
                        },
                    )))
                    .child(setting_chip("8", workers == 8).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.settings.worker_threads = 8;
                            this.save_and_apply_settings(cx);
                        },
                    )))
                    .child(setting_chip("16", workers == 16).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.settings.worker_threads = 16;
                            this.save_and_apply_settings(cx);
                        },
                    ))),
            ))
            .child(setting_row(
                "Preview Cache (MiB)",
                "Maximum memory retained for text, image, and metadata previews",
                div()
                    .flex()
                    .gap_2()
                    .child(setting_chip("64 MB", mem_cache == 64).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.settings.memory_cache_mib = 64;
                            this.save_and_apply_settings(cx);
                        },
                    )))
                    .child(
                        setting_chip("128 MB", mem_cache == 128).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.settings.memory_cache_mib = 128;
                                this.save_and_apply_settings(cx);
                            },
                        )),
                    )
                    .child(
                        setting_chip("256 MB", mem_cache == 256).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.settings.memory_cache_mib = 256;
                                this.save_and_apply_settings(cx);
                            },
                        )),
                    )
                    .child(
                        setting_chip("512 MB", mem_cache == 512).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.settings.memory_cache_mib = 512;
                                this.save_and_apply_settings(cx);
                            },
                        )),
                    ),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(field_label("CONFIGURATION PATH"))
                    .child(
                        div()
                            .p_3()
                            .rounded_lg()
                            .bg(rgb(surface_elevated()))
                            .border_1()
                            .border_color(rgb(border()))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(text_muted()))
                                    .child(config_path),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn new(preferences: &Entity<PreferencesState>, cx: &mut Context<Self>) -> Self {
        let preferences = preferences.clone();
        let (settings, config_paths, keymap_overrides, keymap_error) = {
            let snapshot = preferences.read(cx);
            (
                snapshot.settings.clone(),
                snapshot.config_paths.clone(),
                snapshot.keymap.clone(),
                snapshot.keymap_error.clone(),
            )
        };
        let settings_search = cx.new(|cx| {
            TextInput::new("Search preferences", "", cx)
                .with_leading_icon("icons/action-search.svg")
        });
        let settings_search_subscription = cx.subscribe(&settings_search, |_, _, _, cx| cx.notify());
        let (keymap_search, keymap_search_subscription) = Self::create_keymap_search(cx);
        let mut window = Self {
            focus_handle: cx.focus_handle(),
            preferences: preferences.clone(),
            settings,
            config_paths,
            keymap_overrides,
            keymap_error,
            settings_category: SettingsCategory::Appearance,
            settings_search,
            _settings_search_subscription: settings_search_subscription,
            keymap_search,
            _keymap_search_subscription: keymap_search_subscription,
            keymap_capture_subscription: None,
            keymap_edit: None,
            keymap_conflict: None,
            preferences_subscription: None,
            reduced_motion: false,
        };
        window.reduced_motion = window.settings.reduced_motion;
        window.keymap_capture_subscription = Some(Self::install_keymap_capture(cx));
        window.preferences_subscription = Some(cx.observe(&preferences, |this, state, cx| {
            let state = state.read(cx);
            this.settings = state.settings.clone();
            this.keymap_overrides = state.keymap.clone();
            this.keymap_error.clone_from(&state.keymap_error);
            this.reduced_motion = this.settings.reduced_motion;
            cx.notify();
        }));
        window
    }

    fn save_and_apply_settings(&mut self, cx: &mut Context<Self>) {
        self.settings.normalize();
        let settings = self.settings.clone();
        let result = self.preferences.update(cx, |preferences, cx| {
            let result = preferences.save_settings(settings);
            cx.notify();
            result
        });
        if let Err(error) = result {
            self.keymap_error = Some(format!("Could not save settings: {error}"));
        }
        self.reduced_motion = self.settings.reduced_motion;
        cx.notify();
    }

    fn dismiss_settings(
        &mut self,
        _: &DismissSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.keymap_edit.is_some() || self.keymap_conflict.is_some() {
            self.cancel_keymap_edit(cx);
            return;
        }
        cx.global_mut::<PreferencesRegistry>().window = None;
        self.keymap_edit = None;
        self.keymap_conflict = None;
        window.remove_window();
    }

    fn focus_settings_search(
        &mut self,
        _: &FocusSettingsSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.settings_search.read(cx).focus_handle(cx));
    }
}

impl Focusable for PreferencesWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PreferencesWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("preferences-window-root")
            .size_full()
            .key_context("PreferencesWindow")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::dismiss_settings))
            .on_action(cx.listener(Self::focus_settings_search))
            .child(self.render_settings_workspace(cx))
    }
}
