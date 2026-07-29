impl FileManager {
    #[allow(clippy::too_many_lines)]
    fn render_open_with_chooser(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(chooser) = self.open_with_chooser.as_ref() else {
            return div().into_any_element();
        };
        let search = chooser.search.clone();
        let query = search.read(cx).text().to_owned();
        let selected = chooser.selected_desktop_id.clone();
        let loading = chooser.loading;
        let error = chooser.error.clone();
        let always_use = chooser.always_use;
        let default_supported = chooser.mime_type != "application/octet-stream";
        let file_name = chooser.file_name.clone();
        let mime_type = chooser.mime_type.clone();
        let motion = chooser.motion;
        let (suggested, all) = chooser.catalog.as_ref().map_or_else(
            || (Vec::new(), Vec::new()),
            |catalog| {
                if query.trim().is_empty() {
                    let suggested = catalog.suggested.clone();
                    let suggested_ids = suggested
                        .iter()
                        .map(|application| application.desktop_id.as_str())
                        .collect::<HashSet<_>>();
                    let all = catalog
                        .all
                        .iter()
                        .filter(|application| !suggested_ids.contains(application.desktop_id.as_str()))
                        .cloned()
                        .collect();
                    (suggested, all)
                } else {
                    let mut seen = HashSet::new();
                    let applications = catalog
                        .suggested
                        .iter()
                        .chain(&catalog.all)
                        .filter(|application| seen.insert(application.desktop_id.clone()))
                        .cloned()
                        .collect::<Vec<_>>();
                    (
                        Vec::new(),
                        filter_applications(&applications, &query)
                            .into_iter()
                            .cloned()
                            .collect(),
                    )
                }
            },
        );
        let result_count = suggested.len() + all.len();
        let can_open = selected.is_some() && !loading;

        let mut application_list = div()
            .id("open-with-applications")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_1();
        if !suggested.is_empty() {
            application_list =
                application_list.child(open_with_section_label("SUGGESTED"));
            for application in suggested {
                application_list = application_list.child(Self::open_with_application_row(
                    application,
                    selected.as_deref(),
                    cx,
                ));
            }
        }
        if !all.is_empty() {
            application_list =
                application_list.child(open_with_section_label("ALL APPLICATIONS"));
            for application in all {
                application_list = application_list.child(Self::open_with_application_row(
                    application,
                    selected.as_deref(),
                    cx,
                ));
            }
        }
        if loading {
            application_list = application_list.child(
                div()
                    .h(px(120.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child("Finding installed applications…"),
            );
        } else if result_count == 0 {
            application_list = application_list.child(
                div()
                    .h(px(120.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child(if query.trim().is_empty() {
                        "No applications are available"
                    } else {
                        "No applications match your search"
                    }),
            );
        }

        let modal = div()
            .w(px(620.0))
            .max_w(px(680.0))
            .h(px(560.0))
            .max_h(px(620.0))
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
                    .h(px(58.0))
                    .px_5()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(border()))
                    .child(
                        div()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_sm()
                                    .child("Open with"),
                            )
                            .child(
                                div()
                                    .max_w(px(500.0))
                                    .truncate()
                                    .text_xs()
                                    .text_color(rgb(text_muted()))
                                    .child(format!("{file_name} · {mime_type}")),
                            ),
                    )
                    .child(
                        div()
                            .id("dismiss-open-with")
                            .with_focus_ring()
                            .size_8()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss_open_with(&DismissOpenWith, window, cx);
                            }))
                            .child(ui_icon(
                                "icons/action-close.svg",
                                IconSize::Small,
                                IconTone::Muted,
                            )),
                    ),
            )
            .child(div().px_4().pt_4().child(search))
            .when_some(error, |modal, error| {
                modal.child(
                    div()
                        .mx_4()
                        .mt_2()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(Hsla::from(rgb(error_color())).opacity(0.1))
                        .text_xs()
                        .text_color(rgb(error_color()))
                        .child(error),
                )
            })
            .child(application_list)
            .child(
                div()
                    .min_h(px(70.0))
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(border()))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .id("always-use-open-with")
                            .when(default_supported, FocusableControl::with_focus_ring)
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(if default_supported {
                                rgb(theme_text())
                            } else {
                                rgb(text_muted())
                            })
                            .when(default_supported, |toggle| {
                                toggle
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_open_with_default(
                                            &ToggleOpenWithDefault,
                                            window,
                                            cx,
                                        );
                                    }))
                            })
                            .child(setting_switch(always_use && default_supported))
                            .child(
                                div()
                                    .child("Always use this app")
                                    .when(!default_supported, |label| {
                                        label.child(
                                            div()
                                                .text_color(rgb(text_muted()))
                                                .child("Unavailable for unknown file types"),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                open_with_button("Cancel", false, true).on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.dismiss_open_with(
                                            &DismissOpenWith,
                                            window,
                                            cx,
                                        );
                                    },
                                )),
                            )
                            .child(open_with_button("Open", true, can_open).when(
                                can_open,
                                |button| {
                                    button.on_click(cx.listener(|this, _, window, cx| {
                                        this.confirm_open_with(
                                            &ConfirmOpenWith,
                                            window,
                                            cx,
                                        );
                                    }))
                                },
                            )),
                    ),
            );
        let modal = animate_overlay(modal, motion, self.reduced_motion, 3.0);

        deferred(
            div()
                .id("open-with-backdrop")
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
                        this.dismiss_open_with(&DismissOpenWith, window, cx);
                    }),
                )
                .child(
                    div()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(modal),
                ),
        )
        .with_priority(MODAL_PRIORITY)
        .into_any_element()
    }

    fn open_with_application_row(
        application: DesktopApplication,
        selected: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let desktop_id = application.desktop_id.clone();
        let is_selected = selected == Some(desktop_id.as_str());
        div()
            .id(SharedString::from(format!(
                "open-with-choice-{desktop_id}"
            )))
            .min_h(px(44.0))
            .w_full()
            .px_3()
            .py_2()
            .rounded_md()
            .flex()
            .items_center()
            .gap_3()
            .cursor_pointer()
            .border_1()
            .border_color(if is_selected {
                rgb(accent())
            } else {
                gpui::rgba(0x0000_0000)
            })
            .bg(if is_selected {
                rgb(accent_background())
            } else {
                gpui::rgba(0x0000_0000)
            })
            .hover(|style| style.bg(rgb(border())))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(chooser) = this.open_with_chooser.as_mut() {
                    chooser.selected_desktop_id = Some(desktop_id.clone());
                    cx.notify();
                }
            }))
            .child(ui_icon(
                "icons/action-open-with.svg",
                IconSize::Standard,
                if is_selected {
                    IconTone::Accent
                } else {
                    IconTone::Default
                },
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .text_color(rgb(theme_text()))
                            .child(application.name),
                    )
                    .when_some(application.generic_name, |column, generic_name| {
                        column.child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(rgb(text_muted()))
                                .child(generic_name),
                        )
                    }),
            )
            .when(application.is_default, |row| {
                row.child(
                    div()
                        .px_2()
                        .py_1()
                        .rounded_full()
                        .bg(rgb(surface_elevated()))
                        .text_xs()
                        .text_color(rgb(text_muted()))
                        .child("Default"),
                )
            })
    }
}

fn open_with_section_label(label: &'static str) -> AnyElement {
    div()
        .h(px(28.0))
        .px_2()
        .pt_2()
        .text_xs()
        .text_color(rgb(text_muted()))
        .child(label)
        .into_any_element()
}

fn open_with_button(
    label: &'static str,
    primary: bool,
    enabled: bool,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("open-with-button-{label}")))
        .h_8()
        .px_4()
        .rounded_md()
        .flex()
        .items_center()
        .text_xs()
        .bg(if primary && enabled {
            rgb(accent_background())
        } else {
            rgb(surface_elevated())
        })
        .text_color(if enabled {
            rgb(theme_text())
        } else {
            rgb(border_focused())
        })
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| {
                    style.bg(if primary {
                        rgb(accent_hover())
                    } else {
                        rgb(border())
                    })
                })
                .with_focus_ring()
        })
        .child(label)
}
