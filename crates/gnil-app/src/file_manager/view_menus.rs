impl FileManager {
    fn render_header_action_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Local)
                .position(point(px(80.0), px(36.0)))
                .anchor(Corner::TopRight)
                .snap_to_window()
                .child(self.render_action_menu_panel(cx)),
        )
        .with_priority(MENU_PRIORITY)
        .into_any_element()
    }

    fn render_context_action_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(ActionMenuPlacement::Cursor(position)) =
            self.action_menu.as_ref().map(|menu| menu.placement)
        else {
            return div().into_any_element();
        };
        deferred(
            anchored()
                .position(position)
                .anchor(Corner::TopLeft)
                .snap_to_window()
                .child(self.render_action_menu_panel(cx)),
        )
        .with_priority(MENU_PRIORITY)
        .into_any_element()
    }

    fn render_context_menu_backdrop(cx: &mut Context<Self>) -> AnyElement {
        deferred(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .on_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    if this.appearance_menu_open {
                        this.dismiss_appearance_menu(cx);
                    } else if this.empty_space_menu.is_some() {
                        this.dismiss_empty_space_menu(cx);
                    } else {
                        this.dismiss_action_menu(cx);
                    }
                })),
        )
        .with_priority(BACKDROP_PRIORITY)
        .into_any_element()
    }

    fn render_empty_space_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(menu) = self.empty_space_menu.as_ref() else {
            return div().into_any_element();
        };
        let position = menu.position;
        let animation = menu.animation;
        let root =
            Self::render_empty_space_panel(menu.root_entries.clone(), menu.focused_root, false, cx);
        let panels = div()
            .relative()
            .flex()
            .items_start()
            .gap_1()
            .child(root)
            .when(menu.submenu.is_some(), |panels| {
                let submenu = Self::render_empty_space_panel(
                    menu.submenu_entries.clone(),
                    menu.focused_submenu,
                    true,
                    cx,
                );
                if self.reduced_motion {
                    panels.child(submenu)
                } else {
                    panels.child(
                        submenu.with_animation(
                            "empty-space-submenu-opening",
                            Animation::new(Duration::from_millis(100))
                                .with_easing(gpui::ease_out_quint()),
                            |panel, delta| panel.opacity(delta).left(px(3.0 * (1.0 - delta))),
                        ),
                    )
                }
            });
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
                .snap_to_window()
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
                .w(px(264.0))
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

    fn render_action_menu_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(menu) = self.action_menu.as_ref() else {
            return div().into_any_element();
        };
        let focused = menu.focused;
        let animation = menu.animation;
        let entries = menu.entries.clone();
        let panel = entries.into_iter().enumerate().fold(
            div()
                .id("file-action-menu")
                .w(px(264.0))
                .p_1()
                .relative()
                .rounded_lg()
                .border_1()
                .border_color(rgb(border_focused()))
                .bg(rgb(surface()))
                .shadow_lg()
                .text_xs()
                .occlude()
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation()),
            |panel, (index, entry)| {
                Self::render_action_menu_entry(panel, index, entry, focused, cx)
            },
        );
        match animation {
            MenuAnimationState::Opening => panel
                .with_animation(
                    "file-action-menu-opening",
                    Animation::new(Duration::from_millis(120)).with_easing(gpui::ease_out_quint()),
                    |panel, delta| panel.opacity(delta).top(px(3.0 * (1.0 - delta))),
                )
                .into_any_element(),
            MenuAnimationState::Closing => panel
                .with_animation(
                    "file-action-menu-closing",
                    Animation::new(Duration::from_millis(80)).with_easing(gpui::quadratic),
                    |panel, delta| panel.opacity(1.0 - delta).top(px(2.0 * delta)),
                )
                .into_any_element(),
        }
    }

    fn render_action_menu_entry(
        panel: Stateful<Div>,
        index: usize,
        entry: MenuEntry,
        focused: Option<usize>,
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
                    .h_8()
                    .w_full()
                    .px_2()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_color(if enabled {
                        if danger {
                            rgb(danger_color())
                        } else {
                            rgb(theme_text())
                        }
                    } else {
                        rgb(border_focused())
                    })
                    .when(focused == Some(index), |row| row.bg(rgb(border())))
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .with_focus_ring()
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered && let Some(menu) = this.action_menu.as_mut() {
                                    menu.focus(index);
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.dispatch_menu_command(command, window, cx);
                            }))
                    })
                    .child(label)
                    .when_some(shortcut, |row, shortcut| {
                        row.child(
                            div()
                                .ml_4()
                                .text_color(if enabled {
                                    rgb(text_muted())
                                } else {
                                    rgb(border_focused())
                                })
                                .child(shortcut),
                        )
                    }),
            ),
        }
    }
}
