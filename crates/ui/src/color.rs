//! The semantic colors a label, icon or control is drawn with.
//!
//! A component never picks an [`gpui::Hsla`]: it asks for an intent — [`Color::Muted`],
//! [`Color::Error`] — and resolves it against the active theme here. That is what keeps
//! light and dark correct and keeps contrast checked in one place
//! (REQ-NFR-06; `references/design-system.md` §2).

use gpui::{App, Hsla};
use theme::ActiveTheme;

/// The intent behind the color of a piece of text or an icon.
///
/// The vocabulary is the design system's (`references/design-system.md` §2). The theme
/// crate (S18) defines the four semantics `success`/`warning`/`error`/`info` plus their
/// `_background`/`_border` variants, so the git-review states map onto the nearest of
/// those — a `Created` row is green because `created` means "added", and the row still
/// spells the state out in words (state is never carried by color alone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    /// Body text.
    Default,
    /// Secondary labels and metadata.
    Muted,
    /// Informational hints next to a control.
    Hint,
    /// The hint text shown in an empty input.
    Placeholder,
    /// Text belonging to a control that is turned off.
    Disabled,
    /// Links, the active tab title: text that carries the accent.
    Accent,
    /// Text on a selected surface.
    Selected,
    /// Data that is not shown to the user by default (hidden git state).
    Hidden,
    /// This failed.
    Error,
    /// Look at this.
    Warning,
    /// You may want to know.
    Info,
    /// This went well.
    Success,
    /// Added since the last revision.
    Created,
    /// Changed since the last revision.
    Modified,
    /// Removed since the last revision.
    Deleted,
    /// Two revisions disagree.
    Conflict,
    /// Deliberately not compared (ignored git state).
    Ignored,
    /// An escape hatch for colors the theme has no token for. Needs a reviewer-justified
    /// reason at the call site; prefer one of the tokens above.
    Custom(Hsla),
}

impl Color {
    /// Resolve this intent against the active theme.
    pub fn color(self, cx: &App) -> Hsla {
        let theme = cx.theme();
        match self {
            Self::Default => theme.colors().text,
            Self::Muted | Self::Hint | Self::Hidden | Self::Ignored => theme.colors().text_muted,
            Self::Placeholder => theme.colors().text_placeholder,
            Self::Disabled => theme.colors().text_disabled,
            Self::Accent | Self::Selected => theme.colors().text_accent,
            Self::Error | Self::Deleted | Self::Conflict => theme.status().error,
            Self::Warning | Self::Modified => theme.status().warning,
            Self::Info => theme.status().info,
            Self::Success | Self::Created => theme.status().success,
            Self::Custom(color) => color,
        }
    }
}

/// The theme's method token for a method label (`references/design-system.md` §1: the
/// HTTP verbs are theme tokens). [`Color::Custom`] only resolves the token — the theme
/// still owns the value — and a verb with no token (Trace, Connect, a custom method)
/// falls back to muted text. The verb's text always shows beside the colour: method
/// colour alone carries no meaning.
pub fn method_color(method: &str, cx: &App) -> Color {
    let status = cx.theme().status();
    let token = match method {
        "GET" => status.method_get,
        "POST" => status.method_post,
        "PUT" => status.method_put,
        "PATCH" => status.method_patch,
        "DELETE" => status.method_delete,
        "HEAD" => status.method_head,
        "OPTIONS" => status.method_options,
        _ => return Color::Muted,
    };
    Color::Custom(token)
}

/// How serious a message is: the tone a badge, toast or callout is drawn in.
///
/// Each severity owns a text color, a tinted background and an outline in the theme
/// (`StatusColors`), so a message reads the same wherever it appears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// You may want to know.
    Info,
    /// This went well.
    Success,
    /// Look at this.
    Warning,
    /// This failed.
    Error,
}

impl Severity {
    /// The text and icon color for this severity.
    pub fn color(self, cx: &App) -> Hsla {
        let status = cx.theme().status();
        match self {
            Self::Info => status.info,
            Self::Success => status.success,
            Self::Warning => status.warning,
            Self::Error => status.error,
        }
    }

    /// The tinted background of a banner, badge or pill.
    pub fn background(self, cx: &App) -> Hsla {
        let status = cx.theme().status();
        match self {
            Self::Info => status.info_background,
            Self::Success => status.success_background,
            Self::Warning => status.warning_background,
            Self::Error => status.error_background,
        }
    }

    /// The outline of a banner, badge or pill.
    pub fn border(self, cx: &App) -> Hsla {
        let status = cx.theme().status();
        match self {
            Self::Info => status.info_border,
            Self::Success => status.success_border,
            Self::Warning => status.warning_border,
            Self::Error => status.error_border,
        }
    }
}
