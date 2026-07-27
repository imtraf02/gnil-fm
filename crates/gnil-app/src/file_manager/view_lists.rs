impl FileManager {
    fn cached_row_drag_payload(
        &mut self,
        snapshot: &Arc<DirectorySnapshot>,
        entry: &FileEntry,
        selected_paths: &Arc<[PathBuf]>,
        highlighted: bool,
    ) -> FileDragPayload {
        if highlighted {
            let revision = self.selection.selected_revision();
            if revision != self.drag_payload_cache.selected_revision
                || self.generation != self.drag_payload_cache.selected_generation
            {
                self.drag_payload_cache.selected.clear();
                self.drag_payload_cache.selected_revision = revision;
                self.drag_payload_cache.selected_generation = self.generation;
            }
            return self
                .drag_payload_cache
                .selected
                .entry(entry.path.clone())
                .or_insert_with(|| {
                    FileDragPayload::new(
                        Arc::clone(selected_paths),
                        entry.name.clone(),
                        file_icon_asset(entry),
                    )
                })
                .clone();
        }

        let snapshot_changed = self
            .drag_payload_cache
            .single_snapshot
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .is_none_or(|cached| !Arc::ptr_eq(&cached, snapshot));
        if snapshot_changed {
            self.drag_payload_cache.single.clear();
            self.drag_payload_cache.single_snapshot = Some(Arc::downgrade(snapshot));
        }
        self.drag_payload_cache
            .single
            .entry(entry.path.clone())
            .or_insert_with(|| {
                FileDragPayload::new(
                    Arc::from([entry.path.clone()]),
                    entry.name.clone(),
                    file_icon_asset(entry),
                )
            })
            .clone()
    }

    #[allow(clippy::too_many_lines)]
    fn render_file_list(
        &mut self,
        list_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.tab.root == TabRoot::Trash {
            return self.render_trash_list(list_focused, cx);
        }
        let selection = self.selection.clone();
        let rubber_band = match &self.pointer_interaction {
            PointerInteraction::RubberBand(state) => Some(state.clone()),
            _ => None,
        };
        let active_row_payload = match &self.pointer_interaction {
            PointerInteraction::RowArmed(press) => Some((press.index, press.payload.clone())),
            PointerInteraction::FileDrag(drag) => {
                Some((drag.press.index, drag.press.payload.clone()))
            }
            _ => None,
        };
        let snapshot = Arc::clone(&self.snapshot);
        let selected_paths = self.selected_paths_cached();
        let count = snapshot.entries.len();
        if count == 0 {
            self.rendered_file_range = 0..0;
        }
        let git_status_enabled = self.git_status_enabled;
        let sort = self.tab.sort;
        let operation_running = self.operation_running;
        let body = if count == 0 {
            let title = if self.loading {
                "Opening folder…"
            } else if self.error.is_some() {
                "Could not read this folder"
            } else {
                "This folder is quiet"
            };
            let subtitle = if self.loading || self.error.is_some() {
                ""
            } else {
                "Drop or create a file to get started."
            };
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .mt_8()
                .gap_2()
                .child(img("icons/empty-state.svg").size(px(280.0)))
                .child(
                    div()
                        .mt_2()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(theme_text()))
                        .child(title),
                )
                .when(!subtitle.is_empty(), |view| {
                    view.child(
                        div()
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child(subtitle),
                    )
                })
                .into_any_element()
        } else {
            uniform_list(
                "file-list",
                count,
                cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                    this.rendered_file_range = range.clone();
                    range
                        .map(|index| {
                            let entry = &snapshot.entries[index];
                            let highlighted = rubber_band.as_ref().map_or_else(
                                || selection.is_highlighted(index, entry),
                                |state| rubber_highlighted(state, index, entry),
                            );
                            let cursor_focused =
                                list_focused && selection.cursor == Some(index);
                            let drag_payload = active_row_payload
                                .as_ref()
                                .filter(|(active_index, _)| *active_index == index)
                                .map_or_else(
                                    || {
                                        this.cached_row_drag_payload(
                                            &snapshot,
                                            entry,
                                            &selected_paths,
                                            highlighted,
                                        )
                                    },
                                    |(_, payload)| payload.clone(),
                                );
                            let arm_payload = drag_payload.clone();
                            let is_drop_directory = entry.is_directory_like();
                            let drop_target = DropTarget::Directory(entry.path.clone());
                            let row = div()
                                .id(("entry", stable_path_id(&entry.path)))
                                .w_full()
                                .h(px(FILE_ROW_HEIGHT))
                                .px_2()
                                .rounded_md()
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_sm()
                                .active(|style| style.bg(rgb(accent_background())))
                                .when(highlighted, |style| {
                                    style
                                        .bg(rgb(accent_background()))
                                        .text_color(rgb(text_emphasized()))
                                })
                                .when(!highlighted, |style| {
                                    style
                                        .text_color(rgb(theme_text()))
                                        .hover(|style| style.bg(rgb(surface_elevated())))
                                })
                                .when(cursor_focused, |row| {
                                    row.border_1().border_color(rgb(border_focused()))
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                        window.focus(&this.focus_handle(cx));
                                        this.arm_row_drag(index, arm_payload.clone(), event);
                                        cx.stop_propagation();
                                    }),
                                )
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                        window.focus(&this.focus_handle(cx));
                                        cx.stop_propagation();
                                        let changed = prepare_context_selection(
                                            &mut this.selection,
                                            &this.snapshot.entries,
                                            index,
                                        );
                                        if changed {
                                            this.selection_changed(cx);
                                        }
                                        this.open_action_menu(
                                            ActionMenuPlacement::Cursor(event.position),
                                            cx,
                                        );
                                    }),
                                )
                                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                                    this.select_from_click(index, event, cx);
                                    if event.click_count() >= 2 {
                                        this.open_index(index, cx);
                                    }
                                }))
                                .when(!operation_running, |row| {
                                    row.on_drag(drag_payload, |payload, cursor_offset, _, cx| {
                                        cx.new(|_| DragGhost {
                                            payload: payload.clone(),
                                            cursor_offset,
                                        })
                                    })
                                })
                                .when(is_drop_directory, |row| {
                                    let predicate_target = drop_target.clone();
                                    let internal_move_target = drop_target.clone();
                                    let external_move_target = drop_target.clone();
                                    let internal_drop_target = drop_target.clone();
                                    let external_drop_target = drop_target.clone();
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
                                    .on_drop(cx.listener(
                                        move |this, paths: &ExternalPaths, _, cx| {
                                            this.perform_external_drop(
                                                paths,
                                                external_drop_target.clone(),
                                                cx,
                                            );
                                        },
                                    ))
                                })
                                .when(!is_drop_directory, |row| {
                                    row.can_drop(|_, _, _| false)
                                        .on_drag_move::<FileDragPayload>(|event, window, cx| {
                                            if event.bounds.contains(&event.event.position)
                                                && event.drag(cx).visual.lifted()
                                            {
                                                cx.set_active_drag_cursor_style(
                                                    CursorStyle::OperationNotAllowed,
                                                    window,
                                                );
                                            }
                                        })
                                        .on_drag_move::<ExternalPaths>(|event, window, cx| {
                                            if event.bounds.contains(&event.event.position) {
                                                cx.set_active_drag_cursor_style(
                                                    CursorStyle::OperationNotAllowed,
                                                    window,
                                                );
                                            }
                                        })
                                        .on_drop(|_: &FileDragPayload, _, _| {})
                                        .on_drop(|_: &ExternalPaths, _, _| {})
                                })
                                .child(file_icon(entry))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(entry.name.clone()),
                                )
                                .when(git_status_enabled, |row| {
                                    row.child(
                                        div()
                                            .w(px(FILE_GIT_COLUMN_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(git_color(entry.git_status))
                                            .child(git_label(entry.git_status)),
                                    )
                                })
                                .child(
                                    div()
                                        .w(px(FILE_SIZE_COLUMN_WIDTH))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(size_label(entry)),
                                )
                                .child(
                                    div()
                                        .w(px(FILE_MODIFIED_COLUMN_WIDTH))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(modified_label(entry)),
                                );
                            div().w_full().h(px(FILE_ROW_HEIGHT)).px_2().child(row)
                        })
                        .collect()
                }),
            )
            .track_scroll(self.file_list_scroll.clone())
            .h_full()
            .into_any_element()
        };
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(border()))
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child(
                        div()
                            .id("sort-name")
                            .with_focus_ring()
                            .flex_1()
                            .min_w_0()
                            .cursor_pointer()
                            .text_color(if sort.field == SortField::Name {
                                rgb(accent())
                            } else {
                                rgb(text_muted())
                            })
                            .hover(|style| style.text_color(rgb(theme_text())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sort_from_header(SortField::Name, cx);
                            }))
                            .child(sort_label("NAME", SortField::Name, sort)),
                    )
                    .when(git_status_enabled, |header| {
                        header.child(div().w(px(FILE_GIT_COLUMN_WIDTH)).flex_none().child("GIT"))
                    })
                    .child(
                        div()
                            .id("sort-size")
                            .with_focus_ring()
                            .w(px(FILE_SIZE_COLUMN_WIDTH))
                            .flex_none()
                            .cursor_pointer()
                            .text_color(if sort.field == SortField::Size {
                                rgb(accent())
                            } else {
                                rgb(text_muted())
                            })
                            .hover(|style| style.text_color(rgb(theme_text())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sort_from_header(SortField::Size, cx);
                            }))
                            .child(sort_label("SIZE", SortField::Size, sort)),
                    )
                    .child(
                        div()
                            .id("sort-modified")
                            .with_focus_ring()
                            .w(px(FILE_MODIFIED_COLUMN_WIDTH))
                            .flex_none()
                            .cursor_pointer()
                            .text_color(if sort.field == SortField::Modified {
                                rgb(accent())
                            } else {
                                rgb(text_muted())
                            })
                            .hover(|style| style.text_color(rgb(theme_text())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sort_from_header(SortField::Modified, cx);
                            }))
                            .child(sort_label("MODIFIED", SortField::Modified, sort)),
                    ),
            )
            .child({
                let background_target = DropTarget::Directory(self.tab.path.clone());
                let predicate_target = background_target.clone();
                let move_target = background_target.clone();
                let drop_target = background_target;
                div()
                    .id("file-list-viewport")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .can_drop(move |value, _, _| {
                        value.downcast_ref::<ExternalPaths>().is_some_and(|paths| {
                            can_external_drop_value(paths, &predicate_target, operation_running)
                        })
                    })
                    .drag_over::<ExternalPaths>(|style, _, _, _| {
                        style.bg(Hsla::from(rgb(accent_background())).opacity(0.24))
                    })
                    .on_drag_move::<ExternalPaths>(cx.listener(move |this, event, window, cx| {
                        this.update_external_drop_cursor(event, &move_target, window, cx);
                    }))
                    .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                        this.perform_external_drop(paths, drop_target.clone(), cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.begin_rubber_band(event, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.open_empty_space_menu(event.position, cx);
                        }),
                    )
                    .child(body)
                    .when(
                        matches!(
                            &self.pointer_interaction,
                            PointerInteraction::RubberBand(state) if state.crossed_threshold
                        ),
                        |viewport| viewport.child(self.render_rubber_band()),
                    )
            })
            .into_any_element()
    }

}
