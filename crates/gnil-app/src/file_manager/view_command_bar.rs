impl FileManager {
    #[allow(clippy::too_many_lines)]
    fn render_command_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut content_width = f32::from(window.viewport_size().width) - 218.0;
        if self.preview_visible {
            content_width -= 318.0;
        }
        self.command_bar_width_tier = if content_width < 460.0 {
            CommandBarWidthTier::Narrow
        } else if content_width < 720.0 {
            CommandBarWidthTier::Compact
        } else {
            CommandBarWidthTier::Full
        };
        let labels = self.command_bar_width_tier == CommandBarWidthTier::Full;
        let has_selection = self.selection.selected_count() > 0;
        let writes_enabled = !self.operation_running;
        let clipboard_valid = self.file_clipboard_from_system(cx).is_some();

        let mut bar = div()
            .id("command-bar")
            .relative()
            .h(px(44.0))
            .w_full()
            .flex_none()
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(rgb(border()))
            .bg(rgb(surface()));

        if self.tab.root == TabRoot::Trash {
            bar = bar
                .child(
                    command_bar_button(
                        "command-restore",
                        "Restore",
                        "icons/action-restore.svg",
                        true,
                        has_selection && writes_enabled,
                        false,
                        cx,
                    )
                    .when(has_selection && writes_enabled, |button| {
                        button.on_click(cx.listener(|this, _, window, cx| {
                            this.restore_trash_selected(&RestoreTrashSelected, window, cx);
                        }))
                    }),
                )
                .child(
                    command_bar_button(
                        "command-delete-permanently",
                        "Delete permanently",
                        "icons/trash.svg",
                        labels,
                        has_selection && writes_enabled,
                        true,
                        cx,
                    )
                    .when(has_selection && writes_enabled, |button| {
                        button.on_click(cx.listener(|this, _, window, cx| {
                            this.purge_selected_trash(window, cx);
                        }))
                    }),
                )
                .child(
                    command_bar_button(
                        "command-empty-trash",
                        "Empty Trash",
                        "icons/trash.svg",
                        labels,
                        !self.trash_entries.is_empty() && writes_enabled,
                        true,
                        cx,
                    )
                    .when(!self.trash_entries.is_empty() && writes_enabled, |button| {
                        button.on_click(cx.listener(|this, _, window, cx| {
                            this.empty_trash(&EmptyTrash, window, cx);
                        }))
                    }),
                );
        } else {
            bar = bar
                .child(self.render_command_bar_menu_button(
                    CommandBarMenuKind::New,
                    "command-new",
                    "New",
                    "icons/action-new.svg",
                    labels,
                    writes_enabled,
                    cx,
                ))
                .child(command_bar_separator())
                .when(
                    self.command_bar_width_tier != CommandBarWidthTier::Narrow,
                    |bar| {
                        bar.child(
                            command_bar_button(
                                "command-cut",
                                "Cut",
                                "icons/action-cut.svg",
                                false,
                                has_selection,
                                false,
                                cx,
                            )
                            .when(has_selection, |button| {
                                button.on_click(cx.listener(|this, _, window, cx| {
                                    this.cut_selected(&CutSelected, window, cx);
                                }))
                            }),
                        )
                        .child(
                            command_bar_button(
                                "command-copy",
                                "Copy",
                                "icons/action-copy.svg",
                                false,
                                has_selection,
                                false,
                                cx,
                            )
                            .when(has_selection, |button| {
                                button.on_click(cx.listener(|this, _, window, cx| {
                                    this.copy_selected(&CopySelected, window, cx);
                                }))
                            }),
                        )
                    },
                )
                .child(
                    command_bar_button(
                        "command-paste",
                        "Paste",
                        "icons/action-paste.svg",
                        false,
                        clipboard_valid && writes_enabled,
                        false,
                        cx,
                    )
                    .when(clipboard_valid && writes_enabled, |button| {
                        button.on_click(cx.listener(|this, _, window, cx| {
                            this.paste(&Paste, window, cx);
                        }))
                    }),
                )
                .when(
                    self.command_bar_width_tier == CommandBarWidthTier::Full,
                    |bar| {
                        bar.child(
                            command_bar_button(
                                "command-rename",
                                "Rename",
                                "icons/action-rename.svg",
                                false,
                                has_selection && writes_enabled,
                                false,
                                cx,
                            )
                            .when(has_selection && writes_enabled, |button| {
                                button.on_click(cx.listener(|this, _, window, cx| {
                                    this.open_rename(&OpenRename, window, cx);
                                }))
                            }),
                        )
                        .child(
                            command_bar_button(
                                "command-trash",
                                "Move to Trash",
                                "icons/trash.svg",
                                false,
                                has_selection && writes_enabled,
                                true,
                                cx,
                            )
                            .when(has_selection && writes_enabled, |button| {
                                button.on_click(cx.listener(|this, _, window, cx| {
                                    this.trash_selected(&TrashSelected, window, cx);
                                }))
                            }),
                        )
                    },
                );
        }

        bar.child(command_bar_separator())
            .child(self.render_command_bar_menu_button(
                CommandBarMenuKind::Sort,
                "command-sort",
                "Sort by",
                "icons/action-sort.svg",
                labels,
                true,
                cx,
            ))
            .child(self.render_command_bar_menu_button(
                CommandBarMenuKind::Layout,
                "command-layout",
                "Layout",
                "icons/action-layout-grid.svg",
                labels,
                true,
                cx,
            ))
            .child(self.render_command_bar_menu_button(
                CommandBarMenuKind::More,
                "command-more",
                "More",
                "icons/action-more.svg",
                false,
                self.tab.root != TabRoot::Trash || has_selection,
                cx,
            ))
            .child(div().flex_1().min_w_0())
            .child(self.render_preview_panel_button(cx))
            .into_any_element()
    }

    fn render_preview_panel_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let visible = self.preview_visible;
        div()
            .id("command-preview-panel")
            .debug_selector(|| "preview-panel-toggle".into())
            .size_8()
            .flex_none()
            .rounded_md()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .when(visible, |button| button.bg(rgb(accent_background())))
            .hover(move |style| {
                style.bg(rgb(if visible { accent_hover() } else { border() }))
            })
            .with_focus_ring()
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_preview(&TogglePreview, window, cx);
            }))
            .child(ui_icon(
                "icons/action-panel-right.svg",
                IconSize::Compact,
                if visible {
                    IconTone::Accent
                } else {
                    IconTone::Default
                },
            ))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_command_bar_menu_button(
        &self,
        kind: CommandBarMenuKind,
        id: &'static str,
        label: &'static str,
        icon: &'static str,
        show_label: bool,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self
            .command_bar_menu
            .as_ref()
            .is_some_and(|menu| menu.kind == kind);
        let button = command_bar_button(id, label, icon, show_label, enabled, false, cx)
            .debug_selector(move || format!("command-bar-menu-trigger-{kind:?}"))
            .when(open, |button| button.bg(rgb(accent_background())))
            .when(enabled, |button| {
                button
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_command_bar_menu(kind, cx);
                    }))
            });
        div()
            .relative()
            .flex_none()
            .child(button)
            .when(open, |button| {
                button.child(self.render_command_bar_menu(kind, cx))
            })
            .into_any_element()
    }

    #[allow(clippy::too_many_lines)]
    fn render_command_bar_menu(
        &self,
        kind: CommandBarMenuKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self
            .command_bar_menu
            .as_ref()
            .filter(|state| state.kind == kind)
        else {
            return div().into_any_element();
        };
        let items = self.command_bar_menu_items(kind);
        let panel = items.into_iter().enumerate().fold(
            div()
                .id(("command-bar-menu", kind as usize))
                .debug_selector(move || format!("command-bar-menu-panel-{kind:?}"))
                .w(px(272.0))
                .p_1()
                .rounded_lg()
                .border_1()
                .border_color(rgb(border_focused()))
                .bg(rgb(surface()))
                .shadow_lg()
                .text_xs()
                .occlude()
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.dismiss_command_bar_menu(cx);
                })),
            |panel, (index, item)| {
                let add_separator = matches!(kind, CommandBarMenuKind::Sort) && index == 4;
                panel
                    .when(add_separator, |panel| {
                        panel.child(
                            div()
                                .h(px(9.0))
                                .px_2()
                                .flex()
                                .items_center()
                                .child(div().h(px(1.0)).w_full().bg(rgb(border()))),
                        )
                    })
                    .child(
                        div()
                            .id(("command-bar-menu-item", index))
                            .h(px(36.0))
                            .w_full()
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .text_color(if !item.enabled {
                                rgb(border_focused())
                            } else if item.danger {
                                rgb(danger_color())
                            } else {
                                rgb(theme_text())
                            })
                            .when(state.focused == Some(index), |row| {
                                row.bg(rgb(accent_background()))
                            })
                            .when(item.enabled, |row| {
                                row.cursor_pointer()
                                    .hover(|style| style.bg(rgb(border())))
                                    .with_focus_ring()
                                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                        if *hovered
                                            && let Some(menu) = this.command_bar_menu.as_mut()
                                        {
                                            menu.focused = Some(index);
                                            this.invalidate_surfaces(
                                                SurfaceMask::COMMAND_BAR,
                                                cx,
                                            );
                                        }
                                    }))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.dispatch_command_bar_command(item.command, window, cx);
                                    }))
                            })
                            .child(
                                div()
                                    .debug_selector(move || {
                                        format!("command-bar-menu-label-{kind:?}-{index}")
                                    })
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(item.label),
                            )
                            .child(
                                div()
                                    .text_color(rgb(text_muted()))
                                    .child(if item.checked {
                                        "✓"
                                    } else {
                                        item.shortcut.unwrap_or("")
                                    }),
                            ),
                    )
            },
        );
        let panel = if self.reduced_motion {
            panel.into_any_element()
        } else {
            match state.animation {
                MenuAnimationState::Opening => panel
                    .with_animation(
                        "command-bar-menu-opening",
                        Animation::new(Duration::from_millis(120))
                            .with_easing(gpui::ease_out_quint()),
                        |panel, delta| panel.opacity(delta).top(px(3.0 * (1.0 - delta))),
                    )
                    .into_any_element(),
                MenuAnimationState::Closing => panel
                    .with_animation(
                        "command-bar-menu-closing",
                        Animation::new(Duration::from_millis(80)).with_easing(gpui::quadratic),
                        |panel, delta| panel.opacity(1.0 - delta).top(px(2.0 * delta)),
                    )
                    .into_any_element(),
            }
        };
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Local)
                .position(point(px(-5.0), px(0.0)))
                .anchor(Corner::TopLeft)
                .snap_to_window_with_margin(px(MENU_VIEWPORT_MARGIN))
                .child(panel),
        )
        .with_priority(MENU_PRIORITY)
        .into_any_element()
    }
}

fn command_bar_separator() -> AnyElement {
    div()
        .h_6()
        .w(px(1.0))
        .mx_1()
        .flex_none()
        .bg(rgb(border()))
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn command_bar_button(
    id: &'static str,
    label: &'static str,
    icon: &'static str,
    show_label: bool,
    enabled: bool,
    danger: bool,
    _cx: &mut Context<FileManager>,
) -> Stateful<Div> {
    div()
        .id(id)
        .h_8()
        .min_w(px(32.0))
        .px_2()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .text_xs()
        .text_color(if !enabled {
            rgb(border_focused())
        } else if danger {
            rgb(danger_color())
        } else {
            rgb(theme_text())
        })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| style.bg(rgb(border())))
                .active(|style| style.bg(rgb(accent_background())))
                .with_focus_ring()
        })
        .child(ui_icon(
            icon,
            IconSize::Compact,
            if !enabled {
                IconTone::Muted
            } else if danger {
                IconTone::Danger
            } else {
                IconTone::Default
            },
        ))
        .when(show_label, |button| button.child(label))
}
