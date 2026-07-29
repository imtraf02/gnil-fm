impl FileManager {
    #[allow(clippy::too_many_lines)]
    fn render_file_list(
        &mut self,
        list_focused: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.tab.root == TabRoot::Trash {
            if self.settings.file_layout == FileLayout::Grid {
                self.update_grid_geometry(window);
                return self.render_trash_grid(list_focused, cx);
            }
            return self.render_trash_list(list_focused, window, cx);
        }
        if self.settings.file_layout == FileLayout::Grid {
            self.update_grid_geometry(window);
            return self.render_file_grid(list_focused, cx);
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
        let favorites = Arc::clone(&self.favorite_paths);
        let selected_paths = self.selected_paths_cached();
        let count = snapshot.entries.len();
        if count == 0 {
            self.rendered_file_range = 0..0;
        }
        let sort = self.tab.sort;
        let operation_running = self.operation_running;
        let name_column_width = self.detail_column_width(DetailColumn::Name);
        let modified_column_width = self.detail_column_width(DetailColumn::Modified);
        let type_column_width = self.detail_column_width(DetailColumn::Kind);
        let size_column_width = self.detail_column_width(DetailColumn::Size);
        let row_surface_width =
            name_column_width + modified_column_width + type_column_width + size_column_width + 16.0;
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
                    this.perf_trace.record_rows_rendered(range.len());
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
                            let is_favorite = favorites.contains(&entry.path);
                            let rename_input = this
                                .inline_rename
                                .as_ref()
                                .filter(|rename| rename.path == entry.path)
                                .map(|rename| rename.input.clone());
                            let renaming = rename_input.is_some();
                            let path_id = stable_path_id(&entry.path);
                            let row_group = format!("file-row-{path_id}");
                            let favorite_path = entry.path.clone();
                            let drop_target = DropTarget::Directory(entry.path.clone());
                            let row = div()
                                .id(("entry", path_id))
                                .group(row_group.clone())
                                .when(index == 0, |row| {
                                    row.debug_selector(|| "details-row-0".into())
                                })
                                .w(px(row_surface_width))
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
                                    let opens_directory = event.click_count() >= 2
                                        && this
                                            .snapshot
                                            .entries
                                            .get(index)
                                            .is_some_and(FileEntry::is_directory_like);
                                    this.select_from_click(index, event, !opens_directory, cx);
                                    if event.click_count() >= 2 {
                                        this.open_index(index, cx);
                                    }
                                }))
                                .when(!operation_running && !renaming, |row| {
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
                                .child(
                                    div()
                                        .w(px(name_column_width))
                                        .min_w_0()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .child(file_icon(entry))
                                        .child(if let Some(input) = rename_input {
                                            div()
                                                .id((
                                                    "inline-rename-details",
                                                    path_id,
                                                ))
                                                .flex_1()
                                                .min_w_0()
                                                .on_any_mouse_down(|_, _, cx| {
                                                    cx.stop_propagation();
                                                })
                                                .on_click(|_, _, cx| cx.stop_propagation())
                                                .child(input)
                                                .into_any_element()
                                        } else {
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .truncate()
                                                .child(entry.name.clone())
                                                .into_any_element()
                                        })
                                        .child(if is_drop_directory {
                                            div()
                                                .id((
                                                    "toggle-favorite",
                                                    path_id,
                                                ))
                                                .with_focus_ring()
                                                .w(px(FILE_FAVORITE_ACTION_WIDTH))
                                                .size_6()
                                                .flex_none()
                                                .rounded_md()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .opacity(if highlighted { 1.0 } else { 0.0 })
                                                .group_hover(row_group, |style| style.opacity(1.0))
                                                .hover(|style| style.bg(rgb(border())))
                                                .on_click(cx.listener(
                                                    move |this, _, _, cx| {
                                                        cx.stop_propagation();
                                                        this.toggle_favorite_path(
                                                            favorite_path.clone(),
                                                            cx,
                                                        );
                                                    },
                                                ))
                                                .child(ui_icon(
                                                    if is_favorite {
                                                        "icons/action-star-filled.svg"
                                                    } else {
                                                        "icons/action-star.svg"
                                                    },
                                                    IconSize::Small,
                                                    if is_favorite {
                                                        IconTone::Accent
                                                    } else {
                                                        IconTone::Muted
                                                    },
                                                ))
                                                .into_any_element()
                                        } else {
                                            div()
                                                .w(px(FILE_FAVORITE_ACTION_WIDTH))
                                                .flex_none()
                                                .into_any_element()
                                        }),
                                )
                                .child(
                                    div()
                                        .w(px(modified_column_width))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(modified_label(entry)),
                                )
                                .child(
                                    div()
                                        .w(px(type_column_width))
                                        .flex_none()
                                        .truncate()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(type_label(entry)),
                                )
                                .child(
                                    div()
                                        .w(px(size_column_width))
                                        .flex_none()
                                        .pr_2()
                                        .text_right()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(size_label(entry)),
                                );
                            div().w_full().h(px(FILE_ROW_HEIGHT)).px_2().child(row)
                        })
                        .collect()
                }),
            )
            .item_height(px(FILE_ROW_HEIGHT))
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
                            .relative()
                            .w(px(name_column_width))
                            .flex_none()
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
                            .child(sort_label("NAME", SortField::Name, sort))
                            .child(Self::render_column_resize_handle(
                                DetailColumn::Name,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .id("sort-modified")
                            .with_focus_ring()
                            .relative()
                            .w(px(modified_column_width))
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
                            .child(sort_label("DATE MODIFIED", SortField::Modified, sort))
                            .child(Self::render_column_resize_handle(
                                DetailColumn::Modified,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .id("sort-type")
                            .with_focus_ring()
                            .relative()
                            .w(px(type_column_width))
                            .flex_none()
                            .cursor_pointer()
                            .text_color(if sort.field == SortField::Kind {
                                rgb(accent())
                            } else {
                                rgb(text_muted())
                            })
                            .hover(|style| style.text_color(rgb(theme_text())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sort_from_header(SortField::Kind, cx);
                            }))
                            .child(sort_label("TYPE", SortField::Kind, sort))
                            .child(Self::render_column_resize_handle(
                                DetailColumn::Kind,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .id("sort-size")
                            .with_focus_ring()
                            .relative()
                            .w(px(size_column_width))
                            .flex_none()
                            .pr_2()
                            .text_right()
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
                            .child(sort_label("SIZE", SortField::Size, sort))
                            .child(Self::render_column_resize_handle(
                                DetailColumn::Size,
                                cx,
                            )),
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
            })
            .into_any_element()
    }

}
