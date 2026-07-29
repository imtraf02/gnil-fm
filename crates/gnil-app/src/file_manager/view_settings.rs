impl FileManager {
    #[allow(clippy::too_many_lines)]
    fn render_settings_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let category = self.settings_category;

        let sidebar = div()
            .w(px(210.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
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

        let content = match category {
            SettingsCategory::Appearance => self.render_settings_appearance(cx),
            SettingsCategory::Keymap => self.render_settings_keymap(cx),
            SettingsCategory::FileView => self.render_settings_file_view(cx),
            SettingsCategory::Performance => self.render_settings_performance(cx),
        };

        let dialog_box = div()
            .w(px(760.0))
            .max_w(px(840.0))
            .h(px(540.0))
            .rounded_lg()
            .border_1()
            .border_color(rgb(border()))
            .bg(rgb(surface()))
            .shadow_lg()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .h(px(54.0))
                    .px_5()
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
                            .id("dismiss-settings")
                            .with_focus_ring()
                            .size_8()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss_settings(&DismissSettings, window, cx);
                            }))
                            .child(ui_icon(
                                "icons/action-close.svg",
                                IconSize::Small,
                                IconTone::Muted,
                            )),
                    ),
            )
            .child(
                div().flex_1().min_h_0().flex().child(sidebar).child(
                    div()
                        .id("settings-content-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_6()
                        .child(content),
                    ),
            );
        let dialog_box = animate_overlay(
            dialog_box,
            if self.settings_closing {
                OverlayMotionState::Closing
            } else {
                OverlayMotionState::Opening
            },
            self.reduced_motion,
            3.0,
        );

        deferred(
            div()
                .id("settings-modal-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .bg(Hsla::from(rgb(background())).opacity(0.65))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.dismiss_settings(&DismissSettings, window, cx);
                    }),
                )
                .child(
                    div()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .child(dialog_box),
                ),
        )
        .with_priority(MODAL_PRIORITY)
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
        let profile = self.settings.keymap;

        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(setting_section_header(
                "Keymap & Controls",
                "Choose keyboard profile and review shortcuts",
            ))
            .child(setting_row(
                "Keymap Profile",
                "Desktop (standard shortcuts) or Yazi (vi modal navigation)",
                div()
                    .flex()
                    .gap_2()
                    .child(
                        setting_chip("Desktop", profile == KeymapProfile::Desktop).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.settings.keymap = KeymapProfile::Desktop;
                                this.save_and_apply_settings(cx);
                            }),
                        ),
                    )
                    .child(
                        setting_chip("Yazi (Vi)", profile == KeymapProfile::Yazi).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.settings.keymap = KeymapProfile::Yazi;
                                this.save_and_apply_settings(cx);
                            }),
                        ),
                    ),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(field_label("SHORTCUT REFERENCE"))
                    .child(
                        div()
                            .p_3()
                            .rounded_lg()
                            .bg(rgb(surface_elevated()))
                            .border_1()
                            .border_color(rgb(border()))
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .text_xs()
                            .child(shortcut_row("Open Settings", "Ctrl+,"))
                            .child(shortcut_row("Toggle Hidden Files", "Ctrl+H"))
                            .child(shortcut_row("Toggle Favorite", "Ctrl+D"))
                            .child(shortcut_row("Toggle Preview Panel", "Ctrl+P / F3"))
                            .child(shortcut_row("Move to Trash", "Del / d"))
                            .child(shortcut_row("Undo Operation", "Ctrl+Z / u"))
                            .child(shortcut_row("Copy Absolute Path", "Ctrl+Shift+C")),
                    ),
            )
            .into_any_element()
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
                        this.load_directory(cx);
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
                        this.load_directory(cx);
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
}
