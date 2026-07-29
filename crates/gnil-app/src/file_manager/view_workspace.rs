impl FileManager {
    fn render_preview(&self) -> AnyElement {
        let content = match (
            &self.preview_path,
            &self.preview,
            &self.preview_request_path,
        ) {
            (Some(path), Some(preview), _) => preview_content(path, preview),
            (_, _, Some(_)) => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(rgb(text_muted()))
                .child("Reading preview…")
                .into_any_element(),
            _ => empty_preview(),
        };
        div()
            .w(px(318.0))
            .min_w(px(240.0))
            .h_full()
            .flex_none()
            .debug_selector(|| "preview-surface".into())
            .flex()
            .flex_col()
            .bg(rgb(surface()))
            .border_l_1()
            .border_color(rgb(border()))
            .child(
                div()
                    .h(px(42.0))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child("PREVIEW"),
            )
            .child(content)
            .into_any_element()
    }

    fn render_rubber_band(&self) -> AnyElement {
        let PointerInteraction::RubberBand(state) = &self.pointer_interaction else {
            return div().size_full().into_any_element();
        };
        if !state.crossed_threshold {
            return div().size_full().into_any_element();
        }
        let (viewport, _) = self.file_list_metrics();
        let viewport_left = f32::from(viewport.left());
        let viewport_right = f32::from(viewport.right());
        let viewport_top = f32::from(viewport.top());
        let viewport_bottom = f32::from(viewport.bottom());
        let left = f32::from(state.origin.x)
            .min(f32::from(state.current.x))
            .clamp(viewport_left, viewport_right);
        let right = f32::from(state.origin.x)
            .max(f32::from(state.current.x))
            .clamp(viewport_left, viewport_right);
        let top = f32::from(state.origin.y)
            .min(f32::from(state.current.y))
            .clamp(viewport_top, viewport_bottom);
        let bottom = f32::from(state.origin.y)
            .max(f32::from(state.current.y))
            .clamp(viewport_top, viewport_bottom);
        div()
            .size_full()
            .child(
                div()
                    .debug_selector(|| "rubber-band".into())
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px((right - left).max(1.0)))
                    .h(px((bottom - top).max(1.0)))
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(accent()))
                    .bg(Hsla::from(rgb(accent())).opacity(0.12)),
            )
            .into_any_element()
    }

    fn render_status(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut message = if let Some(error) = &self.error {
            error.clone()
        } else if self.operation_running {
            self.status_message
                .clone()
                .unwrap_or_else(|| "Working…".into())
        } else if let Some(message) = &self.status_message {
            message.clone()
        } else if self.loading {
            "Scanning folder…".into()
        } else if self.selection.selected_count() > 0 {
            format!("{} selected", self.selection.selected_count())
        } else {
            format!("{} items", self.snapshot.entries.len())
        };
        if let Some(progress) = &self.operation_progress {
            let item_progress = progress.total_items.map_or_else(
                || progress.completed_items.to_string(),
                |total| format!("{} / {total}", progress.completed_items),
            );
            let byte_progress = if progress.completed_bytes > 0 {
                format!(" · {}", format_bytes(progress.completed_bytes))
            } else {
                String::new()
            };
            message = format!("Extracting {item_progress}{byte_progress}");
        }
        div()
            .h(px(30.0))
            .w_full()
            .flex_none()
            .px_3()
            .flex()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(rgb(border()))
            .text_xs()
            .text_color(if self.error.is_some() {
                rgb(error_color())
            } else {
                rgb(text_muted())
            })
            .child(message)
            .when(
                self.operation_running && self.operation_cancel.is_some(),
                |status| {
                    status.child(
                        div()
                            .id("cancel-operation")
                            .with_focus_ring()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_color(rgb(theme_text()))
                            .hover(|style| style.bg(rgb(surface_elevated())))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel_operation(&CancelOperation, window, cx);
                            }))
                            .child("Cancel"),
                    )
                },
            )
            .when(!self.operation_running, |status| {
                status.child("Ctrl+Shift+C Copy Path · Del Trash · Ctrl+Z Undo")
            })
            .into_any_element()
    }

    fn render_workspace(&mut self, _cx: &mut Context<Self>) -> AnyElement {
        let right_panel = if self.preview_visible {
            Some(self.surfaces.preview().into_any_element())
        } else {
            None
        };
        div()
            .size_full()
            .flex()
            .child(self.surfaces.sidebar())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.surfaces.header())
                    .child(self.surfaces.command_bar())
                    .child(
                        div()
                            .flex_1()
                            .w_full()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .child(
                                div()
                                    .id("file-list-surface-frame")
                                    .debug_selector(|| "file-list-surface-frame".into())
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_hidden()
                                    .child(self.surfaces.file_list()),
                            )
                            .when_some(right_panel, gpui::ParentElement::child),
                    )
                    .child(self.surfaces.status()),
            )
            .into_any_element()
    }
}

impl Focusable for FileManager {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for FileManager {
    #[allow(clippy::too_many_lines)]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let started = self.perf_trace.start();
        let root = div()
            .id("gnil-root")
            .relative()
            .key_context(
                if self.open_with_chooser.is_some() {
                    "OpenWithChooser"
                } else if self.column_resize.is_some()
                    || !matches!(&self.pointer_interaction, PointerInteraction::Idle)
                {
                    "PointerInteraction"
                } else if self.action_menu.is_some()
                    || self.empty_space_menu.is_some()
                    || self.appearance_menu_open
                    || self.command_bar_menu.is_some()
                {
                    "ActionMenu"
                } else {
                    "FileManager"
                },
            )
            .track_focus(&self.focus_handle(cx))
            .when(self.column_resize.is_some(), |root| {
                root.cursor(CursorStyle::ResizeLeftRight)
            })
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_next_range))
            .on_action(cx.listener(Self::select_previous_range))
            .on_action(cx.listener(Self::select_left_range))
            .on_action(cx.listener(Self::select_right_range))
            .on_action(cx.listener(Self::toggle_selection))
            .on_action(cx.listener(Self::open_selected))
            .on_action(cx.listener(Self::open_with_selected))
            .on_action(cx.listener(Self::confirm_open_with))
            .on_action(cx.listener(Self::dismiss_open_with))
            .on_action(cx.listener(Self::open_with_next))
            .on_action(cx.listener(Self::open_with_previous))
            .on_action(cx.listener(Self::toggle_open_with_default))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::go_up))
            .on_action(cx.listener(Self::toggle_preview))
            .on_action(cx.listener(Self::toggle_hidden))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::copy_selected))
            .on_action(cx.listener(Self::cut_selected))
            .on_action(cx.listener(Self::copy_path_absolute))
            .on_action(cx.listener(Self::copy_path_relative))
            .on_action(cx.listener(Self::toggle_actions))
            .on_action(cx.listener(Self::toggle_appearance))
            .on_action(cx.listener(Self::open_create_symlink))
            .on_action(cx.listener(Self::open_permissions))
            .on_action(cx.listener(Self::open_rename))
            .on_action(cx.listener(Self::confirm_inline_rename))
            .on_action(cx.listener(Self::cancel_inline_rename))
            .on_action(cx.listener(Self::extract_selected))
            .on_action(cx.listener(Self::extract_selected_to))
            .on_action(cx.listener(Self::cancel_operation))
            .on_action(cx.listener(Self::dismiss_sheet))
            .on_action(cx.listener(Self::apply_sheet))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::trash_selected))
            .on_action(cx.listener(Self::delete_selected))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::menu_next))
            .on_action(cx.listener(Self::menu_previous))
            .on_action(cx.listener(Self::menu_first))
            .on_action(cx.listener(Self::menu_last))
            .on_action(cx.listener(Self::menu_activate))
            .on_action(cx.listener(Self::menu_open_submenu))
            .on_action(cx.listener(Self::menu_close_submenu))
            .on_action(cx.listener(Self::dismiss_menu))
            .on_action(cx.listener(Self::create_folder))
            .on_action(cx.listener(Self::create_file))
            .on_action(cx.listener(Self::restore_trash_selected))
            .on_action(cx.listener(Self::empty_trash))
            .on_action(cx.listener(Self::handle_escape))
            .on_action(cx.listener(Self::cancel_pointer_interaction))
            .on_action(cx.listener(Self::activate_path_input))
            .on_action(cx.listener(Self::submit_path_input))
            .on_action(cx.listener(Self::dismiss_path_input))
            .on_action(cx.listener(Self::complete_path_next))
            .on_action(cx.listener(Self::complete_path_previous))
            .on_action(cx.listener(Self::path_history_previous))
            .on_action(cx.listener(Self::path_history_next))
            .on_action(cx.listener(Self::paste_path))
            .on_action(cx.listener(Self::activate_file_search))
            .on_action(cx.listener(Self::dismiss_file_search))
            .on_action(cx.listener(Self::file_search_next))
            .on_action(cx.listener(Self::file_search_previous))
            .on_action(cx.listener(Self::open_file_search_result))
            .on_action(cx.listener(Self::toggle_selected_favorite))
            .on_action(cx.listener(|this, _: &SelectAllEntries, _, cx| {
                this.select_all_entries(cx);
            }))
            .on_action(cx.listener(Self::toggle_settings))
            .on_action(cx.listener(Self::open_keymap))
            .on_any_mouse_down(cx.listener(
                |this, _: &MouseDownEvent, window, cx| {
                    if this.file_search.open {
                        this.dismiss_file_search(&DismissFileSearch, window, cx);
                    }
                },
            ))
            .on_mouse_move(cx.listener(Self::handle_pointer_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(Self::finish_pointer_interaction),
            )
            .on_drag_move::<FileDragPayload>(cx.listener(Self::handle_file_drag_move))
            .on_drag_move::<ExternalPaths>(|_, window, cx| {
                cx.set_active_drag_cursor_style(CursorStyle::OperationNotAllowed, window);
            })
            .on_modifiers_changed(cx.listener(Self::handle_drag_modifiers))
            .size_full()
            .flex()
            .bg(rgb(background()))
            .text_color(rgb(text_emphasized()))
            .child(self.render_workspace(cx))
            .child(self.surfaces.rubber_band())
            .when(
                self.action_menu.is_some()
                    || self.empty_space_menu.is_some()
                    || self.appearance_menu_open
                    || self.command_bar_menu.is_some(),
                |root| root.child(Self::render_context_menu_backdrop(cx)),
            )
            .when(self.empty_space_menu.is_some(), |root| {
                root.child(self.render_empty_space_menu(window, cx))
            })
            .when(
                matches!(
                    self.action_menu.as_ref().map(|menu| menu.placement),
                    Some(ActionMenuPlacement::Cursor(_))
                ),
                |root| root.child(self.render_context_action_menu(window, cx)),
            )
            .when(self.operation_sheet.is_some(), |root| {
                root.child(self.render_operation_sheet(cx))
            })
            .when(self.open_with_chooser.is_some(), |root| {
                root.child(self.render_open_with_chooser(cx))
            });
        self.perf_trace.record_root(started);
        root
    }
}
