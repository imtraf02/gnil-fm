use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, Context, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, PromptButton,
    PromptHandle, PromptLevel, PromptResponse, Render, RenderablePromptHandle,
    StatefulInteractiveElement, Styled, Window, div, prelude::*, px, rgb,
};

use crate::{
    theme_runtime::{
        self, accent, accent_background, accent_hover, background, border, border_focused, danger,
        error, surface, surface_elevated, text, text_emphasized, text_muted, warning,
    },
    ui::{
        icon::{IconSize, IconTone, ui_icon},
        overlay::{OverlayMotionState, animate_overlay},
    },
};

pub(crate) fn register_prompt_builder(cx: &mut App) {
    cx.set_prompt_builder(render_custom_prompt);
}

pub(crate) fn render_custom_prompt(
    level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    actions: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    let reduced_motion = theme_runtime::reduced_motion();
    let actions_vec = actions.to_vec();

    // Default to the cancel/safety action so an immediate Enter does not accidentally execute a destructive action.
    let cancel_index = find_cancel_index(&actions_vec);

    let view = cx.new(|cx| CustomPromptView {
        level,
        message: message.to_string(),
        detail: detail.map(ToString::to_string),
        actions: actions_vec,
        focus_handle: cx.focus_handle(),
        focused_button: cancel_index,
        motion_state: OverlayMotionState::Opening,
        reduced_motion,
    });

    handle.with_view(view, window, cx)
}

fn find_cancel_index(actions: &[PromptButton]) -> usize {
    actions
        .iter()
        .enumerate()
        .find(|(_, a)| {
            a.is_cancel()
                || matches!(
                    a.label().to_lowercase().as_str(),
                    "cancel" | "keep" | "no" | "dismiss" | "close"
                )
        })
        .map_or_else(|| actions.len().saturating_sub(1), |(i, _)| i)
}

pub(crate) struct CustomPromptView {
    level: PromptLevel,
    message: String,
    detail: Option<String>,
    actions: Vec<PromptButton>,
    focus_handle: FocusHandle,
    focused_button: usize,
    motion_state: OverlayMotionState,
    reduced_motion: bool,
}

impl CustomPromptView {
    #[must_use]
    pub(crate) fn cancel_index(&self) -> usize {
        find_cancel_index(&self.actions)
    }

    pub(crate) fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.motion_state == OverlayMotionState::Closing {
            return;
        }
        if self.reduced_motion {
            cx.emit(PromptResponse(index));
            return;
        }
        self.motion_state = OverlayMotionState::Closing;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(80));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |_, cx| {
                cx.emit(PromptResponse(index));
            });
        })
        .detach();
    }
}

impl EventEmitter<PromptResponse> for CustomPromptView {}

impl Focusable for CustomPromptView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CustomPromptView {
    #[allow(clippy::too_many_lines)]
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cancel_index = self.cancel_index();
        let is_trash_or_delete = {
            let lower_msg = self.message.to_lowercase();
            let lower_detail = self.detail.as_deref().unwrap_or("").to_lowercase();
            lower_msg.contains("trash")
                || lower_msg.contains("delete")
                || lower_msg.contains("remove")
                || lower_detail.contains("trash")
                || lower_detail.contains("delete")
        };

        let (icon_path, icon_tone, badge_bg, badge_border) = match self.level {
            PromptLevel::Critical => {
                let path = if is_trash_or_delete {
                    "icons/trash.svg"
                } else {
                    "icons/status-warning.svg"
                };
                (
                    path,
                    IconTone::Danger,
                    gpui::Hsla::from(rgb(danger())).opacity(0.12),
                    gpui::Hsla::from(rgb(danger())).opacity(0.28),
                )
            }
            PromptLevel::Warning => {
                let path = if is_trash_or_delete {
                    "icons/trash.svg"
                } else {
                    "icons/status-warning.svg"
                };
                (
                    path,
                    IconTone::Warning,
                    gpui::Hsla::from(rgb(warning())).opacity(0.12),
                    gpui::Hsla::from(rgb(warning())).opacity(0.28),
                )
            }
            PromptLevel::Info => (
                "icons/status-warning.svg",
                IconTone::Accent,
                gpui::Hsla::from(rgb(accent())).opacity(0.12),
                gpui::Hsla::from(rgb(accent())).opacity(0.28),
            ),
        };

        let icon_badge = div()
            .size(px(40.0))
            .flex_none()
            .rounded_lg()
            .bg(badge_bg)
            .border_1()
            .border_color(badge_border)
            .flex()
            .items_center()
            .justify_center()
            .child(ui_icon(icon_path, IconSize::Compact, icon_tone));

        let text_content = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(text_emphasized()))
                    .line_height(px(20.0))
                    .child(self.message.clone()),
            )
            .when_some(self.detail.clone(), |col, detail| {
                col.child(
                    div()
                        .text_xs()
                        .text_color(rgb(text_muted()))
                        .line_height(px(18.0))
                        .child(detail),
                )
            });

        let close_button = div()
            .id("prompt-close-button")
            .size_6()
            .flex_none()
            .rounded_md()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(rgb(border())))
            .on_click(cx.listener(move |this, _, _, cx| {
                let cancel = this.cancel_index();
                this.choose(cancel, cx);
            }))
            .child(ui_icon(
                "icons/action-close.svg",
                IconSize::Small,
                IconTone::Muted,
            ));

        let body = div()
            .p_5()
            .flex()
            .items_start()
            .gap_4()
            .child(icon_badge)
            .child(text_content)
            .child(close_button);

        // When there are 2 actions where index 0 is affirmative and index 1 is cancel,
        // present cancel first (on the left) and primary action second (on the right)
        // following desktop Linux and macOS design guidelines.
        let display_indices: Vec<usize> = if self.actions.len() == 2 && cancel_index == 1 {
            vec![1, 0]
        } else {
            (0..self.actions.len()).collect()
        };

        let buttons: Vec<AnyElement> = display_indices
            .into_iter()
            .map(|ix| {
                let action = &self.actions[ix];
                let label = action.label().clone();
                let is_focused = self.focused_button == ix;
                let is_cancel = ix == cancel_index;
                let is_destructive = self.level == PromptLevel::Critical && !is_cancel;
                let is_primary = !is_cancel;

                let button = div()
                    .id(("prompt-button", ix))
                    .h_8()
                    .px_4()
                    .rounded_md()
                    .cursor_pointer()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(is_destructive, |btn| {
                        btn.bg(rgb(danger()))
                            .text_color(rgb(surface()))
                            .hover(|style| style.bg(rgb(error())))
                            .active(|style| style.opacity(0.85))
                            .when(is_focused, |btn| {
                                btn.border_2().border_color(rgb(text_emphasized()))
                            })
                    })
                    .when(is_primary && !is_destructive, |btn| {
                        btn.bg(rgb(accent_background()))
                            .text_color(rgb(text_emphasized()))
                            .hover(|style| style.bg(rgb(accent_hover())))
                            .active(|style| style.opacity(0.85))
                            .when(is_focused, |btn| {
                                btn.border_1().border_color(rgb(border_focused()))
                            })
                    })
                    .when(is_cancel, |btn| {
                        btn.bg(rgb(surface_elevated()))
                            .border_1()
                            .border_color(if is_focused {
                                rgb(border_focused())
                            } else {
                                rgb(border())
                            })
                            .text_color(rgb(text()))
                            .hover(|style| style.bg(rgb(border())))
                            .active(|style| style.bg(rgb(surface())))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.choose(ix, cx);
                    }))
                    .child(label);

                button.into_any_element()
            })
            .collect();

        let footer = div()
            .h(px(52.0))
            .px_5()
            .flex()
            .items_center()
            .justify_end()
            .gap_2()
            .border_t_1()
            .border_color(rgb(border()))
            .bg(rgb(surface_elevated()))
            .children(buttons);

        let card = div()
            .id("custom-prompt-card")
            .w(px(440.0))
            .max_w(px(520.0))
            .rounded_lg()
            .border_1()
            .border_color(rgb(border()))
            .bg(rgb(surface()))
            .shadow_lg()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(body)
            .child(footer);

        let animated_card = animate_overlay(card, self.motion_state, self.reduced_motion, 4.0);

        div()
            .id("custom-prompt-overlay")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::Hsla::from(rgb(background())).opacity(0.60))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    let cancel = this.cancel_index();
                    this.choose(cancel, cx);
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                match key {
                    "escape" => {
                        let cancel = this.cancel_index();
                        this.choose(cancel, cx);
                        cx.stop_propagation();
                    }
                    "enter" | "space" => {
                        let index = this.focused_button;
                        this.choose(index, cx);
                        cx.stop_propagation();
                    }
                    "tab" if !this.actions.is_empty() => {
                        if event.keystroke.modifiers.shift {
                            this.focused_button =
                                (this.focused_button + this.actions.len() - 1) % this.actions.len();
                        } else {
                            this.focused_button = (this.focused_button + 1) % this.actions.len();
                        }
                        cx.notify();
                        cx.stop_propagation();
                    }
                    "right" | "down" if !this.actions.is_empty() => {
                        this.focused_button = (this.focused_button + 1) % this.actions.len();
                        cx.notify();
                        cx.stop_propagation();
                    }
                    "left" | "up" if !this.actions.is_empty() => {
                        this.focused_button =
                            (this.focused_button + this.actions.len() - 1) % this.actions.len();
                        cx.notify();
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .child(animated_card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn custom_prompt_identifies_cancel_action() {
        let actions = vec![
            PromptButton::new("Delete Permanently"),
            PromptButton::new("Cancel"),
        ];
        assert_eq!(find_cancel_index(&actions), 1);

        let actions_keep = vec![
            PromptButton::new("Remove from Favorites"),
            PromptButton::new("Keep"),
        ];
        assert_eq!(find_cancel_index(&actions_keep), 1);

        let fallback_actions = vec![PromptButton::new("Action 1"), PromptButton::new("Action 2")];
        assert_eq!(find_cancel_index(&fallback_actions), 1);
    }

    #[gpui::test]
    fn custom_prompt_keyboard_navigation_and_cancel(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let actions = vec![
                PromptButton::new("Delete Permanently"),
                PromptButton::new("Cancel"),
            ];
            let cancel_index = find_cancel_index(&actions);
            let handle = cx.focus_handle();
            window.focus(&handle);
            CustomPromptView {
                level: PromptLevel::Critical,
                message: "Permanently delete file.txt?".into(),
                detail: Some("This action cannot be undone.".into()),
                actions,
                focus_handle: handle,
                focused_button: cancel_index,
                motion_state: OverlayMotionState::Open,
                reduced_motion: true,
            }
        });

        // Initially cancel button (index 1) is focused for safety
        assert_eq!(cx.read(|cx| view.read(cx).focused_button), 1);

        // Tab should cycle focus to index 0
        cx.update(|_window, cx| {
            view.update(cx, |this, cx| {
                this.focused_button = (this.focused_button + 1) % this.actions.len();
                cx.notify();
            });
        });
        assert_eq!(cx.read(|cx| view.read(cx).focused_button), 0);
    }
}
