impl FileManager {
    #[allow(clippy::too_many_lines)]
    fn render_trash_list(
        &mut self,
        list_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selection = self.selection.clone();
        let rubber_band = match &self.pointer_interaction {
            PointerInteraction::RubberBand(state) => Some(state.clone()),
            _ => None,
        };
        let entries = Arc::new(self.trash_entries.clone());
        let count = entries.len();
        if count == 0 {
            self.rendered_file_range = 0..0;
        }
        let has_selection = self.selection.selected_count() > 0;
        let body = if count == 0 {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .mt_8()
                .gap_2()
                .child(img("icons/trash-empty.svg").size(px(280.0)))
                .child(
                    div()
                        .mt_2()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(theme_text()))
                        .child(if self.loading {
                            "Reading Trash…"
                        } else {
                            "Trash is empty"
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(text_muted()))
                        .child("Deleted items from available devices appear together here."),
                )
                .into_any_element()
        } else {
            uniform_list(
                "trash-list",
                count,
                cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                    this.rendered_file_range = range.clone();
                    range
                        .map(|index| {
                            let entry = entries[index].clone();
                            let file_entry = trash_entry_as_file_entry(&entry);
                            let highlighted = rubber_band.as_ref().map_or_else(
                                || selection.is_highlighted(index, &file_entry),
                                |state| rubber_highlighted(state, index, &file_entry),
                            );
                            let cursor_focused =
                                list_focused && selection.cursor == Some(index);
                            let row = div()
                                .id(("trash-entry", stable_path_id(&file_entry.path)))
                                .w_full()
                                .h(px(TRASH_ROW_HEIGHT))
                                .px_2()
                                .rounded_md()
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_sm()
                                .active(|style| style.bg(rgb(accent_background())))
                                .when(highlighted, |row| {
                                    row.bg(rgb(accent_background()))
                                        .text_color(rgb(text_emphasized()))
                                })
                                .when(!highlighted, |row| {
                                    row.text_color(rgb(theme_text()))
                                        .hover(|style| style.bg(rgb(surface_elevated())))
                                })
                                .when(cursor_focused, |row| {
                                    row.border_1().border_color(rgb(border_focused()))
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, window, cx| {
                                        window.focus(&this.focus_handle(cx));
                                        cx.stop_propagation();
                                    }),
                                )
                                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                                    this.select_from_click(index, event, cx);
                                    if event.click_count() >= 2 {
                                        this.open_index(index, cx);
                                    }
                                }))
                                .child(file_icon(&file_entry))
                                .child(div().flex_1().min_w_0().truncate().child(entry.name))
                                .child(
                                    div()
                                        .w(px(142.0))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(deletion_label(entry.deletion_unix)),
                                )
                                .child(
                                    div()
                                        .w(px(280.0))
                                        .flex_none()
                                        .truncate()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(entry.original_path.display().to_string()),
                                );
                            div().w_full().h(px(TRASH_ROW_HEIGHT)).px_2().child(row)
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
                    .h(px(42.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(border()))
                    .child(
                        trash_action_button("Restore", false, has_selection).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.restore_trash_selected(&RestoreTrashSelected, window, cx);
                            },
                        )),
                    )
                    .child(
                        trash_action_button("Delete Permanently", true, has_selection).on_click(
                            cx.listener(|this, _, window, cx| {
                                this.purge_selected_trash(window, cx);
                            }),
                        ),
                    )
                    .child(div().flex_1())
                    .child(
                        trash_action_button("Empty Trash", true, count > 0).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.empty_trash(&EmptyTrash, window, cx);
                            },
                        )),
                    ),
            )
            .child(
                div()
                    .h(px(32.0))
                    .px_4()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(rgb(border()))
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child(div().flex_1().child("NAME"))
                    .child(div().w(px(142.0)).flex_none().child("DELETED"))
                    .child(div().w(px(280.0)).flex_none().child("ORIGINAL LOCATION")),
            )
            .child(
                div()
                    .id("file-list-viewport")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.begin_rubber_band(event, cx);
                        }),
                    )
                    .child(body)
                    .when(
                        matches!(
                            &self.pointer_interaction,
                            PointerInteraction::RubberBand(state) if state.crossed_threshold
                        ),
                        |viewport| viewport.child(self.render_rubber_band()),
                    ),
            )
            .into_any_element()
    }
}
