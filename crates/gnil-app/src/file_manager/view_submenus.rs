impl FileManager {
    fn render_empty_space_submenu_overlay(
        &self,
        menu: &EmptySpaceMenuState,
        root_origin: gpui::Point<Pixels>,
        placement: SubmenuPlacement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let submenu = Self::render_empty_space_panel(
            menu.submenu_entries.clone(),
            menu.focused_submenu,
            true,
            cx,
        );
        let submenu = if self.reduced_motion {
            submenu.into_any_element()
        } else {
            submenu
                .with_animation(
                    "empty-space-submenu-opening",
                    Animation::new(Duration::from_millis(100))
                        .with_easing(gpui::ease_out_quint()),
                    move |panel, delta| {
                        panel
                            .opacity(delta)
                            .left(px(placement.side.opening_offset() * (1.0 - delta)))
                    },
                )
                .into_any_element()
        };
        let (submenu_x, anchor) = match placement.side {
            SubmenuSide::Left => (
                f32::from(root_origin.x) - MENU_SUBMENU_GAP,
                Corner::TopRight,
            ),
            SubmenuSide::Right => (
                f32::from(root_origin.x) + EMPTY_SPACE_MENU_WIDTH + MENU_SUBMENU_GAP,
                Corner::TopLeft,
            ),
        };

        anchored()
            .position(point(
                px(submenu_x),
                px(f32::from(root_origin.y) + placement.top),
            ))
            .anchor(anchor)
            .snap_to_window_with_margin(px(MENU_VIEWPORT_MARGIN))
            .child(submenu)
            .into_any_element()
    }

    fn render_action_submenu_overlay(
        &self,
        root_origin: gpui::Point<Pixels>,
        placement: SubmenuPlacement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let submenu = self.render_open_with_submenu(cx);
        let submenu = if self.reduced_motion {
            submenu.into_any_element()
        } else {
            submenu
                .with_animation(
                    "open-with-submenu-opening",
                    Animation::new(Duration::from_millis(100))
                        .with_easing(gpui::ease_out_quint()),
                    move |panel, delta| {
                        panel
                            .opacity(delta)
                            .left(px(placement.side.opening_offset() * (1.0 - delta)))
                    },
                )
                .into_any_element()
        };
        let (submenu_x, anchor) = match placement.side {
            SubmenuSide::Left => (
                f32::from(root_origin.x) - MENU_SUBMENU_GAP,
                Corner::TopRight,
            ),
            SubmenuSide::Right => (
                f32::from(root_origin.x) + ACTION_MENU_WIDTH + MENU_SUBMENU_GAP,
                Corner::TopLeft,
            ),
        };

        anchored()
            .position(point(
                px(submenu_x),
                px(f32::from(root_origin.y) + placement.top),
            ))
            .anchor(anchor)
            .snap_to_window_with_margin(px(MENU_VIEWPORT_MARGIN))
            .child(submenu)
            .into_any_element()
    }
}
