fn selected_theme_name(settings: &AppSettings, appearance: ThemeAppearance) -> &str {
    match appearance {
        ThemeAppearance::Light => &settings.light_theme,
        ThemeAppearance::Dark => &settings.dark_theme,
    }
}

pub(crate) fn open_main_window(
    request: FileManagerOpenRequest,
    cx: &mut App,
) -> Result<(), String> {
    let bounds = Bounds::centered(None, size(px(1180.0), px(760.0)), cx);
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_background: gpui::WindowBackgroundAppearance::Opaque,
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("gnil-fm".into()),
                    ..Default::default()
                }),
                app_id: Some("gnil-fm".into()),
                ..Default::default()
            },
            move |window, cx| {
                let system_appearance = window_theme_appearance(window);
                cx.new(|cx| {
                    let mut manager =
                        FileManager::new(&request.directory, system_appearance, cx);
                    manager.pending_reveal = request.reveal;
                    manager.pending_show_properties = request.show_properties;
                    manager.load_directory(cx);
                    manager.refresh_devices(cx);
                    FileManager::schedule_device_monitor(cx);
                    manager
                })
            },
        )
        .map_err(|error| error.to_string())?;
    window
        .update(cx, |manager, window, cx| {
            manager.appearance_subscription =
                Some(cx.observe_window_appearance(window, FileManager::system_appearance_changed));
            window.focus(&manager.focus_handle(cx));
        })
        .ok();
    cx.activate(true);
    Ok(())
}

fn setting_section_header(title: &'static str, description: &'static str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_base()
                .child(title),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(text_muted()))
                .child(description),
        )
}

fn setting_row(
    title: &'static str,
    description: &'static str,
    control: impl IntoElement,
) -> gpui::Div {
    div()
        .p_3()
        .rounded_lg()
        .bg(rgb(surface_elevated()))
        .border_1()
        .border_color(rgb(border()))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pr_4()
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_xs()
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(text_muted()))
                        .child(description),
                ),
        )
        .child(control)
}

fn setting_switch(active: bool) -> gpui::Div {
    div()
        .w(px(38.0))
        .h(px(20.0))
        .rounded_full()
        .p(px(2.0))
        .flex()
        .items_center()
        .bg(if active {
            rgb(accent())
        } else {
            rgb(border_focused())
        })
        .child(
            div()
                .size(px(16.0))
                .rounded_full()
                .bg(rgb(background()))
                .when(active, |thumb| thumb.ml(px(18.0))),
        )
}

fn setting_chip(label: &'static str, active: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(format!("setting-chip-{label}")))
        .px_3()
        .py_1()
        .rounded_md()
        .text_xs()
        .cursor_pointer()
        .font_weight(if active {
            gpui::FontWeight::SEMIBOLD
        } else {
            gpui::FontWeight::NORMAL
        })
        .bg(if active {
            rgb(accent_background())
        } else {
            rgb(surface_elevated())
        })
        .text_color(if active {
            rgb(text_emphasized())
        } else {
            rgb(text_muted())
        })
        .hover(|style| style.bg(rgb(border())))
        .with_focus_ring()
        .child(label)
}
