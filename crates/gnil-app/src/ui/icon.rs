use gpui::{Pixels, Svg, prelude::*, px, rgb, svg};

use crate::theme_runtime::{accent, danger, text, text_emphasized, text_muted, warning};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum IconSize {
    Small,
    Compact,
    Standard,
    Detail,
}

impl IconSize {
    const fn pixels(self) -> Pixels {
        match self {
            Self::Small => px(16.0),
            Self::Compact => px(20.0),
            Self::Standard => px(24.0),
            Self::Detail => px(32.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum IconTone {
    Muted,
    #[default]
    Default,
    Emphasized,
    Accent,
    Danger,
    Warning,
}

impl IconTone {
    fn color(self) -> u32 {
        match self {
            Self::Muted => text_muted(),
            Self::Default => text(),
            Self::Emphasized => text_emphasized(),
            Self::Accent => accent(),
            Self::Danger => danger(),
            Self::Warning => warning(),
        }
    }
}

pub(crate) fn ui_icon(path: &'static str, size: IconSize, tone: IconTone) -> Svg {
    svg()
        .path(path)
        .size(size.pixels())
        .text_color(rgb(tone.color()))
}

#[cfg(test)]
mod tests {
    use super::{IconSize, IconTone};

    #[test]
    fn icon_tiers_and_tones_are_distinct_contracts() {
        assert_ne!(IconSize::Small, IconSize::Compact);
        assert_ne!(IconSize::Compact, IconSize::Standard);
        assert_ne!(IconSize::Standard, IconSize::Detail);
        assert_ne!(IconTone::Muted, IconTone::Default);
        assert_ne!(IconTone::Default, IconTone::Emphasized);
    }
}
