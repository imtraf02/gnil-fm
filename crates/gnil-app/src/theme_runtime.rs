use std::sync::atomic::{AtomicU32, Ordering};

use gnil_core::ThemeColors;

macro_rules! token {
    ($static:ident, $getter:ident, $default:expr) => {
        static $static: AtomicU32 = AtomicU32::new($default);

        #[must_use]
        pub fn $getter() -> u32 {
            $static.load(Ordering::Relaxed)
        }
    };
}

token!(BACKGROUND, background, ThemeColors::dark().background);
token!(SURFACE, surface, ThemeColors::dark().surface);
token!(
    SURFACE_ELEVATED,
    surface_elevated,
    ThemeColors::dark().surface_elevated
);
token!(BORDER, border, ThemeColors::dark().border);
token!(
    BORDER_FOCUSED,
    border_focused,
    ThemeColors::dark().border_focused
);
token!(TEXT_MUTED, text_muted, ThemeColors::dark().text_muted);
token!(TEXT, text, ThemeColors::dark().text);
token!(
    TEXT_EMPHASIZED,
    text_emphasized,
    ThemeColors::dark().text_emphasized
);
token!(ACCENT, accent, ThemeColors::dark().accent);
token!(
    ACCENT_BACKGROUND,
    accent_background,
    ThemeColors::dark().accent_background
);
token!(ACCENT_HOVER, accent_hover, ThemeColors::dark().accent_hover);
token!(DANGER, danger, ThemeColors::dark().danger);
token!(ERROR, error, ThemeColors::dark().error);
token!(GIT_ADDED, git_added, ThemeColors::dark().git_added);
token!(GIT_MODIFIED, git_modified, ThemeColors::dark().git_modified);
token!(GIT_DELETED, git_deleted, ThemeColors::dark().git_deleted);
token!(
    GIT_UNTRACKED,
    git_untracked,
    ThemeColors::dark().git_untracked
);

pub fn set_active(colors: ThemeColors) {
    BACKGROUND.store(colors.background, Ordering::Relaxed);
    SURFACE.store(colors.surface, Ordering::Relaxed);
    SURFACE_ELEVATED.store(colors.surface_elevated, Ordering::Relaxed);
    BORDER.store(colors.border, Ordering::Relaxed);
    BORDER_FOCUSED.store(colors.border_focused, Ordering::Relaxed);
    TEXT_MUTED.store(colors.text_muted, Ordering::Relaxed);
    TEXT.store(colors.text, Ordering::Relaxed);
    TEXT_EMPHASIZED.store(colors.text_emphasized, Ordering::Relaxed);
    ACCENT.store(colors.accent, Ordering::Relaxed);
    ACCENT_BACKGROUND.store(colors.accent_background, Ordering::Relaxed);
    ACCENT_HOVER.store(colors.accent_hover, Ordering::Relaxed);
    DANGER.store(colors.danger, Ordering::Relaxed);
    ERROR.store(colors.error, Ordering::Relaxed);
    GIT_ADDED.store(colors.git_added, Ordering::Relaxed);
    GIT_MODIFIED.store(colors.git_modified, Ordering::Relaxed);
    GIT_DELETED.store(colors.git_deleted, Ordering::Relaxed);
    GIT_UNTRACKED.store(colors.git_untracked, Ordering::Relaxed);
}

#[must_use]
pub fn selection_rgba() -> u32 {
    (accent() << 8) | 0x42
}
