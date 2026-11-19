//! The token structs: [`ThemeColors`] for the layers of the window and [`StatusColors`]
//! for the things a status can be. Both are filled from a theme's JSON, so every token a
//! theme defines is a field here and a key there — a key the structs do not know fails to
//! parse, which is how a typo or a missing token gets caught by
//! `both_bundled_themes_parse` instead of by a screen drawing the wrong color.

use gpui::Hsla;
use serde::Deserialize;

/// The layers, borders, text and accents of one theme.
///
/// Token names are ours (`.agents/skills/rust-gpui-ui-ux/references/design-system.md` §1);
/// the *values* of the roles the redesign pack defines are mapped onto them rather than
/// renaming our tokens to the pack's:
///
/// | Pack role (`design_tokens.json`) | Our token |
/// | --- | --- |
/// | `canvas` | [`ThemeColors::background`] |
/// | `surface` | [`ThemeColors::surface_background`] |
/// | `raised` | [`ThemeColors::panel_background`] |
/// | `text/primary` | [`ThemeColors::text`] |
/// | `text/secondary` | [`ThemeColors::text_muted`] |
/// | `border/subtle` | [`ThemeColors::border_variant`] |
/// | `border/control` | [`ThemeColors::border`] |
/// | `accent/action` | [`ThemeColors::accent_background`] |
/// | `accent/on-action` | [`ThemeColors::accent_foreground`] |
/// | `focus` | [`ThemeColors::border_focused`] |
/// | `selection` | [`ThemeColors::element_selection_background`] |
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeColors {
    /// The window behind everything else (pack `canvas`).
    pub background: Hsla,
    /// The main content sheet (pack `surface`).
    pub surface_background: Hsla,
    /// Sidebars and cards sitting on the content sheet (pack `raised`).
    pub panel_background: Hsla,
    /// Menus, popovers and tooltips: the layer that floats above the content.
    pub elevated_surface_background: Hsla,
    /// Request and response editors, where code is read.
    pub editor_background: Hsla,

    /// Strong dividers and control outlines (pack `border/control`).
    pub border: Hsla,
    /// Subtle internal dividers (pack `border/subtle`).
    pub border_variant: Hsla,
    /// The focus ring (pack `focus`).
    pub border_focused: Hsla,
    /// Marks the thing that is currently selected (pack `accent/action`).
    pub border_selected: Hsla,
    /// A border that must take part in layout without drawing.
    pub border_transparent: Hsla,
    /// A divider or outline that is turned off.
    pub border_disabled: Hsla,

    /// The fill of filled controls — inputs and filled buttons.
    pub element_background: Hsla,
    /// A filled control under the pointer.
    pub element_hover: Hsla,
    /// A filled control being pressed.
    pub element_active: Hsla,
    /// A filled control that is on (toggled, chosen).
    pub element_selected: Hsla,
    /// A filled control that is turned off.
    pub element_disabled: Hsla,
    /// The highlight behind selected text (pack `selection`).
    pub element_selection_background: Hsla,

    /// The fill of transparent controls — list rows, subtle buttons, tabs.
    pub ghost_element_background: Hsla,
    /// A transparent control under the pointer.
    pub ghost_element_hover: Hsla,
    /// A transparent control being pressed.
    pub ghost_element_active: Hsla,
    /// A transparent control that is on (pack `selection`).
    pub ghost_element_selected: Hsla,
    /// A transparent control that is turned off.
    pub ghost_element_disabled: Hsla,

    /// Body text (pack `text/primary`).
    pub text: Hsla,
    /// Secondary labels and metadata (pack `text/secondary`).
    pub text_muted: Hsla,
    /// The hint text shown in an empty input.
    pub text_placeholder: Hsla,
    /// Text belonging to a control that is turned off. Disabled controls are exempt
    /// from the 4.5:1 rule, so this token is not in the contrast table.
    pub text_disabled: Hsla,
    /// Text that carries the accent: links, the active tab title.
    pub text_accent: Hsla,

    /// An icon next to body text.
    pub icon: Hsla,
    /// An icon next to secondary text.
    pub icon_muted: Hsla,
    /// An icon in a control that is turned off.
    pub icon_disabled: Hsla,
    /// An icon that carries the accent.
    pub icon_accent: Hsla,

    /// The top bar of the window.
    pub title_bar_background: Hsla,
    /// The bottom strip: connection state, environment, console.
    pub status_bar_background: Hsla,
    /// The strip above the editor: URL bar and request actions.
    pub toolbar_background: Hsla,
    /// The strip the request tabs live in.
    pub tab_bar_background: Hsla,
    /// The tab that is showing its contents.
    pub tab_active_background: Hsla,
    /// A tab that is not showing its contents.
    pub tab_inactive_background: Hsla,

    /// The border of the pane group that has keyboard focus.
    pub pane_focused_border: Hsla,
    /// The divider between panes.
    pub pane_group_border: Hsla,
    /// The border of the panel that has keyboard focus.
    pub panel_focused_border: Hsla,

    /// The scrollbar thumb.
    pub scrollbar_thumb_background: Hsla,
    /// The scrollbar thumb under the pointer.
    pub scrollbar_thumb_hover_background: Hsla,
    /// The trough the scrollbar thumb runs in.
    pub scrollbar_track_background: Hsla,

    /// The surface a dragged item may land on.
    pub drop_target_background: Hsla,
    /// The outline of a valid drop target.
    pub drop_target_border: Hsla,

    /// The highlight behind a find-in-response match.
    pub search_match_background: Hsla,

    /// A link under the pointer.
    pub link_text_hover: Hsla,

    /// The fill of the primary action (pack `accent/action`) — Send and friends.
    pub accent_background: Hsla,
    /// Text and icons drawn on [`Self::accent_background`] (pack `accent/on-action`).
    pub accent_foreground: Hsla,

    /// A key in a JSON response body.
    pub syntax_key: Hsla,
    /// A string in a response body.
    pub syntax_string: Hsla,
    /// A number in a response body.
    pub syntax_number: Hsla,
    /// A boolean in a response body.
    pub syntax_boolean: Hsla,
    /// A null in a response body.
    pub syntax_null: Hsla,
    /// Braces, brackets and commas in a response body.
    pub syntax_punctuation: Hsla,

    /// A `{{variable}}` that resolved to a value.
    pub variable_resolved: Hsla,
    /// A `{{variable}}` that has no value.
    pub variable_unresolved: Hsla,
}

/// What a status can be: how a request ended, how an HTTP response is classified, and
/// which method a request uses.
///
/// The four semantics carry the design system's `_background`/`_border` variants so a
/// banner or a status pill can be drawn from one family. The HTTP classes color the
/// status code itself, one hue per class; they are separate from the semantics because
/// a 4xx response is a *client* error while `error` is the semantic the UI uses for "this
/// failed".
///
/// The method hues keep Postman's mapping (GET green, POST amber, PUT blue, PATCH
/// purple, DELETE red), retuned per theme so each one clears 4.5:1 on the row it is
/// drawn on. Method labels always spell the method out — the hue never carries the
/// meaning on its own.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusColors {
    /// "This went well": 2xx responses, resolved variables, passing tests.
    pub success: Hsla,
    /// The tinted background of a success banner or pill.
    pub success_background: Hsla,
    /// The outline of a success banner or pill.
    pub success_border: Hsla,

    /// "Look at this": skipped tests, disabled TLS verification, expiring variables.
    pub warning: Hsla,
    /// The tinted background of a warning banner or pill.
    pub warning_background: Hsla,
    /// The outline of a warning banner or pill.
    pub warning_border: Hsla,

    /// "This failed": transport errors, 4xx and 5xx responses, failing tests.
    pub error: Hsla,
    /// The tinted background of an error banner or pill.
    pub error_background: Hsla,
    /// The outline of an error banner or pill.
    pub error_border: Hsla,

    /// "You may want to know": redirects, notes, informational results.
    pub info: Hsla,
    /// The tinted background of an info banner or pill.
    pub info_background: Hsla,
    /// The outline of an info banner or pill.
    pub info_border: Hsla,

    /// A `1xx` status code.
    pub http_informational: Hsla,
    /// A `2xx` status code.
    pub http_success: Hsla,
    /// A `3xx` status code.
    pub http_redirect: Hsla,
    /// A `4xx` status code.
    pub http_client_error: Hsla,
    /// A `5xx` status code.
    pub http_server_error: Hsla,

    /// The `GET` method label.
    pub method_get: Hsla,
    /// The `POST` method label.
    pub method_post: Hsla,
    /// The `PUT` method label.
    pub method_put: Hsla,
    /// The `PATCH` method label.
    pub method_patch: Hsla,
    /// The `DELETE` method label.
    pub method_delete: Hsla,
    /// The `HEAD` method label.
    pub method_head: Hsla,
    /// The `OPTIONS` method label.
    pub method_options: Hsla,
}
