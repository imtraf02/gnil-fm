impl FileManager {
    #[allow(clippy::too_many_lines)]
    fn render_operation_sheet(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let read_only = matches!(
            self.operation_sheet,
            Some(OperationSheet::FolderProperties { .. })
        );
        let (title, subtitle, body) = match self.operation_sheet.as_ref().expect("active sheet") {
            OperationSheet::Extract {
                sources,
                destination,
            } => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(field_label("DESTINATION FOLDER"))
                    .child(destination.clone())
                    .child(helper_text(
                        "Each archive creates a named output. Existing items are never merged or replaced.",
                    ))
                    .into_any_element();
                (
                    "Extract to…",
                    format!(
                        "{} archive{}",
                        sources.len(),
                        if sources.len() == 1 { "" } else { "s" }
                    ),
                    body,
                )
            }
            OperationSheet::CreateEntry { kind, name } => {
                let title = match kind {
                    CreateEntryKind::Folder => "New folder",
                    CreateEntryKind::File => "New file",
                };
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(field_label("NAME"))
                    .child(name.clone())
                    .child(helper_text(
                        "Created in the current folder; an existing item is never replaced.",
                    ))
                    .into_any_element();
                (title, self.tab.path.display().to_string(), body)
            }
            OperationSheet::FolderProperties {
                path,
                item_count,
                file_bytes,
                mode,
                readonly,
            } => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(property_row("Path", path.display().to_string()))
                    .child(property_row("Loaded items", item_count.to_string()))
                    .child(property_row("Visible file size", format_bytes(*file_bytes)))
                    .child(property_row(
                        "Permissions",
                        mode.map_or_else(|| "Unavailable".into(), |mode| format!("{mode:04o}")),
                    ))
                    .child(property_row(
                        "Access",
                        if *readonly { "Read only" } else { "Read and write" },
                    ))
                    .child(helper_text(
                        "Item count and size reflect the currently loaded view, including its hidden-file setting.",
                    ))
                    .into_any_element();
                (
                    "Folder properties",
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                        .into_owned(),
                    body,
                )
            }
            OperationSheet::Symlink {
                target,
                name,
                relative,
            } => {
                let relative = *relative;
                let target = target.clone();
                let name = name.clone();
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(field_label("TARGET"))
                    .child(target)
                    .child(field_label("LINK NAME"))
                    .child(name)
                    .child(
                        div()
                            .id("relative-link")
                            .with_focus_ring()
                            .h_8()
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(OperationSheet::Symlink { relative, .. }) =
                                    this.operation_sheet.as_mut()
                                {
                                    *relative = !*relative;
                                    cx.notify();
                                }
                            }))
                            .child(if relative { "●" } else { "○" })
                            .child("Store relative target"),
                    )
                    .child(helper_text(
                        "Dangling targets are allowed; existing paths are never replaced.",
                    ))
                    .into_any_element();
                ("Create symlink", "Link from this folder".to_owned(), body)
            }
            OperationSheet::Permissions {
                current_modes,
                octal,
                paths,
                ..
            } => {
                let modes = current_modes.clone();
                let octal = octal.clone();
                let mut grid = div().flex().flex_col().gap_1();
                for (label, bits) in [
                    ("Owner", [0o400, 0o200, 0o100]),
                    ("Group", [0o040, 0o020, 0o010]),
                    ("Other", [0o004, 0o002, 0o001]),
                ] {
                    let mut row = div().h_8().flex().items_center().gap_1().child(
                        div()
                            .w(px(54.0))
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child(label),
                    );
                    for (permission, bit) in ["Read", "Write", "Execute"].into_iter().zip(bits) {
                        let all = modes.iter().all(|mode| mode & bit != 0);
                        let none = modes.iter().all(|mode| mode & bit == 0);
                        let marker = if all {
                            "●"
                        } else if none {
                            "○"
                        } else {
                            "−"
                        };
                        row = row.child(
                            div()
                                .id(("permission-bit", bit as usize))
                                .with_focus_ring()
                                .flex_1()
                                .h_7()
                                .rounded_md()
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap_1()
                                .cursor_pointer()
                                .text_xs()
                                .bg(if all {
                                    rgb(accent_background())
                                } else {
                                    rgb(surface_elevated())
                                })
                                .hover(|style| style.bg(rgb(border())))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_permission_bit(bit, cx);
                                }))
                                .child(marker)
                                .child(permission),
                        );
                    }
                    grid = grid.child(row);
                }
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(grid)
                    .child(field_label("OCTAL MODE"))
                    .child(octal)
                    .child(helper_text(
                        "Non-recursive. Symlinks are excluded so their targets cannot be changed.",
                    ))
                    .into_any_element();
                (
                    "Permissions",
                    format!(
                        "{} item{}",
                        paths.len(),
                        if paths.len() == 1 { "" } else { "s" }
                    ),
                    body,
                )
            }
            OperationSheet::Rename { name, .. } => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(field_label("NEW NAME"))
                    .child(name.clone())
                    .child(helper_text("The item stays in its current folder."))
                    .into_any_element();
                ("Rename", "One item".to_owned(), body)
            }
            sheet @ OperationSheet::BulkRename {
                find,
                replace,
                prefix,
                suffix,
                start,
                padding,
                regex,
                numbering,
                scope,
                paths,
            } => {
                let preview = bulk_rename_preview(sheet, cx);
                let preview_rows = match &preview {
                    Ok(pairs) => pairs
                        .iter()
                        .take(6)
                        .map(|pair| {
                            div().text_xs().text_color(rgb(theme_text())).child(format!(
                                "{}  →  {}",
                                pair.from.file_name().unwrap_or_default().to_string_lossy(),
                                pair.to.file_name().unwrap_or_default().to_string_lossy()
                            ))
                        })
                        .collect::<Vec<_>>(),
                    Err(_) => Vec::new(),
                };
                let error = preview.err();
                let regex_enabled = *regex;
                let numbering_enabled = *numbering;
                let active_scope = *scope;
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(segmented_scope(active_scope, cx))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(find.clone())
                            .child(replace.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(prefix.clone())
                            .child(suffix.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(toggle_chip("regex", "Regex", regex_enabled, cx, |sheet| {
                                if let OperationSheet::BulkRename { regex, .. } = sheet {
                                    *regex = !*regex;
                                }
                            }))
                            .child(toggle_chip(
                                "numbering",
                                "Numbering",
                                numbering_enabled,
                                cx,
                                |sheet| {
                                    if let OperationSheet::BulkRename { numbering, .. } = sheet {
                                        *numbering = !*numbering;
                                    }
                                },
                            )),
                    )
                    .when(numbering_enabled, |view| {
                        view.child(
                            div()
                                .flex()
                                .gap_2()
                                .child(start.clone())
                                .child(padding.clone()),
                        )
                    })
                    .child(field_label("PREVIEW"))
                    .child(
                        div()
                            .p_2()
                            .rounded_md()
                            .bg(rgb(background()))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .when_some(error, |view, error| {
                                view.child(
                                    div().text_xs().text_color(rgb(error_color())).child(error),
                                )
                            })
                            .children(preview_rows),
                    )
                    .into_any_element();
                (
                    "Bulk rename",
                    format!("{} items · live preview", paths.len()),
                    body,
                )
            }
        };

        let footer = div()
            .h(px(58.0))
            .px_4()
            .flex()
            .items_center()
            .justify_end()
            .gap_2()
            .border_t_1()
            .border_color(rgb(border()))
            .when(read_only, |footer| {
                footer.child(sheet_button("Close", true).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.dismiss_sheet(&DismissSheet, window, cx);
                    },
                )))
            })
            .when(!read_only, |footer| {
                footer
                    .child(sheet_button("Cancel", false).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.dismiss_sheet(&DismissSheet, window, cx);
                        },
                    )))
                    .child(sheet_button("Apply", true).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.apply_sheet(&ApplySheet, window, cx);
                        },
                    )))
            });

        let modal_box = div()
            .w(px(520.0))
            .max_w(px(640.0))
            .max_h(px(580.0))
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
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_sm()
                                    .child(title),
                            )
                            .when(!subtitle.is_empty(), |view| {
                                view.child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(subtitle),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("dismiss-sheet")
                            .with_focus_ring()
                            .size_8()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss_sheet(&DismissSheet, window, cx);
                            }))
                            .child(ui_icon(
                                "icons/action-close.svg",
                                IconSize::Small,
                                IconTone::Muted,
                            )),
                    ),
            )
            .child(
                div()
                    .id("sheet-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_5()
                    .child(body),
            )
            .child(footer);
        let modal_box = animate_overlay(
            modal_box,
            if self.operation_sheet_closing {
                OverlayMotionState::Closing
            } else {
                OverlayMotionState::Opening
            },
            self.reduced_motion,
            3.0,
        );

        deferred(
            div()
                .id("sheet-modal-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .bg(Hsla::from(rgb(background())).opacity(0.6))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.dismiss_sheet(&DismissSheet, window, cx);
                    }),
                )
                .child(
                    div()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .child(modal_box),
                ),
        )
        .with_priority(MODAL_PRIORITY)
        .into_any_element()
    }

}
