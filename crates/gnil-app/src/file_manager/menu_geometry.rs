const MENU_PANEL_INSET: f32 = 5.0;
const MENU_SEPARATOR_HEIGHT: f32 = 9.0;
const MENU_SUBMENU_GAP: f32 = 4.0;
const MENU_VIEWPORT_MARGIN: f32 = 4.0;
const EMPTY_SPACE_MENU_WIDTH: f32 = 264.0;
const EMPTY_SPACE_MENU_ROW_HEIGHT: f32 = 32.0;
const ACTION_MENU_WIDTH: f32 = 288.0;
const ACTION_MENU_ROW_HEIGHT: f32 = 36.0;
const ACTION_MENU_TOOLBAR_HEIGHT: f32 = 44.0;
const HEADER_ACTION_MENU_TOP: f32 = 47.0;
const OPEN_WITH_SUBMENU_WIDTH: f32 = 280.0;
const OPEN_WITH_MAX_APPLICATIONS: f32 = 5.0;
const OPEN_WITH_SUBMENU_RESERVED_HEIGHT: f32 = MENU_PANEL_INSET * 2.0
    + OPEN_WITH_MAX_APPLICATIONS * ACTION_MENU_ROW_HEIGHT
    + MENU_SEPARATOR_HEIGHT
    + ACTION_MENU_ROW_HEIGHT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubmenuSide {
    Left,
    Right,
}

impl SubmenuSide {
    const fn opening_offset(self) -> f32 {
        match self {
            Self::Left => -3.0,
            Self::Right => 3.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SubmenuPlacement {
    side: SubmenuSide,
    top: f32,
}

fn empty_space_submenu_top(
    entries: &[EmptySpaceMenuEntry],
    target: EmptySpaceSubmenu,
) -> f32 {
    MENU_PANEL_INSET
        + entries
            .iter()
            .take_while(|entry| {
                !matches!(
                    entry,
                    EmptySpaceMenuEntry::Submenu { submenu, .. } if *submenu == target
                )
            })
            .map(|entry| match entry {
                EmptySpaceMenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
                EmptySpaceMenuEntry::Action { .. } | EmptySpaceMenuEntry::Submenu { .. } => {
                    EMPTY_SPACE_MENU_ROW_HEIGHT
                }
            })
            .sum::<f32>()
}

fn action_submenu_top(entries: &[MenuEntry], target: FileMenuSubmenu) -> f32 {
    MENU_PANEL_INSET
        + ACTION_MENU_TOOLBAR_HEIGHT
        + MENU_SEPARATOR_HEIGHT
        + entries
            .iter()
            .take_while(|entry| {
                !matches!(
                    entry,
                    MenuEntry::Submenu { submenu, .. } if *submenu == target
                )
            })
            .map(|entry| match entry {
                MenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
                MenuEntry::Action { .. } | MenuEntry::Submenu { .. } => ACTION_MENU_ROW_HEIGHT,
            })
            .sum::<f32>()
}

fn empty_space_panel_height(entries: &[EmptySpaceMenuEntry]) -> f32 {
    MENU_PANEL_INSET * 2.0
        + entries
            .iter()
            .map(|entry| match entry {
                EmptySpaceMenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
                EmptySpaceMenuEntry::Action { .. } | EmptySpaceMenuEntry::Submenu { .. } => {
                    EMPTY_SPACE_MENU_ROW_HEIGHT
                }
            })
            .sum::<f32>()
}

fn action_menu_panel_height(entries: &[MenuEntry]) -> f32 {
    MENU_PANEL_INSET * 2.0
        + ACTION_MENU_TOOLBAR_HEIGHT
        + MENU_SEPARATOR_HEIGHT
        + entries
            .iter()
            .map(|entry| match entry {
                MenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
                MenuEntry::Action { .. } | MenuEntry::Submenu { .. } => ACTION_MENU_ROW_HEIGHT,
            })
            .sum::<f32>()
}

fn snapped_menu_origin(
    desired: gpui::Point<gpui::Pixels>,
    menu_width: f32,
    menu_height: f32,
    viewport: gpui::Size<gpui::Pixels>,
) -> gpui::Point<gpui::Pixels> {
    let max_x = (f32::from(viewport.width) - MENU_VIEWPORT_MARGIN - menu_width)
        .max(MENU_VIEWPORT_MARGIN);
    let max_y = (f32::from(viewport.height) - MENU_VIEWPORT_MARGIN - menu_height)
        .max(MENU_VIEWPORT_MARGIN);
    gpui::point(
        gpui::px(
            f32::from(desired.x)
                .max(MENU_VIEWPORT_MARGIN)
                .min(max_x),
        ),
        gpui::px(
            f32::from(desired.y)
                .max(MENU_VIEWPORT_MARGIN)
                .min(max_y),
        ),
    )
}

fn submenu_placement(
    root_origin: gpui::Point<gpui::Pixels>,
    root_width: f32,
    trigger_top: f32,
    submenu_width: f32,
    submenu_reserved_height: f32,
    viewport: gpui::Size<gpui::Pixels>,
    preferred_side: Option<SubmenuSide>,
) -> SubmenuPlacement {
    let root_left = f32::from(root_origin.x);
    let root_right = root_left + root_width;
    let viewport_width = f32::from(viewport.width);
    let right_space = viewport_width - MENU_VIEWPORT_MARGIN - root_right - MENU_SUBMENU_GAP;
    let left_space = root_left - MENU_VIEWPORT_MARGIN - MENU_SUBMENU_GAP;
    let side = preferred_side.unwrap_or({
        if right_space >= submenu_width || right_space >= left_space {
            SubmenuSide::Right
        } else {
            SubmenuSide::Left
        }
    });

    let desired_top = f32::from(root_origin.y) + trigger_top;
    let max_top = (f32::from(viewport.height)
        - MENU_VIEWPORT_MARGIN
        - submenu_reserved_height)
        .max(MENU_VIEWPORT_MARGIN);
    let stable_top = desired_top.max(MENU_VIEWPORT_MARGIN).min(max_top);

    SubmenuPlacement {
        side,
        top: stable_top - f32::from(root_origin.y),
    }
}

#[cfg(test)]
mod menu_geometry_tests {
    use super::*;
    use gpui::{point, px, size};

    #[test]
    fn submenu_uses_left_side_when_root_is_near_right_edge() {
        let placement = submenu_placement(
            point(px(340.0), px(100.0)),
            288.0,
            50.0,
            280.0,
            160.0,
            size(px(640.0), px(480.0)),
            None,
        );

        assert_eq!(placement.side, SubmenuSide::Left);
    }

    #[test]
    fn submenu_uses_right_side_when_root_is_near_left_edge() {
        let placement = submenu_placement(
            point(px(4.0), px(100.0)),
            288.0,
            50.0,
            280.0,
            160.0,
            size(px(800.0), px(480.0)),
            None,
        );

        assert_eq!(placement.side, SubmenuSide::Right);
    }

    #[test]
    fn submenu_reserves_height_before_async_content_arrives() {
        let placement = submenu_placement(
            point(px(100.0), px(300.0)),
            288.0,
            140.0,
            280.0,
            OPEN_WITH_SUBMENU_RESERVED_HEIGHT,
            size(px(800.0), px(480.0)),
            None,
        );

        let bottom = placement.top + 300.0 + OPEN_WITH_SUBMENU_RESERVED_HEIGHT;
        assert!((bottom - 476.0).abs() < f32::EPSILON);
    }
}
