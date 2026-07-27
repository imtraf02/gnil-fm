use std::time::Duration;

use gpui::{Animation, AnimationExt as _, AnyElement, IntoElement, Styled, px};

pub(crate) const BACKDROP_PRIORITY: usize = 9;
pub(crate) const MENU_PRIORITY: usize = 10;
pub(crate) const PATH_MENU_PRIORITY: usize = 11;
pub(crate) const APPEARANCE_PRIORITY: usize = 12;
pub(crate) const MODAL_PRIORITY: usize = 15;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum OverlayMotionState {
    #[default]
    Opening,
    Open,
    Closing,
}

pub(crate) fn animate_overlay<E>(
    element: E,
    state: OverlayMotionState,
    reduced_motion: bool,
    offset: f32,
) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    if reduced_motion || state == OverlayMotionState::Open {
        return element.into_any_element();
    }

    match state {
        OverlayMotionState::Opening => element
            .with_animation(
                "overlay-opening",
                Animation::new(Duration::from_millis(120)).with_easing(gpui::ease_out_quint()),
                move |element, delta| element.opacity(delta).top(px(offset * (1.0 - delta))),
            )
            .into_any_element(),
        OverlayMotionState::Closing => element
            .with_animation(
                "overlay-closing",
                Animation::new(Duration::from_millis(80)).with_easing(gpui::quadratic),
                move |element, delta| element.opacity(1.0 - delta).top(px(offset * delta)),
            )
            .into_any_element(),
        OverlayMotionState::Open => element.into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::OverlayMotionState;

    #[test]
    fn overlay_state_distinguishes_entry_and_exit() {
        assert_ne!(OverlayMotionState::Opening, OverlayMotionState::Closing);
        assert_ne!(OverlayMotionState::Open, OverlayMotionState::Closing);
    }
}
