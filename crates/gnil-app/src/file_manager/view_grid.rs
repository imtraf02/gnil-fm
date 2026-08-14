impl FileManager {
    fn update_grid_geometry(&mut self, window: &Window) {
        let mut width = f32::from(window.viewport_size().width) - 218.0;
        if self.preview_visible {
            width -= 318.0;
        }
        let available = (width - GRID_HORIZONTAL_PADDING * 2.0).max(GRID_CARD_TARGET_WIDTH);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let columns =
            ((available + GRID_CARD_GAP) / (GRID_CARD_TARGET_WIDTH + GRID_CARD_GAP)).floor()
                as usize;
        self.grid_columns = columns.max(1);
        let column_count = f32::from(u16::try_from(self.grid_columns).unwrap_or(u16::MAX));
        let gap_count = f32::from(
            u16::try_from(self.grid_columns.saturating_sub(1)).unwrap_or(u16::MAX),
        );
        self.grid_card_width =
            ((available - GRID_CARD_GAP * gap_count) / column_count).max(120.0);
    }

    fn render_file_grid(&mut self, list_focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let count = self.snapshot.entries.len();
        if count == 0 {
            self.rendered_file_range = 0..0;
            return Self::render_grid_empty_state(
                if self.loading {
                    "Opening folder…"
                } else if self.error.is_some() {
                    "Could not read this folder"
                } else {
                    "This folder is quiet"
                },
                if self.loading || self.error.is_some() {
                    ""
                } else {
                    "Drop or create a file to get started."
                },
            );
        }
        let columns = self.grid_columns;
        let row_count = count.div_ceil(columns);
        let card_width = self.grid_card_width;
        let snapshot = Arc::clone(&self.snapshot);
        let selection = self.selection.clone();
        let selected_paths = self.selected_paths_cached();
        let favorites = Arc::clone(&self.favorite_paths);
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
        let body = uniform_list(
            "file-grid",
            row_count,
            cx.processor(move |this, range: Range<usize>, _window, cx| {
                this.rendered_file_range =
                    range.start.saturating_mul(columns)..range.end.saturating_mul(columns).min(count);
                this.perf_trace
                    .record_rows_rendered(this.rendered_file_range.len());
                range
                    .map(|row_index| {
                        let first = row_index * columns;
                        let last = (first + columns).min(count);
                        let mut row = div()
                            .h(px(GRID_ROW_HEIGHT))
                            .w_full()
                            .px_3()
                            .pt_2()
                            .flex()
                            .gap(px(GRID_CARD_GAP));
                        for index in first..last {
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
                            let is_favorite = favorites.contains(&entry.path);
                            row = row.child(this.render_grid_card(
                                index,
                                entry,
                                card_width,
                                highlighted,
                                cursor_focused,
                                is_favorite,
                                drag_payload, cx,
                            ));
                        }
                        row
                    })
                    .collect()
            }),
        )
        .item_height(px(GRID_ROW_HEIGHT))
        .track_scroll(self.file_list_scroll.clone())
        .h_full();
        self.render_grid_viewport(body.into_any_element(), true, cx)
    }

    #[allow(
        clippy::fn_params_excessive_bools,
        clippy::too_many_arguments,
        clippy::too_many_lines
    )]
    fn render_grid_card(
        &mut self,
        index: usize,
        entry: &FileEntry,
        width: f32,
        highlighted: bool,
        cursor_focused: bool,
        is_favorite: bool,
        drag_payload: FileDragPayload,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let arm_payload = drag_payload.clone();
        let is_drop_directory = entry.is_directory_like();
        let favorite_path = entry.path.clone();
        let drop_target = DropTarget::Directory(entry.path.clone());
        let rename_input = self
            .inline_rename
            .as_ref()
            .filter(|rename| rename.path == entry.path)
            .map(|rename| rename.input.clone());
        let renaming = rename_input.is_some();
        let path_id = stable_path_id(&entry.path);
        let card = div()
            .id(("grid-entry", path_id))
            .relative()
            .w(px(width))
            .h(px(174.0))
            .flex_none()
            .p_2()
            .rounded_md()
            .flex()
            .flex_col()
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
            .when(cursor_focused, |card| {
                card.border_1().border_color(rgb(border_focused()))
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
                    this.open_action_menu(ActionMenuPlacement::Cursor(event.position), cx);
                }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                let opens_directory = event.click_count() >= 2
                    && this
                        .snapshot
                        .entries
                        .get(index)
                        .is_some_and(FileEntry::is_directory_like);
                this.select_from_click(index, event, !opens_directory, cx);
                if event.click_count() >= 2 {
                    this.open_index(index, window, cx);
                }
            }))
            .when(!renaming, |card| {
                card.on_drag(drag_payload, |payload, cursor_offset, _, cx| {
                    cx.new(|_| DragGhost {
                        payload: payload.clone(),
                        cursor_offset,
                    })
                })
            })
            .when(is_drop_directory, |card| {
                let predicate_target = drop_target.clone();
                let internal_move_target = drop_target.clone();
                let external_move_target = drop_target.clone();
                let internal_drop_target = drop_target.clone();
                let external_drop_target = drop_target;
                card.can_drop(move |value, _, _| {
                    can_drop_value(value, &predicate_target)
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
                .on_drag_move::<FileDragPayload>(cx.listener(move |this, event, window, cx| {
                    this.update_internal_drop_cursor(event, &internal_move_target, window, cx);
                }))
                .on_drag_move::<ExternalPaths>(cx.listener(move |this, event, window, cx| {
                    this.update_external_drop_cursor(event, &external_move_target, window, cx);
                }))
                .on_drop(cx.listener(move |this, payload: &FileDragPayload, window, cx| {
                    this.perform_internal_drop(
                        payload,
                        internal_drop_target.clone(),
                        window,
                        cx,
                    );
                }))
                .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                    this.perform_external_drop(paths, external_drop_target.clone(), cx);
                }))
            })
            .child(self.render_grid_preview(entry, &entry.path, cx))
            .child(if let Some(input) = rename_input {
                div()
                    .id(("inline-rename-grid", path_id))
                    .w_full()
                    .h(px(38.0))
                    .pt_1()
                    .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(input)
                    .into_any_element()
            } else {
                div()
                    .w_full()
                    .h(px(38.0))
                    .pt_1()
                    .overflow_hidden()
                    .text_center()
                    .line_height(px(18.0))
                    .child(entry.name.clone())
                    .into_any_element()
            })
            .when(is_drop_directory, |card| {
                card.child(
                    div()
                        .id(("grid-toggle-favorite", path_id))
                        .absolute()
                        .top_1()
                        .right_1()
                        .size_6()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .bg(rgb(surface()))
                        .hover(|style| style.bg(rgb(border())))
                        .with_focus_ring()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_favorite_path(favorite_path.clone(), cx);
                        }))
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
                        )),
                )
            });
        card.into_any_element()
    }

    fn render_grid_preview(
        &mut self,
        entry: &FileEntry,
        source: &Path,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !entry.is_directory_like() {
            self.ensure_grid_preview(entry.path.clone(), source.to_path_buf(), cx);
        }
        let content = match self.grid_previews.get(&entry.path) {
            Some(GridPreviewState::Thumbnail(path)) => img(Arc::<Path>::from(path.as_path()))
                .size_full()
                .object_fit(gpui::ObjectFit::Contain)
                .rounded_md()
                .into_any_element(),
            Some(GridPreviewState::Text(lines)) => lines
                .iter()
                .take(6)
                .fold(
                    div()
                        .size_full()
                        .p_2()
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .font_family("Noto Sans Mono")
                        .text_xs()
                        .line_height(px(15.0))
                        .text_color(rgb(text_muted())),
                    |preview, line| preview.child(div().truncate().child(line.clone())),
                )
                .into_any_element(),
            Some(GridPreviewState::Loading | GridPreviewState::Unavailable) | None => ui_icon(
                file_icon_asset(entry),
                IconSize::Detail,
                if matches!(
                    self.grid_previews.get(&entry.path),
                    Some(GridPreviewState::Loading)
                ) {
                    IconTone::Muted
                } else {
                    IconTone::Default
                },
            )
            .into_any_element(),
        };
        div()
            .w_full()
            .h(px(108.0))
            .flex_none()
            .rounded_md()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(background()))
            .overflow_hidden()
            .child(content)
            .into_any_element()
    }

    #[allow(clippy::too_many_lines)]
    fn render_trash_grid(&mut self, list_focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let entries = Arc::clone(&self.trash_entries);
        let count = entries.len();
        if count == 0 {
            self.rendered_file_range = 0..0;
            return Self::render_grid_empty_state(
                if self.loading {
                    "Reading Trash…"
                } else {
                    "Trash is empty"
                },
                "Deleted items from available devices appear together here.",
            );
        }
        let columns = self.grid_columns;
        let row_count = count.div_ceil(columns);
        let card_width = self.grid_card_width;
        let selection = self.selection.clone();
        let rubber_band = match &self.pointer_interaction {
            PointerInteraction::RubberBand(state) => Some(state.clone()),
            _ => None,
        };
        let body = uniform_list(
            "trash-grid",
            row_count,
            cx.processor(move |this, range: Range<usize>, _window, cx| {
                this.rendered_file_range =
                    range.start.saturating_mul(columns)..range.end.saturating_mul(columns).min(count);
                this.perf_trace
                    .record_rows_rendered(this.rendered_file_range.len());
                range
                    .map(|row_index| {
                        let first = row_index * columns;
                        let last = (first + columns).min(count);
                        let mut row = div()
                            .h(px(GRID_ROW_HEIGHT))
                            .w_full()
                            .px_3()
                            .pt_2()
                            .flex()
                            .gap(px(GRID_CARD_GAP));
                        for index in first..last {
                            let entry = entries[index].clone();
                            let file_entry = trash_entry_as_file_entry(&entry);
                            let highlighted = rubber_band.as_ref().map_or_else(
                                || selection.is_highlighted(index, &file_entry),
                                |state| rubber_highlighted(state, index, &file_entry),
                            );
                            let cursor_focused =
                                list_focused && selection.cursor == Some(index);
                            row = row.child(
                                div()
                                    .id(("trash-grid-entry", stable_path_id(&file_entry.path)))
                                    .w(px(card_width))
                                    .h(px(174.0))
                                    .flex_none()
                                    .p_2()
                                    .rounded_md()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .cursor_pointer()
                                    .text_sm()
                                    .when(highlighted, |card| {
                                        card.bg(rgb(accent_background()))
                                            .text_color(rgb(text_emphasized()))
                                    })
                                    .when(!highlighted, |card| {
                                        card.text_color(rgb(theme_text()))
                                            .hover(|style| style.bg(rgb(surface_elevated())))
                                    })
                                    .when(cursor_focused, |card| {
                                        card.border_1().border_color(rgb(border_focused()))
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, window, cx| {
                                            window.focus(&this.focus_handle(cx));
                                            cx.stop_propagation();
                                        }),
                                    )
                                    .on_click(cx.listener(
                                        move |this, event: &ClickEvent, window, cx| {
                                            this.select_from_click(index, event, true, cx);
                                            if event.click_count() >= 2 {
                                                this.open_index(index, window, cx);
                                            }
                                        },
                                    ))
                                    .child(this.render_grid_preview(
                                        &file_entry,
                                        &entry.reference.trashed_path,
                                        cx,
                                    ))
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(38.0))
                                            .pt_1()
                                            .overflow_hidden()
                                            .text_center()
                                            .line_height(px(18.0))
                                            .child(entry.name),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .truncate()
                                            .text_center()
                                            .text_xs()
                                            .text_color(rgb(text_muted()))
                                            .child(deletion_label(entry.deletion_unix)),
                                    ),
                            );
                        }
                        row
                    })
                    .collect()
            }),
        )
        .item_height(px(GRID_ROW_HEIGHT))
        .track_scroll(self.file_list_scroll.clone())
        .h_full();
        self.render_grid_viewport(body.into_any_element(), false, cx)
    }

    fn render_grid_viewport(
        &self,
        body: AnyElement,
        allow_background_menu: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let background_target = DropTarget::Directory(self.tab.path.clone());
        let predicate_target = background_target.clone();
        let move_target = background_target.clone();
        let drop_target = background_target;
        div()
            .id("file-list-viewport")
            .relative()
            .size_full()
            .min_h_0()
            .can_drop(move |value, _, _| {
                value.downcast_ref::<ExternalPaths>().is_some_and(|paths| {
                    can_external_drop_value(paths, &predicate_target)
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
            .when(allow_background_menu, |viewport| {
                viewport.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.open_empty_space_menu(event.position, cx);
                    }),
                )
            })
            .child(body)
            .into_any_element()
    }

    fn render_grid_empty_state(title: &'static str, subtitle: &'static str) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
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
                view.child(div().text_xs().text_color(rgb(text_muted())).child(subtitle))
            })
            .into_any_element()
    }

}
