impl FileManager {
    // Declarative GPUI trees are more legible kept together than split into state-free fragments.
    #[allow(clippy::too_many_lines)]
    fn render_header(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
            .child(self.render_file_search(cx))
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
                                |anchor| {
                                    anchor.child(self.render_header_action_menu(window, cx))
                                },
                            ),
                    )
                },
            )
            .into_any_element()
    }

    fn render_file_search(&self, cx: &mut Context<Self>) -> AnyElement {
        const FIELD_WIDTH: f32 = 252.0;
        let field = if self.file_search.open {
            self.render_file_search_open_field(cx)
        } else {
            Self::render_file_search_trigger(cx)
        };

        div()
            .w(px(FIELD_WIDTH))
            .h(px(34.0))
            .flex_none()
            .relative()
            .when(self.file_search.open, |field| {
                field.on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            })
            .child(field)
            .when(self.file_search.open, |field| {
                field.child(self.render_file_search_results(FIELD_WIDTH, cx))
            })
            .into_any_element()
    }

    fn render_file_search_open_field(&self, cx: &mut Context<Self>) -> AnyElement {
        let input = div()
            .size_full()
            .child(self.file_search.input.clone());
        let close = div()
            .id("file-search-close")
            .debug_selector(|| "file-search-close".into())
            .absolute()
            .right_1()
            .top(px(5.0))
            .size_6()
            .rounded_md()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(rgb(text_muted()))
            .hover(|style| {
                style
                    .bg(rgb(border()))
                    .text_color(rgb(text_emphasized()))
            })
            .occlude()
            .with_focus_ring()
            .on_click(cx.listener(|this, _, window, cx| {
                this.dismiss_file_search(&DismissFileSearch, window, cx);
            }))
            .child(ui_icon(
                "icons/action-close.svg",
                IconSize::Small,
                IconTone::Emphasized,
            ));
        let content = div()
            .debug_selector(|| "file-search-input-layer".into())
            .size_full()
            .relative()
            .text_color(rgb(text_muted()))
            .child(input)
            .child(close);
        self.animate_file_search_field(content)
    }

    fn render_file_search_trigger(cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("activate-file-search")
            .size_full()
            .px_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(border()))
            .bg(rgb(surface_elevated()))
            .flex()
            .items_center()
            .gap_2()
            .text_sm()
            .text_color(rgb(theme_text()))
            .cursor_pointer()
            .hover(|style| {
                style
                    .border_color(rgb(border_focused()))
                    .bg(rgb(surface()))
            })
            .with_focus_ring()
            .on_click(cx.listener(|this, _, window, cx| {
                this.activate_file_search(&ActivateFileSearch, window, cx);
            }))
            .child(ui_icon(
                "icons/action-search.svg",
                IconSize::Small,
                IconTone::Muted,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child("Search files"),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child("Ctrl+F"),
            )
            .into_any_element()
    }

    fn animate_file_search_field(&self, field: Div) -> AnyElement {
        if self.reduced_motion {
            field.into_any_element()
        } else if self.file_search.closing {
            field
                .with_animation(
                    "file-search-field-closing",
                    Animation::new(Duration::from_millis(80)).with_easing(gpui::quadratic),
                    |field, delta| field.opacity(1.0 - delta).right(px(2.0 * delta)),
                )
                .into_any_element()
        } else {
            field
                .with_animation(
                    "file-search-field-opening",
                    Animation::new(Duration::from_millis(120))
                        .with_easing(gpui::ease_out_quint()),
                    |field, delta| field.opacity(delta).right(px(2.0 * (1.0 - delta))),
                )
                .into_any_element()
        }
    }

    fn render_file_search_results(
        &self,
        field_width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panel = div()
            .w(px(420.0))
            .max_w_full()
            .max_h(px(480.0))
            .rounded_lg()
            .border_1()
            .border_color(rgb(border_focused()))
            .bg(rgb(surface()))
            .shadow_lg()
            .overflow_hidden()
            .occlude()
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h_10()
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(border()))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(text_emphasized()))
                            .child("Search"),
                    )
                    .child(self.render_file_search_scope_switch(cx))
                    .child(
                        div()
                            .flex_1()
                            .text_right()
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child(self.file_search_status()),
                    ),
            )
            .child(self.render_file_search_results_body(cx));
        let panel = self.animate_file_search_results(panel);

        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Local)
                .position(point(px(field_width), px(38.0)))
                .anchor(Corner::TopRight)
                .snap_to_window()
                .child(panel),
        )
        .with_priority(PATH_MENU_PRIORITY)
        .into_any_element()
    }

    fn render_file_search_scope_switch(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .h_7()
            .flex_none()
            .p(px(2.0))
            .rounded_md()
            .border_1()
            .border_color(rgb(border()))
            .bg(rgb(surface_elevated()))
            .flex()
            .items_center()
            .child(self.render_file_search_scope_option(
                FileSearchScope::CurrentFolder,
                "Current",
                self.tab.root != TabRoot::Trash,
                cx,
            ))
            .child(self.render_file_search_scope_option(
                FileSearchScope::Home,
                "Home",
                true,
                cx,
            ))
            .into_any_element()
    }

    fn render_file_search_scope_option(
        &self,
        scope: FileSearchScope,
        label: &'static str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.file_search.scope == scope;
        div()
            .id((
                "file-search-scope",
                match scope {
                    FileSearchScope::CurrentFolder => 0_usize,
                    FileSearchScope::Home => 1_usize,
                },
            ))
            .h_full()
            .px_2()
            .rounded_sm()
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(rgb(if !enabled {
                text_muted()
            } else if selected {
                accent()
            } else {
                theme_text()
            }))
            .when(selected, |option| option.bg(rgb(accent_background())))
            .when(enabled && !selected, |option| {
                option
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(border())))
                    .with_focus_ring()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.select_file_search_scope(scope, cx);
                    }))
            })
            .child(label)
            .into_any_element()
    }

    fn file_search_status(&self) -> String {
        if self.file_search.loading {
            "Searching…".to_owned()
        } else if self.file_search.results.len() == FILE_SEARCH_RESULT_LIMIT {
            format!("{FILE_SEARCH_RESULT_LIMIT}+ results")
        } else {
            format!("{} results", self.file_search.results.len())
        }
    }

    fn render_file_search_results_body(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.file_search.input.read(cx).text().trim().is_empty() {
            return div()
                .px_3()
                .py_3()
                .text_xs()
                .text_color(rgb(text_muted()))
                .child("Type a name to search files and folders recursively.")
                .into_any_element();
        }
        if let Some(error) = self.file_search.error.clone() {
            return div()
                .px_3()
                .py_3()
                .text_xs()
                .text_color(rgb(danger_color()))
                .child(error)
                .into_any_element();
        }
        if self.file_search.loading && self.file_search.results.is_empty() {
            let location = match self.file_search.scope {
                FileSearchScope::CurrentFolder => "this folder",
                FileSearchScope::Home => "Home",
            };
            return div()
                .px_3()
                .py_3()
                .text_xs()
                .text_color(rgb(text_muted()))
                .child(format!("Looking through {location}…"))
                .into_any_element();
        }
        if self.file_search.results.is_empty() {
            return div()
                .px_3()
                .py_3()
                .text_xs()
                .text_color(rgb(text_muted()))
                .child("No files or folders match this search.")
                .into_any_element();
        }
        self.file_search
            .results
            .iter()
            .take(12)
            .enumerate()
            .fold(div().w_full().flex().flex_col(), |list, (index, entry)| {
                list.child(self.render_file_search_result(index, entry, cx))
            })
            .into_any_element()
    }

    fn render_file_search_result(
        &self,
        index: usize,
        entry: &FileEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let focused = self.file_search.focused == Some(index);
        let relative = entry
            .path
            .strip_prefix(&self.file_search.root)
            .unwrap_or(&entry.path)
            .parent()
            .map_or_else(
                || match self.file_search.scope {
                    FileSearchScope::CurrentFolder => "This folder".to_owned(),
                    FileSearchScope::Home => "Home".to_owned(),
                },
                |path| path.display().to_string(),
            );
        div()
            .id(("file-search-result", index))
            .h_9()
            .w_full()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .text_sm()
            .when(focused, |row| row.bg(rgb(accent_background())))
            .hover(|row| row.bg(rgb(border())))
            .with_focus_ring()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.file_search.focused = Some(index);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.activate_file_search_result(index, window, cx);
            }))
            .child(file_icon(entry))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(rgb(theme_text()))
                    .child(entry.name.clone()),
            )
            .child(
                div()
                    .max_w(px(180.0))
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child(relative),
            )
            .into_any_element()
    }

    fn animate_file_search_results(&self, panel: Div) -> AnyElement {
        if self.reduced_motion {
            panel.into_any_element()
        } else if self.file_search.closing {
            panel
                .with_animation(
                    "file-search-results-closing",
                    Animation::new(Duration::from_millis(80)).with_easing(gpui::quadratic),
                    |panel, delta| panel.opacity(1.0 - delta).top(px(3.0 * delta)),
                )
                .into_any_element()
        } else {
            panel
                .with_animation(
                    "file-search-results-opening",
                    Animation::new(Duration::from_millis(120))
                        .with_easing(gpui::ease_out_quint()),
                    |panel, delta| panel.opacity(delta).top(px(3.0 * (1.0 - delta))),
                )
                .into_any_element()
        }
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
                .child(if is_trash {
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child("Trash")
                        .into_any_element()
                } else {
                    self.render_breadcrumb_segments(cx)
                })
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

    fn render_breadcrumb_segments(&self, cx: &mut Context<Self>) -> AnyElement {
        let current_path = self.tab.path.clone();
        visible_breadcrumb_segments(&current_path)
            .into_iter()
            .enumerate()
            .fold(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap_1(),
                |row, (position, (source_index, label, target))| {
                    let current = target.as_ref() == Some(&current_path);
                    let clickable = target.is_some() && !current;
                    let click_target = target.clone();
                    row.when(position > 0, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_color(rgb(text_muted()))
                                .child("›"),
                        )
                    })
                    .child(
                        div()
                            .id(("breadcrumb-segment", source_index))
                            .h_6()
                            .max_w(px(180.0))
                            .px_1()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .truncate()
                            .text_color(rgb(if target.is_none() {
                                text_muted()
                            } else if current {
                                text_emphasized()
                            } else {
                                theme_text()
                            }))
                            .when(clickable, |segment| {
                                segment
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(border())))
                                    .with_focus_ring()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        if let Some(target) = click_target.clone() {
                                            this.pending_reveal.clear();
                                            this.navigate_path(target);
                                            this.load_directory(cx);
                                        }
                                    }))
                            })
                            .child(label),
                    )
                },
            )
            .into_any_element()
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

fn breadcrumb_segments(path: &Path) -> Vec<(String, PathBuf)> {
    let mut current = PathBuf::new();
    path.components()
        .filter_map(|component| {
            let label = match component {
                std::path::Component::Prefix(prefix) => {
                    current.push(prefix.as_os_str());
                    prefix.as_os_str().to_string_lossy().into_owned()
                }
                std::path::Component::RootDir => {
                    current.push(Path::new("/"));
                    "/".to_owned()
                }
                std::path::Component::CurDir => return None,
                std::path::Component::ParentDir => {
                    current.push("..");
                    "..".to_owned()
                }
                std::path::Component::Normal(part) => {
                    current.push(part);
                    part.to_string_lossy().into_owned()
                }
            };
            Some((label, current.clone()))
        })
        .collect()
}

fn visible_breadcrumb_segments(path: &Path) -> Vec<(usize, String, Option<PathBuf>)> {
    let segments = breadcrumb_segments(path);
    if segments.len() <= 4 {
        return segments
            .into_iter()
            .enumerate()
            .map(|(index, (label, target))| (index, label, Some(target)))
            .collect();
    }

    let tail_start = segments.len() - 2;
    let mut visible = Vec::with_capacity(4);
    if let Some((label, target)) = segments.first().cloned() {
        visible.push((0, label, Some(target)));
    }
    visible.push((usize::MAX, "…".to_owned(), None));
    visible.extend(
        segments
            .into_iter()
            .enumerate()
            .skip(tail_start)
            .map(|(index, (label, target))| (index, label, Some(target))),
    );
    visible
}
