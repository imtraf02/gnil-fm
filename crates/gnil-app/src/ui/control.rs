use gpui::{Div, Stateful, prelude::*, rgb};

use crate::theme_runtime::{accent_background, border_focused};

pub(crate) trait FocusableControl {
    fn with_focus_ring(self) -> Self;
}

impl FocusableControl for Stateful<Div> {
    fn with_focus_ring(self) -> Self {
        self.tab_index(0)
            .focus(|style| style.border_1().border_color(rgb(border_focused())))
            .active(|style| style.bg(rgb(accent_background())))
    }
}
