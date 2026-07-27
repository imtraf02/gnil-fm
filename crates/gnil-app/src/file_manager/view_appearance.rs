impl FileManager {
    #[allow(clippy::too_many_lines)]
    fn render_appearance_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected_mode = self.settings.theme;
        let mode_row = [
            (ThemeMode::System, "System"),
            (ThemeMode::Light, "Light"),
            (ThemeMode::Dark, "Dark"),
        ]
        .into_iter()
        .enumerate()
        .fold(
            div()
                .h_8()
                .w_full()
                .p_1()
                .flex()
                .rounded_md()
                .bg(rgb(surface_elevated())),
            |row, (index, (mode, label))| {
                row.child(
                    div()
                        .id(("theme-mode", index))
                        .h_full()
                        .flex_1()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(if selected_mode == mode {
                            rgb(text_emphasized())
                        } else {
                            rgb(text_muted())
                        })
                        .when(selected_mode == mode, |button| {
                            button.bg(rgb(accent_background()))
                        })
                        .hover(|button| button.bg(rgb(border())))
                        .with_focus_ring()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.set_theme_mode(mode, window, cx);
                        }))
                        .child(label),
                )
            },
        );
        let themes = self
            .theme_catalog
            .themes_for(self.theme_appearance)
            .cloned()
            .collect::<Vec<_>>();
        let selected_name = self.active_theme_name.clone();
        let theme_rows = themes.into_iter().enumerate().fold(
            div().w_full().flex().flex_col().gap_1(),
            |list, (index, theme)| {
                let name = theme.name.clone();
                let selected = name == selected_name;
                let builtin = theme.builtin();
                list.child(
                    div()
                        .id(("theme-choice", index))
                        .h(px(36.0))
                        .w_full()
                        .px_2()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .when(selected, |row| row.bg(rgb(accent_background())))
                        .hover(|row| row.bg(rgb(border())))
                        .with_focus_ring()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_theme(&name, cx);
                        }))
                        .child(
                            div()
                                .size_3()
                                .rounded_full()
                                .border_1()
                                .border_color(rgb(theme.colors.border_focused))
                                .bg(rgb(theme.colors.accent)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(rgb(theme_text()))
                                .child(theme.name),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(if selected { accent() } else { text_muted() }))
                                .child(if selected {
                                    "●"
                                } else if builtin {
                                    "Built-in"
                                } else {
                                    "JSON"
                                }),
                        ),
                )
            },
        );
        let theme_path = self.config_paths.themes_dir().display().to_string();
        let error_summary = (!self.theme_catalog.errors.is_empty()).then(|| {
            format!(
                "{} invalid theme file{} — Reload after fixing JSON",
                self.theme_catalog.errors.len(),
                if self.theme_catalog.errors.len() == 1 {
                    ""
                } else {
                    "s"
                }
            )
        });
        let panel = div()
            .id("appearance-menu")
            .w(px(304.0))
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(rgb(border_focused()))
            .bg(rgb(surface()))
            .shadow_lg()
            .occlude()
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(text_emphasized()))
                            .child("Appearance"),
                    )
                    .child(
                        div()
                            .id("close-appearance")
                            .size_6()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_color(rgb(text_muted()))
                            .hover(|button| button.bg(rgb(border())).text_color(rgb(theme_text())))
                            .with_focus_ring()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dismiss_appearance_menu(cx);
                            }))
                            .child(ui_icon(
                                "icons/action-close.svg",
                                IconSize::Small,
                                IconTone::Muted,
                            )),
                    ),
            )
            .child(mode_row)
            .child(div().text_xs().text_color(rgb(text_muted())).child(
                match self.theme_appearance {
                    ThemeAppearance::Light => "LIGHT THEMES",
                    ThemeAppearance::Dark => "DARK THEMES",
                },
            ))
            .child(theme_rows)
            .when_some(error_summary, |panel, error| {
                panel.child(
                    div()
                        .rounded_md()
                        .p_2()
                        .bg(rgb(surface_elevated()))
                        .text_xs()
                        .text_color(rgb(error_color()))
                        .child(error),
                )
            })
            .child(
                div()
                    .pt_2()
                    .border_t_1()
                    .border_color(rgb(border()))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child(theme_path),
                    )
                    .child(
                        div()
                            .id("reload-themes")
                            .h_7()
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(theme_text()))
                            .bg(rgb(surface_elevated()))
                            .hover(|button| button.bg(rgb(border())))
                            .with_focus_ring()
                            .on_click(cx.listener(|this, _, _, cx| this.reload_themes(cx)))
                            .child("Reload"),
                    ),
            );
        let panel = animate_overlay(
            panel,
            if self.appearance_menu_closing {
                OverlayMotionState::Closing
            } else {
                OverlayMotionState::Opening
            },
            self.reduced_motion,
            3.0,
        );
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Local)
                .position(point(px(88.0), px(36.0)))
                .anchor(Corner::TopRight)
                .snap_to_window()
                .child(panel),
        )
        .with_priority(APPEARANCE_PRIORITY)
        .into_any_element()
    }

}
