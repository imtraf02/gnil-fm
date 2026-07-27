impl Picker {
    fn render_footer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let error = self.error.clone();
        let status = self.status.clone();
        let warning = self.conflict_warning(cx);
        let has_error = error.is_some();
        let has_warning = warning.is_some();
        let save_many_summary = match &self.request.kind {
            PickerRequestKind::SaveMany(options) => Some(format!(
                "Choose a destination for {} file(s).",
                options.files.len()
            )),
            _ => None,
        };
        let accept_enabled = self.can_accept(cx);
        let name_input = self.name_input.clone();
        let filter = self.render_filter(cx);
        let choices = (!self.choices.is_empty()).then(|| self.render_choices(cx));

        div()
            .min_h(px(72.0))
            .px_4()
            .py_3()
            .border_t_1()
            .border_color(rgb(border()))
            .flex()
            .flex_col()
            .gap_2()
            .when_some(
                error.or(status).or(warning).or(save_many_summary),
                |row, message| {
                    row.child(
                        div()
                            .text_xs()
                            .text_color(rgb(if has_error {
                                danger()
                            } else if has_warning {
                                warning_color()
                            } else {
                                text_muted()
                            }))
                            .child(message),
                    )
                },
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when_some(name_input, |row, input| {
                        row.child(div().w(px(280.0)).h_9().child(input))
                    })
                    .when_some(choices, gpui::ParentElement::child)
                    .child(div().flex_1())
                    .when_some(filter, gpui::ParentElement::child)
                    .child(
                        secondary_button("Cancel")
                            .id("picker-cancel")
                            .with_focus_ring()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel(&Cancel, window, cx);
                            })),
                    )
                    .child(
                        primary_button(self.accept_label(), accept_enabled)
                            .id("picker-accept")
                            .when(accept_enabled, FocusableControl::with_focus_ring)
                            .when(accept_enabled, |button| {
                                button.on_click(cx.listener(|this, _, window, cx| {
                                    this.accept(&Accept, window, cx);
                                }))
                            }),
                    ),
            )
            .into_any_element()
    }
}
