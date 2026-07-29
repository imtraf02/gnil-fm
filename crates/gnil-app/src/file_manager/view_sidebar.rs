impl FileManager {
    // Sidebar groups stay together so their spacing and hierarchy remain visually auditable.
    #[allow(clippy::too_many_lines)]
    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let operation_running = self.operation_running;
        let trash_target = DropTarget::Trash;
        let trash_predicate_target = trash_target.clone();
        let trash_move_target = trash_target.clone();
        let trash_drop_target = trash_target;
        div()
            .w(px(218.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(surface()))
            .border_r_1()
            .border_color(rgb(border()))
            .child(
                div()
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child("QUICK ACCESS"),
            )
            .children(
                self.places
                    .iter()
                    .enumerate()
                    .map(|(index, (label, path))| {
                        let path = path.clone();
                        let navigate_path = path.clone();
                        let drop_target = DropTarget::Directory(path.clone());
                        let predicate_target = drop_target.clone();
                        let internal_move_target = drop_target.clone();
                        let external_move_target = drop_target.clone();
                        let internal_drop_target = drop_target.clone();
                        let external_drop_target = drop_target;
                        let active = self.tab.root == TabRoot::Directory
                            && place_is_active(&self.tab.path, &path);
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
                        div()
                            .id(("place", index))
                            .with_focus_ring()
                            .mx_2()
                            .h(px(34.0))
                            .px_3()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_sm()
                            .cursor_pointer()
                            .when(active, |style| {
                                style
                                    .bg(rgb(accent_background()))
                                    .text_color(rgb(text_emphasized()))
                            })
                            .when(!active, |style| style.text_color(rgb(theme_text())))
                            .hover(|style| style.bg(rgb(border())))
                            .can_drop(move |value, _, _| {
                                can_drop_value(value, &predicate_target, operation_running)
                            })
                            .drag_over::<FileDragPayload>(|style, _, _, _| {
                                style
                                    .bg(rgb(accent_background()))
                                    .border_1()
                                    .border_color(rgb(accent()))
                            })
                            .drag_over::<ExternalPaths>(|style, _, _, _| {
                                style
                                    .bg(rgb(accent_background()))
                                    .border_1()
                                    .border_color(rgb(accent()))
                            })
                            .on_drag_move::<FileDragPayload>(cx.listener(
                                move |this, event, window, cx| {
                                    this.update_internal_drop_cursor(
                                        event,
                                        &internal_move_target,
                                        window,
                                        cx,
                                    );
                                },
                            ))
                            .on_drag_move::<ExternalPaths>(cx.listener(
                                move |this, event, window, cx| {
                                    this.update_external_drop_cursor(
                                        event,
                                        &external_move_target,
                                        window,
                                        cx,
                                    );
                                },
                            ))
                            .on_drop(cx.listener(
                                move |this, payload: &FileDragPayload, window, cx| {
                                    this.perform_internal_drop(
                                        payload,
                                        internal_drop_target.clone(),
                                        window,
                                        cx,
                                    );
                                },
                            ))
                            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                                this.perform_external_drop(paths, external_drop_target.clone(), cx);
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.tab.navigate(navigate_path.clone());
                                this.load_directory(cx);
                            }))
                            .child(ui_icon(
                                icon,
                                IconSize::Compact,
                                if active {
                                    IconTone::Accent
                                } else {
                                    IconTone::Muted
                                },
                            ))
                            .child(label.clone())
                    }),
            )
            .child(sidebar_section_label("FAVORITES"))
            .when(self.settings.favorites.is_empty(), |sidebar| {
                sidebar.child(
                    div()
                        .mx_3()
                        .py_1()
                        .text_xs()
                        .text_color(rgb(text_muted()))
                        .child("Star a folder to add it"),
                )
            })
            .children(
                self.settings
                    .favorites
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(index, path)| {
                        let exists = self
                            .favorite_availability
                            .get(&path)
                            .copied()
                            .unwrap_or(false);
                        let active =
                            exists && self.tab.root != TabRoot::Trash && self.tab.path == path;
                        let label = path.file_name().map_or_else(
                            || path.display().to_string(),
                            |name| name.to_string_lossy().into_owned(),
                        );
                        let click_path = path.clone();
                        let drag_payload = FavoriteDragPayload {
                            path: path.clone(),
                            label: label.clone(),
                        };
                        div()
                            .id(("favorite", stable_path_id(&path)))
                            .with_focus_ring()
                            .mx_2()
                            .h(px(34.0))
                            .px_3()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_3()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(rgb(if exists {
                                theme_text()
                            } else {
                                text_muted()
                            }))
                            .when(active, |row| {
                                row.bg(rgb(accent_background()))
                                    .text_color(rgb(text_emphasized()))
                            })
                            .hover(|style| style.bg(rgb(border())))
                            .can_drop(|value, _, _| {
                                value.downcast_ref::<FavoriteDragPayload>().is_some()
                            })
                            .drag_over::<FavoriteDragPayload>(|style, _, _, _| {
                                style
                                    .bg(rgb(accent_background()))
                                    .border_1()
                                    .border_color(rgb(accent()))
                            })
                            .on_drop(cx.listener(
                                move |this, payload: &FavoriteDragPayload, _, cx| {
                                    this.reorder_favorite_path(&payload.path, index, cx);
                                },
                            ))
                            .on_drag(
                                drag_payload,
                                |payload, cursor_offset, _, cx| {
                                    cx.new(|_| FavoriteDragGhost {
                                        label: payload.label.clone(),
                                        cursor_offset,
                                    })
                                },
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_favorite(click_path.clone(), window, cx);
                            }))
                            .child(ui_icon(
                                if exists {
                                    "icons/folder-favorite.svg"
                                } else {
                                    "icons/status-warning.svg"
                                },
                                IconSize::Compact,
                                if !exists {
                                    IconTone::Warning
                                } else if active {
                                    IconTone::Accent
                                } else {
                                    IconTone::Muted
                                },
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(label),
                            )
                    }),
            )
            .child(sidebar_section_label("DEVICES"))
            .children(self.devices.iter().enumerate().map(|(index, device)| {
                let id = device.id.clone();
                let disconnect_id = device.id.clone();
                let drive_id = device.drive_id.clone();
                let eject = device.can_eject;
                let mounted = device.mount_path.is_some();
                let mount_target = device.mount_path.clone().map(DropTarget::Directory);
                let active =
                    matches!(&self.tab.root, TabRoot::Device { id, .. } if id == &device.id);
                let usage = device_usage(device);
                div()
                    .id(("device", index))
                    .with_focus_ring()
                    .mx_2()
                    .min_h(px(46.0))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .when(active, |row| row.bg(rgb(accent_background())))
                    .hover(|style| style.bg(rgb(border())))
                    .when_some(mount_target, |row, drop_target| {
                        let predicate_target = drop_target.clone();
                        let internal_move_target = drop_target.clone();
                        let external_move_target = drop_target.clone();
                        let internal_drop_target = drop_target.clone();
                        let external_drop_target = drop_target;
                        row.can_drop(move |value, _, _| {
                            can_drop_value(value, &predicate_target, operation_running)
                        })
                        .drag_over::<FileDragPayload>(|style, _, _, _| {
                            style
                                .bg(rgb(accent_background()))
                                .border_1()
                                .border_color(rgb(accent()))
                        })
                        .drag_over::<ExternalPaths>(|style, _, _, _| {
                            style
                                .bg(rgb(accent_background()))
                                .border_1()
                                .border_color(rgb(accent()))
                        })
                        .on_drag_move::<FileDragPayload>(cx.listener(
                            move |this, event, window, cx| {
                                this.update_internal_drop_cursor(
                                    event,
                                    &internal_move_target,
                                    window,
                                    cx,
                                );
                            },
                        ))
                        .on_drag_move::<ExternalPaths>(cx.listener(
                            move |this, event, window, cx| {
                                this.update_external_drop_cursor(
                                    event,
                                    &external_move_target,
                                    window,
                                    cx,
                                );
                            },
                        ))
                        .on_drop(
                            cx.listener(move |this, payload: &FileDragPayload, window, cx| {
                                this.perform_internal_drop(
                                    payload,
                                    internal_drop_target.clone(),
                                    window,
                                    cx,
                                );
                            }),
                        )
                        .on_drop(cx.listener(
                            move |this, paths: &ExternalPaths, _, cx| {
                                this.perform_external_drop(paths, external_drop_target.clone(), cx);
                            },
                        ))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_device(id.clone(), cx);
                    }))
                    .child(ui_icon(
                        device_icon(device.kind),
                        IconSize::Compact,
                        if active {
                            IconTone::Accent
                        } else {
                            IconTone::Default
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(rgb(theme_text()))
                                    .child(device.label.clone()),
                            )
                            .child(
                                div()
                                    .h(px(3.0))
                                    .w_full()
                                    .rounded_full()
                                    .bg(rgb(border_focused()))
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(usage))
                                            .rounded_full()
                                            .bg(rgb(accent())),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(9.0))
                                    .text_color(rgb(text_muted()))
                                    .child(device_capacity_label(device)),
                            ),
                    )
                    .when(mounted, |row| {
                        row.child(
                            div()
                                .id(("disconnect-device", index))
                                .with_focus_ring()
                                .size_7()
                                .rounded_md()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|style| style.bg(rgb(border_focused())))
                                .on_click(cx.listener(move |_this, _, _, cx| {
                                    cx.stop_propagation();
                                    Self::disconnect_device(
                                        disconnect_id.clone(),
                                        drive_id.clone(),
                                        eject,
                                        cx,
                                    );
                                }))
                                .child(ui_icon(
                                    "icons/device-eject.svg",
                                    IconSize::Small,
                                    IconTone::Muted,
                                )),
                        )
                    })
            }))
            .when(self.devices.is_empty(), |sidebar| {
                sidebar.child(
                    div()
                        .mx_3()
                        .py_1()
                        .text_xs()
                        .text_color(rgb(border_focused()))
                        .child(if self.device_refresh.is_loading() {
                            "Looking for devices…"
                        } else if self.device_scan_error.is_some() {
                            "Device service unavailable · retrying"
                        } else {
                            "No storage volumes"
                        }),
                )
            })
            .when(
                !self.devices.is_empty() && self.device_scan_error.is_some(),
                |sidebar| {
                    sidebar.child(
                        div()
                            .mx_3()
                            .py_1()
                            .text_xs()
                            .text_color(rgb(border_focused()))
                            .child("Device service unavailable · retrying"),
                    )
                },
            )
            .child(
                div()
                    .id("trash-place")
                    .with_focus_ring()
                    .mx_2()
                    .mt_2()
                    .h(px(34.0))
                    .px_3()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_3()
                    .cursor_pointer()
                    .text_sm()
                    .when(self.tab.root == TabRoot::Trash, |row| {
                        row.bg(rgb(accent_background()))
                            .text_color(rgb(text_emphasized()))
                    })
                    .when(self.tab.root != TabRoot::Trash, |row| {
                        row.text_color(rgb(theme_text()))
                    })
                    .hover(|style| style.bg(rgb(border())))
                    .can_drop(move |value, _, _| {
                        value
                            .downcast_ref::<FileDragPayload>()
                            .is_some_and(|payload| {
                                can_internal_drop_value(
                                    payload,
                                    &trash_predicate_target,
                                    operation_running,
                                )
                            })
                    })
                    .drag_over::<FileDragPayload>(|style, _, _, _| {
                        style
                            .bg(rgb(accent_background()))
                            .border_1()
                            .border_color(rgb(accent()))
                    })
                    .on_drag_move::<FileDragPayload>(cx.listener(move |this, event, window, cx| {
                        this.update_internal_drop_cursor(event, &trash_move_target, window, cx);
                    }))
                    .on_drop(
                        cx.listener(move |this, payload: &FileDragPayload, window, cx| {
                            this.perform_internal_drop(
                                payload,
                                trash_drop_target.clone(),
                                window,
                                cx,
                            );
                        }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab.navigate_trash();
                        this.load_directory(cx);
                    }))
                    .child(ui_icon(
                        "icons/trash.svg",
                        IconSize::Compact,
                        if self.tab.root == TabRoot::Trash {
                            IconTone::Accent
                        } else {
                            IconTone::Default
                        },
                    ))
                    .child("Trash"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("open-settings-button")
                    .with_focus_ring()
                    .mx_2()
                    .mb_3()
                    .h(px(34.0))
                    .px_3()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(rgb(theme_text()))
                    .hover(|style| style.bg(rgb(border())))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_settings(&ToggleSettings, window, cx);
                    }))
                    .child(div().flex().items_center().gap_2().child("⚙ Settings"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child("Ctrl+,"),
                    ),
            )
            .into_any_element()
    }

}
