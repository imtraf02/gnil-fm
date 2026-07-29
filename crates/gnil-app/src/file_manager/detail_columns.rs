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

    fn detail_column_width(&self, column: DetailColumn) -> f32 {
        let columns = self.settings.details_columns;
        match column {
            DetailColumn::Name => columns.name,
            DetailColumn::Modified => columns.modified,
            DetailColumn::Kind => columns.kind,
            DetailColumn::Size => columns.size,
            DetailColumn::TrashName => columns.trash_name,
            DetailColumn::TrashDeleted => columns.trash_deleted,
            DetailColumn::TrashLocation => columns.trash_location,
        }
    }

    fn set_detail_column_width(&mut self, column: DetailColumn, width: f32) {
        let width = match column {
            DetailColumn::Name | DetailColumn::TrashName => width.clamp(160.0, 640.0),
            DetailColumn::Modified => width.clamp(100.0, 260.0),
            DetailColumn::Kind => width.clamp(90.0, 240.0),
            DetailColumn::Size => width.clamp(80.0, 180.0),
            DetailColumn::TrashDeleted => width.clamp(110.0, 260.0),
            DetailColumn::TrashLocation => width.clamp(160.0, 520.0),
        };
        let columns = &mut self.settings.details_columns;
        match column {
            DetailColumn::Name => columns.name = width,
            DetailColumn::Modified => columns.modified = width,
            DetailColumn::Kind => columns.kind = width,
            DetailColumn::Size => columns.size = width,
            DetailColumn::TrashName => columns.trash_name = width,
            DetailColumn::TrashDeleted => columns.trash_deleted = width,
            DetailColumn::TrashLocation => columns.trash_location = width,
        }
    }

    fn begin_column_resize(
        &mut self,
        column: DetailColumn,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.column_resize = Some(ColumnResizeState {
            column,
            origin_x: f32::from(event.position.x),
            initial_width: self.detail_column_width(column),
        });
        cx.notify();
    }

    fn finish_column_resize(&mut self, cx: &mut Context<Self>) -> bool {
        if self.column_resize.take().is_none() {
            return false;
        }
        self.save_settings(cx);
        cx.notify();
        true
    }

    fn render_column_resize_handle(
        column: DetailColumn,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(("column-resize", column as usize))
            .debug_selector(move || format!("column-resize-{}", column as usize))
            .absolute()
            .right(px(-3.0))
            .top_0()
            .h_full()
            .w(px(7.0))
            .cursor(CursorStyle::ResizeLeftRight)
            .flex()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.begin_column_resize(column, event, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.finish_column_resize(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.finish_column_resize(cx);
                }),
            )
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h_full()
                    .w(px(1.0))
                    .bg(rgb(border()))
                    .hover(|style| style.bg(rgb(border_focused()))),
            )
            .into_any_element()
    }
}
