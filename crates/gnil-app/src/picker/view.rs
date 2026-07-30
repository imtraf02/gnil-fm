impl Picker {
    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let places = picker_places();
        let current = self.current_dir.clone();
        let mut sidebar = div()
            .id("picker-sidebar")
            .w(px(190.0))
            .h_full()
            .min_h_0()
            .flex_none()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(rgb(border()))
            .p_3()
            .flex()
            .flex_col()
            .gap_1()
            .child(section_label("PLACES"));
        for (index, (label, path)) in places.into_iter().enumerate() {
            let active = self.location == PickerLocation::Directory && current == path;
            let icon = match label.as_str() {
                "Home" => "icons/place-home.svg",
                "Downloads" => "icons/place-downloads.svg",
                "Pictures" => "icons/place-pictures.svg",
                "Documents" => "icons/place-documents.svg",
                "Videos" => "icons/place-videos.svg",
                "Music" => "icons/place-music.svg",
                "Desktop" => "icons/place-desktop.svg",
                _ => "icons/folder-closed.svg",
            };
            sidebar = sidebar.child(
                sidebar_item_with_icon(label.clone(), icon, active)
                    .id(("picker-place", index))
                    .with_focus_ring()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.navigate(path.clone(), cx);
                    })),
            );
        }
        if matches!(self.request.kind, PickerRequestKind::Open(_)) {
            sidebar = sidebar.child(
                sidebar_item("Recent", self.location == PickerLocation::Recent)
                    .id("picker-recent")
                    .with_focus_ring()
                    .on_click(cx.listener(|this, _, _, cx| this.show_recent(cx))),
            );
        }
        if !self.favorites.is_empty() {
            sidebar = sidebar.child(section_label("FAVORITES").mt_4());
        }
        for (index, path) in self.favorites.iter().cloned().enumerate() {
            sidebar = sidebar.child(self.render_picker_favorite(index, path, cx));
        }
        sidebar = sidebar.child(section_label("DEVICES").mt_4());
        for (index, device) in self.devices.iter().enumerate() {
            let icon = if device.removable {
                "icons/device-usb.svg"
            } else {
                "icons/device-drive.svg"
            };
            sidebar = sidebar.child(
                sidebar_item_with_icon(device.label.clone(), icon, false)
                    .id(("picker-device", index))
                    .with_focus_ring()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_device(index, cx);
                    })),
            );
        }
        if self.devices.is_empty() {
            let message = if self.device_refresh.is_loading() {
                "Looking for devices…"
            } else if self.device_scan_error.is_some() {
                "Device service unavailable · retrying"
            } else {
                "No storage volumes"
            };
            sidebar = sidebar.child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child(message),
            );
        } else if self.device_scan_error.is_some() {
            sidebar = sidebar.child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child("Device service unavailable · retrying"),
            );
        }
        sidebar.into_any_element()
    }

    fn render_picker_favorite(
        &self,
        index: usize,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.location == PickerLocation::Directory && self.current_dir == path;
        let label = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        sidebar_item_with_icon(label, "icons/folder-favorite.svg", active)
            .id(("picker-favorite", stable_picker_path_id(&path)))
            .with_focus_ring()
            .when(index == 0, |row| {
                row.debug_selector(|| "picker-favorite-0".into())
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.navigate(path.clone(), cx);
            }))
            .into_any_element()
    }

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let path = self.current_dir.display().to_string();
        let path_field: AnyElement = if self.path_editing {
            div()
                .h_9()
                .flex_1()
                .min_w_0()
                .child(self.path_input.clone())
                .into_any_element()
        } else {
            div()
                .id("picker-breadcrumb")
                .h_9()
                .flex_1()
                .min_w_0()
                .px_3()
                .rounded_md()
                .border_1()
                .border_color(rgb(border()))
                .bg(rgb(surface_elevated()))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|style| style.border_color(rgb(border_focused())))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.activate_path(&ActivatePath, window, cx);
                }))
                .child(div().flex_1().min_w_0().truncate().child(path))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(text_muted()))
                        .child("Ctrl+L"),
                )
                .into_any_element()
        };
        div()
            .h(px(58.0))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(rgb(border()))
            .child(
                nav_button("icons/action-back.svg", !self.back.is_empty())
                    .id("picker-nav-back")
                    .when(!self.back.is_empty(), FocusableControl::with_focus_ring)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_back(&GoBack, window, cx);
                    })),
            )
            .child(
                nav_button("icons/action-forward.svg", !self.forward.is_empty())
                    .id("picker-nav-forward")
                    .when(!self.forward.is_empty(), FocusableControl::with_focus_ring)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_forward(&GoForward, window, cx);
                    })),
            )
            .child(
                nav_button(
                    "icons/action-up.svg",
                    self.current_dir.parent().is_some(),
                )
                    .id("picker-nav-up")
                    .when(
                        self.current_dir.parent().is_some(),
                        FocusableControl::with_focus_ring,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_up(&GoUp, window, cx);
                    })),
            )
            .child(path_field)
            .child(div().w(px(220.0)).h_9().child(self.search_input.clone()))
            .into_any_element()
    }

    fn render_file_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let visible = self.visible_indices(cx);
        let snapshot = Arc::clone(&self.snapshot);
        let selected = self.selected.clone();
        let directory_only =
            matches!(&self.request.kind, PickerRequestKind::Open(options) if options.directory);
        if visible.is_empty() {
            let title = if self.loading {
                "Opening folder…"
            } else if self.error.is_some() {
                "This location is unavailable"
            } else {
                "Nothing matches here"
            };
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .mt_6()
                .gap_3()
                .child(img("icons/empty-state.svg").size(px(220.0)))
                .child(div().text_sm().text_color(rgb(text())).child(title))
                .into_any_element();
        }
        uniform_list(
            "picker-file-list",
            visible.len(),
            cx.processor(move |_this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let entry_index = visible[index];
                        let entry = snapshot.entries[entry_index].clone();
                        let highlighted = selected.contains(&entry.path);
                        let disabled = directory_only && !entry_is_directory(&entry);
                        let is_directory = entry_is_directory(&entry);
                        let icon = file_icon_asset(&entry);
                        let name = entry.name.clone();
                        let detail = picker_entry_detail(&entry, is_directory);
                        let row = div()
                            .id(("picker-entry", index))
                            .when(index == 0, |row| {
                                row.debug_selector(|| "picker-entry-0".into())
                            })
                            .when(!disabled, FocusableControl::with_focus_ring)
                            .h_9()
                            .w_full()
                            .px_3()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .when(highlighted, |row| row.bg(rgb(accent_background())))
                            .when(!highlighted && !disabled, |row| {
                                row.hover(|style| style.bg(rgb(surface_elevated())))
                            })
                            .text_color(if disabled {
                                rgb(text_muted())
                            } else {
                                rgb(text())
                            })
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                                this.click_entry(index, event, window, cx);
                            }))
                            .child(ui_icon(
                                icon,
                                IconSize::Compact,
                                if highlighted {
                                    IconTone::Emphasized
                                } else {
                                    IconTone::Default
                                },
                            ))
                            .child(div().flex_1().min_w_0().truncate().child(name))
                            .child(
                                div()
                                    .w(px(88.0))
                                    .text_right()
                                    .text_xs()
                                    .text_color(rgb(text_muted()))
                                    .child(detail),
                            );
                        div().w_full().px_2().py(px(1.0)).child(row)
                    })
                    .collect()
            }),
        )
        .flex_1()
        .py_2()
        .into_any_element()
    }

    fn render_filter(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.filters.is_empty() {
            return None;
        }
        let label = self
            .active_filter()
            .map_or("All files", |filter| filter.label.as_str())
            .to_owned();
        Some(
            div()
                .relative()
                .child(
                    secondary_button(label)
                        .id("picker-filter-trigger")
                        .with_focus_ring()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.filter_open {
                                this.dismiss_picker_menus(cx);
                                return;
                            }
                            this.filter_open = true;
                            this.choice_open = None;
                            this.menu_closing = false;
                            cx.notify();
                        })),
                )
                .when(self.filter_open, |root| {
                    root.child(self.render_filter_menu(cx))
                })
                .into_any_element(),
        )
    }

    fn render_filter_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.filters
            .iter()
            .enumerate()
            .fold(
                div()
                    .id("picker-filter-menu")
                    .w(px(240.0))
                    .p_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(border_focused()))
                    .bg(rgb(surface()))
                    .shadow_lg()
                    .occlude()
                    .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.dismiss_picker_menus(cx);
                    })),
                |menu, (index, filter)| {
                    let label = filter.label.clone();
                    menu.child(
                        div()
                            .id(("picker-filter", index))
                            .with_focus_ring()
                            .h_8()
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .text_xs()
                            .when(self.filter_index == Some(index), |row| {
                                row.bg(rgb(accent_background()))
                            })
                            .hover(|row| row.bg(rgb(surface_elevated())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.filter_index = Some(index);
                                this.selected.clear();
                                this.dismiss_picker_menus(cx);
                            }))
                            .child(label),
                    )
                },
            );
        let panel = animate_overlay(
            panel,
            if self.menu_closing {
                OverlayMotionState::Closing
            } else {
                OverlayMotionState::Opening
            },
            self.reduced_motion,
            3.0,
        );
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Local)
                .position(point(px(0.0), px(-6.0)))
                .anchor(Corner::BottomLeft)
                .snap_to_window()
                .child(panel),
        )
        .with_priority(MENU_PRIORITY)
        .into_any_element()
    }

    fn render_choices(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut row = div().flex().flex_wrap().items_center().gap_2();
        for index in 0..self.choices.len() {
            let choice = self.choices[index].clone();
            if choice.is_boolean() {
                let enabled = choice.selected == "true";
                row = row.child(
                    secondary_button(format!(
                        "{}: {}",
                        choice.label,
                        if enabled { "On" } else { "Off" }
                    ))
                    .id(("picker-boolean-choice", index))
                    .with_focus_ring()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.choices[index].selected =
                            if enabled { "false" } else { "true" }.into();
                        cx.notify();
                    })),
                );
            } else {
                let selected_label = choice
                    .options
                    .iter()
                    .find(|(id, _)| *id == choice.selected)
                    .map_or(choice.selected.clone(), |(_, label)| label.clone());
                row = row.child(
                    div()
                        .relative()
                        .child(
                            secondary_button(format!("{}: {}", choice.label, selected_label))
                                .id(("picker-choice-trigger", index))
                                .with_focus_ring()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if this.choice_open == Some(index) {
                                        this.dismiss_picker_menus(cx);
                                        return;
                                    }
                                    this.choice_open = Some(index);
                                    this.filter_open = false;
                                    this.menu_closing = false;
                                    cx.notify();
                                })),
                        )
                        .when(self.choice_open == Some(index), |root| {
                            root.child(self.render_choice_menu(index, cx))
                        }),
                );
            }
        }
        row.into_any_element()
    }

    fn render_choice_menu(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let panel = self.choices[index]
            .options
            .iter()
            .cloned()
            .enumerate()
            .fold(
                div()
                    .id(("picker-choice-menu", index))
                    .w(px(240.0))
                    .p_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(border_focused()))
                    .bg(rgb(surface()))
                    .shadow_lg()
                    .occlude()
                    .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.dismiss_picker_menus(cx);
                    })),
                |menu, (option_index, (id, label))| {
                    let selected = self.choices[index].selected == id;
                    menu.child(
                        div()
                            .id(("picker-choice", option_index))
                            .with_focus_ring()
                            .h_8()
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .text_xs()
                            .when(selected, |row| row.bg(rgb(accent_background())))
                            .hover(|row| row.bg(rgb(surface_elevated())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.choices[index].selected.clone_from(&id);
                                this.dismiss_picker_menus(cx);
                            }))
                            .child(label),
                    )
                },
            );
        let panel = animate_overlay(
            panel,
            if self.menu_closing {
                OverlayMotionState::Closing
            } else {
                OverlayMotionState::Opening
            },
            self.reduced_motion,
            3.0,
        );
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Local)
                .position(point(px(0.0), px(-6.0)))
                .anchor(Corner::BottomLeft)
                .snap_to_window()
                .child(panel),
        )
        .with_priority(MENU_PRIORITY)
        .into_any_element()
    }

    fn accept_label(&self) -> String {
        let common = match &self.request.kind {
            PickerRequestKind::Open(options) => &options.common,
            PickerRequestKind::Save(options) => &options.common,
            PickerRequestKind::SaveMany(options) => &options.common,
        };
        let default = match &self.request.kind {
            PickerRequestKind::Open(options) if options.directory => "Select Folder",
            PickerRequestKind::Open(_) => "Open",
            PickerRequestKind::Save(_) => "Save",
            PickerRequestKind::SaveMany(_) => "Select Folder",
        };
        let mut label = common
            .accept_label
            .as_deref()
            .unwrap_or(default)
            .replace('_', "");
        if self.allows_multiple() && self.selected.len() > 1 {
            label.push_str(" (");
            label.push_str(&self.selected.len().to_string());
            label.push(')');
        }
        label
    }

    fn can_accept(&self, cx: &App) -> bool {
        match &self.request.kind {
            PickerRequestKind::Open(options) if options.directory => true,
            PickerRequestKind::Open(_) => !self.selected.is_empty(),
            PickerRequestKind::Save(_) => self
                .name_input
                .as_ref()
                .is_some_and(|input| valid_save_name(input.read(cx).text())),
            PickerRequestKind::SaveMany(_) => true,
        }
    }

    fn save_name(&self, cx: &App) -> Option<OsString> {
        let display = self.name_input.as_ref()?.read(cx).text().to_owned();
        if self.initial_name_display.as_deref() == Some(display.as_str()) {
            self.initial_raw_name
                .clone()
                .or_else(|| Some(OsString::from(display)))
        } else {
            Some(OsString::from(display))
        }
    }

    fn conflict_warning(&self, cx: &App) -> Option<String> {
        match &self.request.kind {
            PickerRequestKind::Save(_) => {
                let name = self.name_input.as_ref()?.read(cx).text().to_owned();
                valid_save_name(&name)
                    .then(|| {
                        self.current_dir.join(
                            self.save_name(cx)
                                .unwrap_or_else(|| OsString::from(name)),
                        )
                    })
                    .filter(|path| path.exists())
                    .map(|_| {
                        "A file with this name already exists; the app will decide whether to replace it."
                            .into()
                    })
            }
            PickerRequestKind::SaveMany(options) => {
                let directory = self
                    .selected
                    .iter()
                    .find(|path| path.is_dir())
                    .unwrap_or(&self.current_dir);
                let count = options
                    .files
                    .iter()
                    .filter(|name| directory.join(name).exists())
                    .count();
                (count > 0).then(|| format!("{count} destination name(s) already exist."))
            }
            PickerRequestKind::Open(_) => None,
        }
    }
}

fn picker_entry_detail(entry: &FileEntry, is_directory: bool) -> String {
    if is_directory {
        return entry
            .metadata()
            .and_then(|metadata| metadata.child_count)
            .map_or_else(
                || "Folder".into(),
                |count| {
                    if count == 1 {
                        "1 item".to_owned()
                    } else {
                        format!("{count} items")
                    }
                },
            );
    }
    entry
        .metadata()
        .map_or_else(|| "—".into(), |metadata| size_label(metadata.len))
}
