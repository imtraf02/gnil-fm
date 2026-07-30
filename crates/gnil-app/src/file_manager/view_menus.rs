impl FileManager {
    fn render_header_action_menu(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Local)
                .position(point(px(80.0), px(36.0)))
                .anchor(Corner::TopRight)
                .snap_to_window_with_margin(px(MENU_VIEWPORT_MARGIN))
                .child(self.render_action_menu_panel(
                    ActionMenuPlacement::Header,
                    window,
                    cx,
                )),
        )
        .with_priority(MENU_PRIORITY)
        .into_any_element()
    }

    fn render_context_action_menu(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(ActionMenuPlacement::Cursor(position)) =
            self.action_menu.as_ref().map(|menu| menu.placement)
        else {
            return div().into_any_element();
        };
        deferred(
            anchored()
                .position(position)
                .anchor(Corner::TopLeft)
                .snap_to_window_with_margin(px(MENU_VIEWPORT_MARGIN))
                .child(self.render_action_menu_panel(
                    ActionMenuPlacement::Cursor(position),
                    window,
                    cx,
                )),
        )
        .with_priority(MENU_PRIORITY)
        .into_any_element()
    }

    fn render_empty_space_menu(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(menu) = self.empty_space_menu.as_ref() else {
            return div().into_any_element();
        };
        let position = menu.position;
        let animation = menu.animation;
        let viewport = window.viewport_size();
        let root_height = empty_space_panel_height(&menu.root_entries);
        let root_origin =
            snapped_menu_origin(position, EMPTY_SPACE_MENU_WIDTH, root_height, viewport);
        let submenu_top = menu.submenu.map_or(0.0, |submenu| {
            empty_space_submenu_top(&menu.root_entries, submenu)
        });
        let submenu_placement = menu.submenu.map(|_| {
            submenu_placement(
                root_origin,
                EMPTY_SPACE_MENU_WIDTH,
                submenu_top,
                EMPTY_SPACE_MENU_WIDTH,
                empty_space_panel_height(&menu.submenu_entries),
                viewport,
                None,
            )
        });
        let root =
            Self::render_empty_space_panel(menu.root_entries.clone(), menu.focused_root, false, cx);
        let submenu_bounds = submenu_placement.map(|placement| {
            submenu_panel_bounds(
                root_origin,
                EMPTY_SPACE_MENU_WIDTH,
                placement,
                EMPTY_SPACE_MENU_WIDTH,
                empty_space_panel_height(&menu.submenu_entries),
            )
        });
        let panels = div()
            .relative()
            .w(px(EMPTY_SPACE_MENU_WIDTH))
            .h(px(root_height))
            .child(root)
            .when_some(submenu_placement, |panels, placement| {
                panels.child(
                    self.render_empty_space_submenu_overlay(menu, root_origin, placement, cx),
                )
            })
            .on_mouse_down_out(cx.listener(
                move |this, event: &MouseDownEvent, _, cx| {
                    if submenu_bounds
                        .is_some_and(|bounds| bounds.contains(&event.position))
                    {
                        return;
                    }
                    this.dismiss_empty_space_menu(cx);
                },
            ));
        let panels = if self.reduced_motion {
            panels.into_any_element()
        } else {
            match animation {
                MenuAnimationState::Opening => panels
                    .with_animation(
                        "empty-space-menu-opening",
                        Animation::new(Duration::from_millis(120))
                            .with_easing(gpui::ease_out_quint()),
                        |panels, delta| panels.opacity(delta).top(px(3.0 * (1.0 - delta))),
                    )
                    .into_any_element(),
                MenuAnimationState::Closing => panels
                    .with_animation(
                        "empty-space-menu-closing",
                        Animation::new(Duration::from_millis(80)).with_easing(gpui::quadratic),
                        |panels, delta| panels.opacity(1.0 - delta).top(px(2.0 * delta)),
                    )
                    .into_any_element(),
            }
        };
        deferred(
            anchored()
                .position(position)
                .anchor(Corner::TopLeft)
                .snap_to_window_with_margin(px(MENU_VIEWPORT_MARGIN))
                .child(panels),
        )
        .with_priority(MENU_PRIORITY)
        .into_any_element()
    }

    fn render_empty_space_panel(
        entries: Vec<EmptySpaceMenuEntry>,
        focused: Option<usize>,
        is_submenu: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        entries.into_iter().enumerate().fold(
            div()
                .id(if is_submenu {
                    "empty-space-submenu"
                } else {
                    "empty-space-menu"
                })
                .debug_selector(move || {
                    if is_submenu {
                        "empty-space-submenu-panel".into()
                    } else {
                        "empty-space-menu-panel".into()
                    }
                })
                .w(px(EMPTY_SPACE_MENU_WIDTH))
                .p_1()
                .rounded_lg()
                .border_1()
                .border_color(rgb(border_focused()))
                .bg(rgb(surface()))
                .shadow_lg()
                .text_xs()
                .occlude()
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation()),
            |panel, (index, entry)| {
                Self::render_empty_space_entry(panel, index, entry, focused, is_submenu, cx)
            },
        )
    }

    fn render_empty_space_entry(
        panel: Stateful<Div>,
        index: usize,
        entry: EmptySpaceMenuEntry,
        focused: Option<usize>,
        is_submenu: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        match entry {
            EmptySpaceMenuEntry::Separator => panel.child(
                div()
                    .h(px(9.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .child(div().h(px(1.0)).w_full().bg(rgb(border()))),
            ),
            EmptySpaceMenuEntry::Action {
                command,
                label,
                shortcut,
                enabled,
                checked,
            } => panel.child(
                empty_space_menu_row(index, enabled, focused == Some(index))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if !*hovered {
                            return;
                        }
                        if let Some(menu) = this.empty_space_menu.as_mut() {
                            if is_submenu {
                                menu.focus_submenu(index);
                            } else {
                                menu.focus_root(index);
                                menu.close_submenu();
                            }
                            cx.notify();
                        }
                    }))
                    .when(enabled, |row| {
                        row.on_click(cx.listener(move |this, _, window, cx| {
                            this.dispatch_empty_space_command(command, window, cx);
                        }))
                    })
                    .child(label)
                    .child(
                        div()
                            .ml_4()
                            .text_color(if enabled {
                                rgb(text_muted())
                            } else {
                                rgb(border_focused())
                            })
                            .child(if checked {
                                "●"
                            } else {
                                shortcut.unwrap_or("")
                            }),
                    ),
            ),
            EmptySpaceMenuEntry::Submenu {
                submenu,
                label,
                enabled,
            } => panel.child(
                empty_space_menu_row(index, enabled, focused == Some(index))
                    .debug_selector(move || {
                        format!("empty-space-submenu-trigger-{submenu:?}")
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered && let Some(menu) = this.empty_space_menu.as_mut() {
                            menu.focus_root(index);
                            if enabled {
                                menu.open_submenu(submenu);
                            }
                            cx.notify();
                        }
                    }))
                    .when(enabled, |row| {
                        row.on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(menu) = this.empty_space_menu.as_mut() {
                                menu.open_submenu(submenu);
                                cx.notify();
                            }
                        }))
                    })
                    .child(label)
                    .child(div().ml_4().text_color(rgb(text_muted())).child("›")),
            ),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn render_action_menu_panel(
        &self,
        placement: ActionMenuPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(menu) = self.action_menu.as_ref() else {
            return div().into_any_element();
        };
        let focused = menu.focused;
        let animation = menu.animation;
        let toolbar_actions = menu.toolbar_actions.clone();
        let entries = menu.entries.clone();
        let viewport = window.viewport_size();
        let root_height = action_menu_panel_height(&entries);
        let desired_root_origin = match placement {
            ActionMenuPlacement::Header => point(
                px(f32::from(viewport.width) - MENU_VIEWPORT_MARGIN - ACTION_MENU_WIDTH),
                px(HEADER_ACTION_MENU_TOP),
            ),
            ActionMenuPlacement::Cursor(position) => position,
        };
        let root_origin =
            snapped_menu_origin(desired_root_origin, ACTION_MENU_WIDTH, root_height, viewport);
        let submenu_top = action_submenu_top(&entries, FileMenuSubmenu::OpenWith);
        let submenu_placement = menu.open_with_submenu.as_ref().map(|_| {
            submenu_placement(
                root_origin,
                ACTION_MENU_WIDTH,
                submenu_top,
                OPEN_WITH_SUBMENU_WIDTH,
                OPEN_WITH_SUBMENU_RESERVED_HEIGHT,
                viewport,
                matches!(placement, ActionMenuPlacement::Header).then_some(SubmenuSide::Left),
            )
        });
        let root_panel = entries.into_iter().enumerate().fold(
            div()
                .id("file-action-menu")
                .debug_selector(|| "file-action-menu-panel".into())
                .w(px(ACTION_MENU_WIDTH))
                .p_1()
                .relative()
                .rounded_lg()
                .border_1()
                .border_color(rgb(border_focused()))
                .bg(rgb(surface()))
                .shadow_lg()
                .text_xs()
                .occlude()
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .child(Self::render_action_menu_toolbar(
                    toolbar_actions,
                    focused,
                    cx,
                ))
                .child(
                    div()
                        .h(px(9.0))
                        .px_2()
                        .flex()
                        .items_center()
                        .child(div().h(px(1.0)).w_full().bg(rgb(border()))),
                ),
            |panel, (index, entry)| {
                Self::render_action_menu_entry(panel, index, entry, focused, cx)
            },
        );
        let submenu_height = menu
            .open_with_submenu
            .as_ref()
            .map(open_with_submenu_panel_height);
        let submenu_bounds = submenu_placement
            .zip(submenu_height)
            .map(|(placement, height)| {
                submenu_panel_bounds(
                    root_origin,
                    ACTION_MENU_WIDTH,
                    placement,
                    OPEN_WITH_SUBMENU_WIDTH,
                    height,
                )
            });
        let panels = div()
            .relative()
            .w(px(ACTION_MENU_WIDTH))
            .h(px(root_height))
            .child(root_panel)
            .when_some(submenu_placement, |panels, placement| {
                panels.child(self.render_action_submenu_overlay(root_origin, placement, cx))
            })
            .on_mouse_down_out(cx.listener(
                move |this, event: &MouseDownEvent, _, cx| {
                    if submenu_bounds
                        .is_some_and(|bounds| bounds.contains(&event.position))
                    {
                        return;
                    }
                    this.dismiss_action_menu(cx);
                },
            ));
        if self.reduced_motion {
            panels.into_any_element()
        } else {
            match animation {
                MenuAnimationState::Opening => panels
                    .with_animation(
                        "file-action-menu-opening",
                        Animation::new(Duration::from_millis(120))
                            .with_easing(gpui::ease_out_quint()),
                        |panels, delta| panels.opacity(delta).top(px(3.0 * (1.0 - delta))),
                    )
                    .into_any_element(),
                MenuAnimationState::Closing => panels
                    .with_animation(
                        "file-action-menu-closing",
                        Animation::new(Duration::from_millis(80)).with_easing(gpui::quadratic),
                        |panels, delta| panels.opacity(1.0 - delta).top(px(2.0 * delta)),
                    )
                    .into_any_element(),
            }
        }
    }

    fn render_action_menu_toolbar(
        actions: Vec<MenuToolbarAction>,
        focused: Option<MenuFocusTarget>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        actions.into_iter().enumerate().fold(
            div()
                .id("file-menu-toolbar")
                .h(px(44.0))
                .px_1()
                .flex()
                .items_center()
                .gap_1(),
            |toolbar, (index, action)| {
                let enabled = action.enabled;
                let command = action.command;
                toolbar.child(
                    div()
                        .id(("file-menu-toolbar-action", index))
                        .size_7()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(if enabled {
                            rgb(theme_text())
                        } else {
                            rgb(border_focused())
                        })
                        .when(
                            focused == Some(MenuFocusTarget::Toolbar(index)),
                            |button| button.bg(rgb(accent_background())),
                        )
                        .when(enabled, |button| {
                            button
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(border())))
                                .with_focus_ring()
                                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                    if *hovered
                                        && let Some(menu) = this.action_menu.as_mut()
                                    {
                                        menu.focus_toolbar(index);
                                        cx.notify();
                                    }
                                }))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.dispatch_menu_command(command, window, cx);
                                }))
                        })
                        .child(ui_icon(
                            action_menu_icon(command),
                            IconSize::Compact,
                            if enabled {
                                IconTone::Default
                            } else {
                                IconTone::Muted
                            },
                        )),
                )
            },
        )
    }

    #[allow(clippy::too_many_lines)]
    fn render_action_menu_entry(
        panel: Stateful<Div>,
        index: usize,
        entry: MenuEntry,
        focused: Option<MenuFocusTarget>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        match entry {
            MenuEntry::Separator => panel.child(
                div()
                    .h(px(9.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .child(div().h(px(1.0)).w_full().bg(rgb(border()))),
            ),
            MenuEntry::Action {
                command,
                label,
                shortcut,
                enabled,
                danger,
            } => panel.child(
                div()
                    .id(("file-menu-entry", index))
                    .h(px(36.0))
                    .w_full()
                    .px_2()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(if enabled {
                        if danger {
                            rgb(danger_color())
                        } else {
                            rgb(theme_text())
                        }
                    } else {
                        rgb(border_focused())
                    })
                    .when(
                        focused == Some(MenuFocusTarget::Entry(index)),
                        |row| row.bg(rgb(accent_background())),
                    )
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .with_focus_ring()
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered && let Some(menu) = this.action_menu.as_mut() {
                                    menu.focus_entry(index);
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.dispatch_menu_command(command, window, cx);
                            }))
                    })
                    .child(ui_icon(
                        action_menu_icon(command),
                        IconSize::Compact,
                        if enabled {
                            if danger {
                                IconTone::Danger
                            } else {
                                IconTone::Default
                            }
                        } else {
                            IconTone::Muted
                        },
                    ))
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .when_some(shortcut, |row, shortcut| {
                        row.child(
                            div()
                                .text_color(if enabled {
                                    rgb(text_muted())
                                } else {
                                    rgb(border_focused())
                                })
                                .child(shortcut),
                        )
                    }),
            ),
            MenuEntry::Submenu {
                submenu,
                label,
                enabled,
            } => panel.child(
                div()
                    .id(("file-menu-entry", index))
                    .debug_selector(move || format!("file-submenu-trigger-{submenu:?}"))
                    .h(px(36.0))
                    .w_full()
                    .px_2()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(if enabled {
                        rgb(theme_text())
                    } else {
                        rgb(border_focused())
                    })
                    .when(
                        focused == Some(MenuFocusTarget::Entry(index)),
                        |row| row.bg(rgb(accent_background())),
                    )
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .with_focus_ring()
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if !*hovered {
                                    return;
                                }
                                if let Some(menu) = this.action_menu.as_mut() {
                                    menu.focus_entry(index);
                                }
                                if submenu == FileMenuSubmenu::OpenWith
                                    && this
                                        .action_menu
                                        .as_ref()
                                        .is_some_and(|menu| menu.open_with_submenu.is_none())
                                {
                                    this.show_open_with_submenu(cx);
                                } else {
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if submenu == FileMenuSubmenu::OpenWith {
                                    this.show_open_with_submenu(cx);
                                }
                            }))
                    })
                    .child(ui_icon(
                        "icons/action-open-with.svg",
                        IconSize::Compact,
                        if enabled {
                            IconTone::Default
                        } else {
                            IconTone::Muted
                        },
                    ))
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .child(
                        div()
                            .text_color(if enabled {
                                rgb(text_muted())
                            } else {
                                rgb(border_focused())
                            })
                            .child("›"),
                    ),
            ),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn render_open_with_submenu(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let Some(submenu) = self
            .action_menu
            .as_ref()
            .and_then(|menu| menu.open_with_submenu.as_ref())
        else {
            return div().id("open-with-submenu");
        };
        let applications = submenu.applications.clone();
        let focused = submenu.focused;
        let loading = submenu.loading;
        let error = submenu.error.clone();
        let mut panel = div()
            .id("open-with-submenu")
            .debug_selector(|| "open-with-submenu-panel".into())
            .w(px(280.0))
            .p_1()
            .rounded_lg()
            .border_1()
            .border_color(rgb(border_focused()))
            .bg(rgb(surface()))
            .shadow_lg()
            .text_xs()
            .occlude()
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation());

        if loading {
            panel = panel.child(
                div()
                    .h(px(36.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(rgb(text_muted()))
                    .child(ui_icon(
                        "icons/action-open-with.svg",
                        IconSize::Compact,
                        IconTone::Muted,
                    ))
                    .child("Finding compatible apps…"),
            );
        } else if let Some(error) = error {
            panel = panel.child(
                div()
                    .min_h(px(36.0))
                    .px_2()
                    .py_2()
                    .text_color(rgb(error_color()))
                    .child(error),
            );
        } else if applications.is_empty() {
            panel = panel.child(
                div()
                    .h(px(36.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .text_color(rgb(text_muted()))
                    .child("No compatible apps found"),
            );
        }

        for (index, application) in applications.into_iter().enumerate() {
            let app_for_click = application.clone();
            panel = panel.child(
                div()
                    .id(("open-with-app", index))
                    .h(px(36.0))
                    .w_full()
                    .px_2()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .text_sm()
                    .hover(|style| style.bg(rgb(border())))
                    .when(focused == Some(index), |row| {
                        row.bg(rgb(accent_background()))
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered
                            && let Some(menu) = this.action_menu.as_mut()
                        {
                            menu.focus_open_with(index);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.launch_with_application(
                            app_for_click.clone(),
                            false,
                            window,
                            cx,
                        );
                    }))
                    .child(ui_icon(
                        "icons/action-open-with.svg",
                        IconSize::Compact,
                        IconTone::Default,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(application.name),
                    )
                    .when(application.is_default, |row| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(rgb(text_muted()))
                                .child("Default"),
                        )
                    }),
            );
        }

        let choose_index = submenu.applications.len();
        panel
            .child(
                div()
                    .h(px(9.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .child(div().h(px(1.0)).w_full().bg(rgb(border()))),
            )
            .child(
                div()
                    .id("choose-another-app")
                    .h(px(36.0))
                    .w_full()
                    .px_2()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .text_sm()
                    .hover(|style| style.bg(rgb(border())))
                    .when(focused == Some(choose_index), |row| {
                        row.bg(rgb(accent_background()))
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered
                            && let Some(menu) = this.action_menu.as_mut()
                        {
                            menu.focus_open_with(choose_index);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let target = this.action_menu.as_ref().and_then(|menu| {
                            menu.open_with_submenu.as_ref().map(|submenu| {
                                (submenu.path.clone(), submenu.mime_type.clone())
                            })
                        });
                        if let Some((path, mime_type)) = target {
                            this.open_open_with_chooser(path, mime_type, window, cx);
                        }
                    }))
                    .child(ui_icon(
                        "icons/action-open-with.svg",
                        IconSize::Compact,
                        IconTone::Default,
                    ))
                    .child("Choose another app…"),
            )
    }
}

const fn action_menu_icon(command: FileMenuCommand) -> &'static str {
    match command {
        FileMenuCommand::Open => "icons/folder-open.svg",
        FileMenuCommand::Extract | FileMenuCommand::ExtractTo => "icons/file-archive.svg",
        FileMenuCommand::Copy
        | FileMenuCommand::CopyPathAbsolute
        | FileMenuCommand::CopyPathRelative => "icons/action-copy.svg",
        FileMenuCommand::Cut => "icons/action-cut.svg",
        FileMenuCommand::Paste => "icons/action-paste.svg",
        FileMenuCommand::Rename => "icons/action-rename.svg",
        FileMenuCommand::ToggleFavorite => "icons/action-star.svg",
        FileMenuCommand::CreateSymlink => "icons/action-link.svg",
        FileMenuCommand::Permissions => "icons/action-permissions.svg",
        FileMenuCommand::Trash | FileMenuCommand::DeletePermanently => "icons/trash.svg",
    }
}
