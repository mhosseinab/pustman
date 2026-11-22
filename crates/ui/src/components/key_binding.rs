//! The keybinding hint shown in tooltips, menus and empty states.
//!
//! A hint is read from the active keymap at render time, so it cannot go stale when the
//! user rebinds a key (`references/interaction-focus-a11y.md`: "bindings are shown in
//! tooltips and menus").

use gpui::{
    Action, App, IntoElement, Keystroke, Modifiers, OperatingSystem, ParentElement, RenderOnce,
    Styled, Window, div,
};
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
    size: TextSize,
}

impl Clone for KeyBinding {
    fn clone(&self) -> Self {
        Self {
            action: self.action.boxed_clone(),
            color: self.color,
            size: self.size,
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
            size: TextSize::XSmall,
        }
    }

    /// Draw the keys this big. The default is the design system's hint size; an empty
    /// state's chips read at body size instead, where nothing else competes with them.
    pub fn size(mut self, size: TextSize) -> Self {
        self.size = size;
        self
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
            .map(|keystroke| keystroke_label(keystroke.modifiers(), keystroke.key(), os))
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

/// One keystroke as a hint spells it: `Ctrl+Shift+Tab` on Linux and Windows, GPUI's native
/// symbols (`⌘⇧P`) on macOS. GPUI's own display on Linux is the keymap syntax
/// (`ctrl-shift-tab`), which reads as code, not as a key to press.
pub fn keystroke_label(modifiers: &Modifiers, key: &str, os: OperatingSystem) -> String {
    if os == OperatingSystem::Mac {
        let keystroke = Keystroke {
            modifiers: *modifiers,
            key: key.to_string(),
            key_char: None,
        };
        return keystroke.display(os).to_string();
    }
    let platform = if os == OperatingSystem::Windows {
        "Win"
    } else {
        "Super"
    };
    let names = [
        (modifiers.function, "Fn"),
        (modifiers.control, "Ctrl"),
        (modifiers.alt, "Alt"),
        (modifiers.platform, platform),
        (modifiers.shift, "Shift"),
    ];
    let mut parts: Vec<String> = names
        .into_iter()
        .filter(|(held, _)| *held)
        .map(|(_, name)| name.to_string())
        .collect();
    parts.push(key_name(key));
    parts.join("+")
}

fn key_name(key: &str) -> String {
    match key {
        "pageup" => "PageUp".to_string(),
        "pagedown" => "PageDown".to_string(),
        _ => {
            let mut chars = key.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
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
                    .text_size(self.size.rems(cx))
                    .child(key)
            }))
    }
}

#[cfg(test)]
#[path = "key_binding_tests.rs"]
mod tests;
