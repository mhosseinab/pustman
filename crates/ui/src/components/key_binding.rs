//! The keybinding hint shown in tooltips, menus and empty states.
//!
//! A hint is read from the active keymap at render time, so it cannot go stale when the
//! user rebinds a key (`references/interaction-focus-a11y.md`: "bindings are shown in
//! tooltips and menus").

use gpui::{Action, App, IntoElement, ParentElement, RenderOnce, Styled, Window, div};
use theme::ActiveTheme;

use crate::color::Color;
use crate::h_flex;
use crate::spacing::DynamicSpacing;
use crate::typography::{StyledTypography, TextSize};

/// The keys that run an action, as the current keymap spells them on this platform.
#[derive(IntoElement)]
pub struct KeyBinding {
    action: Box<dyn Action>,
    color: Color,
}

impl Clone for KeyBinding {
    fn clone(&self) -> Self {
        Self {
            action: self.action.boxed_clone(),
            color: self.color,
        }
    }
}

impl std::fmt::Debug for KeyBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyBinding")
            .field("action", &self.action.name())
            .finish()
    }
}

impl KeyBinding {
    /// The binding the active keymap has for `action` on this platform.
    pub fn for_action(action: &dyn Action) -> Self {
        Self {
            action: action.boxed_clone(),
            color: Color::Muted,
        }
    }

    /// Tint the hint (use [`Color::Disabled`] in a disabled control).
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// The keys as the user would press them: `["Ctrl+Enter"]`, one entry per chord step.
    /// Empty when nothing is bound.
    pub fn keystrokes(&self, window: &Window, cx: &App) -> Vec<String> {
        let Some(binding) = window.highest_precedence_binding_for_action(self.action.as_ref())
        else {
            return Vec::new();
        };
        let os = cx.platform_info().os;
        binding
            .keystrokes()
            .iter()
            .map(|keystroke| keystroke.display(os).to_string())
            .collect()
    }

    /// The same keys as one line: `"Ctrl+K Ctrl+S"` for a chord.
    pub fn keystroke_text(&self, window: &Window, cx: &App) -> Option<String> {
        let keys = self.keystrokes(window, cx);
        if keys.is_empty() {
            None
        } else {
            Some(keys.join(" "))
        }
    }
}

impl RenderOnce for KeyBinding {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let keys = self.keystrokes(window, cx);
        h_flex()
            .font_ui(cx)
            .gap(DynamicSpacing::Base01.rems(cx))
            .children(keys.into_iter().map(|key| {
                div()
                    .px(DynamicSpacing::Base02.rems(cx))
                    .rounded_xs()
                    .border_1()
                    .border_color(cx.theme().colors().border_variant)
                    .text_color(self.color.color(cx))
                    .text_size(TextSize::XSmall.rems(cx))
                    .child(key)
            }))
    }
}
