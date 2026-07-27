impl FileManager {
    // Declarative GPUI trees are more legible kept together than split into state-free fragments.
    #[allow(clippy::too_many_lines)]
    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let can_go_up = match &self.tab.root {
            TabRoot::Trash => false,
            TabRoot::Device { mount_root, .. } => self.tab.path != *mount_root,
            TabRoot::Directory => self.tab.path.parent().is_some(),
        };
        let button = |id: &'static str, icon: &'static str, enabled: bool| {
            div()
                .id(id)
                .size_8()
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .text_color(if enabled {
                    rgb(theme_text())
                } else {
                    rgb(border_focused())
                })
                .when(enabled, |style| {
                    style
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(border())))
                })
                .when(enabled, FocusableControl::with_focus_ring)
                .child(ui_icon(
                    icon,
                    IconSize::Small,
                    if enabled {
                        IconTone::Default
                    } else {
                        IconTone::Muted
                    },
                ))
        };
        div()
            .h(px(58.0))
            .w_full()
            .flex_none()
            .relative()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(rgb(border()))
            .child(
                button(
                    "back",
                    "icons/action-back.svg",
                    !self.tab.back_history.is_empty(),
                )
                .on_click(cx.listener(
                    |this, _, window, cx| {
                        this.go_back(&GoBack, window, cx);
                    },
                )),
            )
            .child(
                button(
                    "forward",
                    "icons/action-forward.svg",
                    !self.tab.forward_history.is_empty(),
                )
                .on_click(cx.listener(
                    |this, _, window, cx| {
                        this.go_forward(&GoForward, window, cx);
                    },
                )),
            )
            .child(
                button("up", "icons/action-up.svg", can_go_up).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.go_up(&GoUp, window, cx);
                    },
                )),
            )
            .child(
                div()
                    .ml_2()
                    .h(px(34.0))
                    .flex_1()
                    .min_w_0()
                    .relative()
                    .child(self.render_path_field(cx)),
            )
            .when(
                self.appearance_menu_open
                    || matches!(
                        self.action_menu.as_ref().map(|menu| menu.placement),
                        Some(ActionMenuPlacement::Header)
                    ),
                |header| {
                    header.child(
                        div()
                            .absolute()
                            .right_3()
                            .top(px(11.0))
                            .w(px(0.0))
                            .h(px(0.0))
                            .relative()
                            .when(self.appearance_menu_open, |anchor| {
                                anchor.child(self.render_appearance_menu(cx))
                            })
                            .when(
                                matches!(
                                    self.action_menu.as_ref().map(|menu| menu.placement),
                                    Some(ActionMenuPlacement::Header)
                                ),
                                |anchor| anchor.child(self.render_header_action_menu(cx)),
                            ),
                    )
                },
            )
            .into_any_element()
    }

    fn render_path_field(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.path_input.editing {
            let input = self.path_input.input.clone();
            let checking = self.path_input.checking;
            let field = div()
                .id("path-input-layer")
                .size_full()
                .relative()
                .child(input)
                .when(checking, |field| {
                    field.child(
                        div()
                            .absolute()
                            .right_2()
                            .top(px(8.0))
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child("Checking…"),
                    )
                })
                .when(
                    self.path_input.error.is_some() || !self.path_input.suggestions.is_empty(),
                    |field| field.child(self.render_path_feedback(cx)),
                );
            if self.reduced_motion {
                field.into_any_element()
            } else {
                field
                    .with_animation(
                        "path-input-enter",
                        Animation::new(Duration::from_millis(120))
                            .with_easing(gpui::ease_out_quint()),
                        |field, delta| field.opacity(delta).top(px(2.0 * (1.0 - delta))),
                    )
                    .into_any_element()
            }
        } else {
            let is_trash = self.tab.root == TabRoot::Trash;
            let breadcrumb = div()
                .id("breadcrumb-path")
                .size_full()
                .min_w_0()
                .rounded_md()
                .bg(rgb(surface_elevated()))
                .border_1()
                .border_color(rgb(border()))
                .px_3()
                .flex()
                .items_center()
                .gap_3()
                .text_sm()
                .text_color(rgb(theme_text()))
                .hover(|style| style.border_color(rgb(border_focused())))
                .when(!is_trash, |field| {
                    field
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.activate_path_input(&ActivatePathInput, window, cx);
                        }))
                })
                .child(div().flex_1().min_w_0().truncate().child(if is_trash {
                    "Trash".into()
                } else {
                    self.tab.path.display().to_string()
                }))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(rgb(text_muted()))
                        .child(if is_trash { "Virtual view" } else { "Ctrl+L" }),
                );
            if self.reduced_motion {
                breadcrumb.into_any_element()
            } else {
                breadcrumb
                    .with_animation(
                        "breadcrumb-enter",
                        Animation::new(Duration::from_millis(100))
                            .with_easing(gpui::ease_out_quint()),
                        gpui::Styled::opacity,
                    )
                    .into_any_element()
            }
        }
    }

    fn render_path_feedback(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = if let Some(error) = self.path_input.error.clone() {
            div()
                .w(px(420.0))
                .max_w_full()
                .px_3()
                .py_2()
                .rounded_lg()
                .border_1()
                .border_color(rgb(danger_color()))
                .bg(rgb(surface()))
                .shadow_lg()
                .text_xs()
                .text_color(rgb(danger_color()))
                .child(error)
                .into_any_element()
        } else {
            self.path_input
                .suggestions
                .iter()
                .take(8)
                .cloned()
                .enumerate()
                .fold(
                    div()
                        .w(px(420.0))
                        .max_w_full()
                        .p_1()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(border_focused()))
                        .bg(rgb(surface()))
                        .shadow_lg()
                        .text_xs()
                        .occlude()
                        .on_any_mouse_down(|_, _, cx| cx.stop_propagation()),
                    |menu, (index, suggestion)| {
                        let focused = self.path_input.focused_suggestion == Some(index);
                        let clicked = suggestion.clone();
                        menu.child(
                            div()
                                .id(("path-suggestion", index))
                                .h(px(30.0))
                                .w_full()
                                .px_2()
                                .rounded_md()
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_color(rgb(theme_text()))
                                .when(focused, |row| row.bg(rgb(border())))
                                .hover(|row| row.bg(rgb(border())))
                                .with_focus_ring()
                                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                    if *hovered {
                                        this.path_input.focus_suggestion(index);
                                        cx.notify();
                                    }
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.accept_path_suggestion(clicked.clone(), cx);
                                }))
                                .child(suggestion.label),
                        )
                    },
                )
                .into_any_element()
        };
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Local)
                .position(point(px(0.0), px(38.0)))
                .anchor(Corner::TopLeft)
                .snap_to_window()
                .child(content),
        )
        .with_priority(PATH_MENU_PRIORITY)
        .into_any_element()
    }

}
