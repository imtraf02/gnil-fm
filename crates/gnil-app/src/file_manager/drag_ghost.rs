struct DragGhost {
    payload: FileDragPayload,
    cursor_offset: gpui::Point<Pixels>,
}

#[derive(Clone)]
struct FavoriteDragPayload {
    path: PathBuf,
    label: String,
}

struct FavoriteDragGhost {
    label: String,
    cursor_offset: gpui::Point<Pixels>,
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let lifted = self.payload.visual.lifted();
        let copy = self.payload.visual.copy();
        let count = self.payload.paths.len();
        div()
            .pl(self.cursor_offset.x - px(12.0))
            .pt(self.cursor_offset.y - px(18.0))
            .opacity(if lifted { 1.0 } else { 0.0 })
            .child(
                div()
                    .h(px(38.0))
                    .max_w(px(236.0))
                    .px_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(if copy { accent() } else { border_focused() }))
                    .bg(rgb(surface_elevated()))
                    .shadow_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(rgb(text_emphasized()))
                    .child(ui_icon(
                        self.payload.first_icon,
                        IconSize::Compact,
                        IconTone::Default,
                    ))
                    .child(
                        div()
                            .max_w(px(150.0))
                            .truncate()
                            .child(self.payload.first_name.clone()),
                    )
                    .when(count > 1, |ghost| {
                        ghost.child(
                            div()
                                .h_5()
                                .min_w(px(24.0))
                                .px_1()
                                .rounded_full()
                                .bg(rgb(accent_background()))
                                .flex()
                                .items_center()
                                .justify_center()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(count.to_string()),
                        )
                    })
                    .when(copy, |ghost| {
                        ghost.child(div().text_color(rgb(accent())).child("Copy"))
                    }),
            )
    }
}

impl Render for FavoriteDragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.cursor_offset.x - px(12.0))
            .pt(self.cursor_offset.y - px(17.0))
            .child(
                div()
                    .h(px(34.0))
                    .max_w(px(204.0))
                    .px_2()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(border_focused()))
                    .bg(rgb(surface_elevated()))
                    .shadow_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(rgb(text_emphasized()))
                    .child(ui_icon(
                        "icons/folder-favorite.svg",
                        IconSize::Compact,
                        IconTone::Accent,
                    ))
                    .child(div().max_w(px(150.0)).truncate().child(self.label.clone())),
            )
    }
}
