use gpui::Hsla;

use crate::{Theme, ThemeRegistry, contrast_ratio};

/// A pair to check: what it is called, and how to reach its two colors in a theme.
type ContrastPair = (&'static str, fn(&Theme) -> Hsla, fn(&Theme) -> Hsla);

/// Every pair of a text token and the background it is actually drawn on (REQ-NFR-06:
/// 4.5:1 for text in the bundled themes). Body text, secondary labels, placeholder text,
/// accent text and every colored word — a method label, a status code, a syntax token, a
/// `{{variable}}` — is text to a reader, so each is here against the layers it lands on.
///
/// `text_disabled` is the one omission: a control that is turned off is exempt from
/// WCAG 1.4.3, and holding it to 4.5:1 would make disabled text look enabled.
const CONTRAST_PAIRS: &[ContrastPair] = &[
    (
        "colors.text on colors.background",
        |theme| theme.colors().text,
        |theme| theme.colors().background,
    ),
    (
        "colors.text on colors.surface_background",
        |theme| theme.colors().text,
        |theme| theme.colors().surface_background,
    ),
    (
        "colors.text on colors.panel_background",
        |theme| theme.colors().text,
        |theme| theme.colors().panel_background,
    ),
    (
        "colors.text on colors.elevated_surface_background",
        |theme| theme.colors().text,
        |theme| theme.colors().elevated_surface_background,
    ),
    (
        "colors.text on colors.editor_background",
        |theme| theme.colors().text,
        |theme| theme.colors().editor_background,
    ),
    (
        "colors.text on colors.element_background",
        |theme| theme.colors().text,
        |theme| theme.colors().element_background,
    ),
    (
        "colors.text on colors.element_hover",
        |theme| theme.colors().text,
        |theme| theme.colors().element_hover,
    ),
    (
        "colors.text on colors.element_active",
        |theme| theme.colors().text,
        |theme| theme.colors().element_active,
    ),
    (
        "colors.text on colors.element_selected",
        |theme| theme.colors().text,
        |theme| theme.colors().element_selected,
    ),
    (
        "colors.text on colors.element_selection_background",
        |theme| theme.colors().text,
        |theme| theme.colors().element_selection_background,
    ),
    (
        "colors.text on colors.ghost_element_hover",
        |theme| theme.colors().text,
        |theme| theme.colors().ghost_element_hover,
    ),
    (
        "colors.text on colors.ghost_element_active",
        |theme| theme.colors().text,
        |theme| theme.colors().ghost_element_active,
    ),
    (
        "colors.text on colors.ghost_element_selected",
        |theme| theme.colors().text,
        |theme| theme.colors().ghost_element_selected,
    ),
    (
        "colors.text on colors.search_match_background",
        |theme| theme.colors().text,
        |theme| theme.colors().search_match_background,
    ),
    (
        "colors.text on colors.title_bar_background",
        |theme| theme.colors().text,
        |theme| theme.colors().title_bar_background,
    ),
    (
        "colors.text on colors.status_bar_background",
        |theme| theme.colors().text,
        |theme| theme.colors().status_bar_background,
    ),
    (
        "colors.text on colors.toolbar_background",
        |theme| theme.colors().text,
        |theme| theme.colors().toolbar_background,
    ),
    (
        "colors.text on colors.tab_bar_background",
        |theme| theme.colors().text,
        |theme| theme.colors().tab_bar_background,
    ),
    (
        "colors.text on colors.tab_active_background",
        |theme| theme.colors().text,
        |theme| theme.colors().tab_active_background,
    ),
    (
        "colors.text on colors.tab_inactive_background",
        |theme| theme.colors().text,
        |theme| theme.colors().tab_inactive_background,
    ),
    (
        "colors.text on status.success_background",
        |theme| theme.colors().text,
        |theme| theme.status().success_background,
    ),
    (
        "colors.text on status.warning_background",
        |theme| theme.colors().text,
        |theme| theme.status().warning_background,
    ),
    (
        "colors.text on status.error_background",
        |theme| theme.colors().text,
        |theme| theme.status().error_background,
    ),
    (
        "colors.text on status.info_background",
        |theme| theme.colors().text,
        |theme| theme.status().info_background,
    ),
    (
        "colors.text_muted on colors.background",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().background,
    ),
    (
        "colors.text_muted on colors.surface_background",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().surface_background,
    ),
    (
        "colors.text_muted on colors.panel_background",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().panel_background,
    ),
    (
        "colors.text_muted on colors.elevated_surface_background",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().elevated_surface_background,
    ),
    (
        "colors.text_muted on colors.editor_background",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().editor_background,
    ),
    (
        "colors.text_muted on colors.element_background",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().element_background,
    ),
    (
        "colors.text_muted on colors.ghost_element_hover",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().ghost_element_hover,
    ),
    (
        "colors.text_muted on colors.ghost_element_selected",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().ghost_element_selected,
    ),
    (
        "colors.text_muted on colors.title_bar_background",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().title_bar_background,
    ),
    (
        "colors.text_muted on colors.status_bar_background",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().status_bar_background,
    ),
    (
        "colors.text_muted on colors.tab_bar_background",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().tab_bar_background,
    ),
    (
        "colors.text_muted on colors.tab_inactive_background",
        |theme| theme.colors().text_muted,
        |theme| theme.colors().tab_inactive_background,
    ),
    (
        "colors.text_placeholder on colors.element_background",
        |theme| theme.colors().text_placeholder,
        |theme| theme.colors().element_background,
    ),
    (
        "colors.text_placeholder on colors.surface_background",
        |theme| theme.colors().text_placeholder,
        |theme| theme.colors().surface_background,
    ),
    (
        "colors.text_accent on colors.background",
        |theme| theme.colors().text_accent,
        |theme| theme.colors().background,
    ),
    (
        "colors.text_accent on colors.surface_background",
        |theme| theme.colors().text_accent,
        |theme| theme.colors().surface_background,
    ),
    (
        "colors.text_accent on colors.panel_background",
        |theme| theme.colors().text_accent,
        |theme| theme.colors().panel_background,
    ),
    (
        "colors.link_text_hover on colors.surface_background",
        |theme| theme.colors().link_text_hover,
        |theme| theme.colors().surface_background,
    ),
    (
        "colors.accent_foreground on colors.accent_background",
        |theme| theme.colors().accent_foreground,
        |theme| theme.colors().accent_background,
    ),
    (
        "status.success on colors.surface_background",
        |theme| theme.status().success,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.warning on colors.surface_background",
        |theme| theme.status().warning,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.error on colors.surface_background",
        |theme| theme.status().error,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.info on colors.surface_background",
        |theme| theme.status().info,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.success on colors.background",
        |theme| theme.status().success,
        |theme| theme.colors().background,
    ),
    (
        "status.warning on colors.background",
        |theme| theme.status().warning,
        |theme| theme.colors().background,
    ),
    (
        "status.error on colors.background",
        |theme| theme.status().error,
        |theme| theme.colors().background,
    ),
    (
        "status.info on colors.background",
        |theme| theme.status().info,
        |theme| theme.colors().background,
    ),
    (
        "status.http_informational on colors.surface_background",
        |theme| theme.status().http_informational,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.http_success on colors.surface_background",
        |theme| theme.status().http_success,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.http_redirect on colors.surface_background",
        |theme| theme.status().http_redirect,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.http_client_error on colors.surface_background",
        |theme| theme.status().http_client_error,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.http_server_error on colors.surface_background",
        |theme| theme.status().http_server_error,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.http_informational on colors.background",
        |theme| theme.status().http_informational,
        |theme| theme.colors().background,
    ),
    (
        "status.http_success on colors.background",
        |theme| theme.status().http_success,
        |theme| theme.colors().background,
    ),
    (
        "status.http_redirect on colors.background",
        |theme| theme.status().http_redirect,
        |theme| theme.colors().background,
    ),
    (
        "status.http_client_error on colors.background",
        |theme| theme.status().http_client_error,
        |theme| theme.colors().background,
    ),
    (
        "status.http_server_error on colors.background",
        |theme| theme.status().http_server_error,
        |theme| theme.colors().background,
    ),
    (
        "status.method_get on colors.surface_background",
        |theme| theme.status().method_get,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.method_post on colors.surface_background",
        |theme| theme.status().method_post,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.method_put on colors.surface_background",
        |theme| theme.status().method_put,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.method_patch on colors.surface_background",
        |theme| theme.status().method_patch,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.method_delete on colors.surface_background",
        |theme| theme.status().method_delete,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.method_head on colors.surface_background",
        |theme| theme.status().method_head,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.method_options on colors.surface_background",
        |theme| theme.status().method_options,
        |theme| theme.colors().surface_background,
    ),
    (
        "status.method_get on colors.background",
        |theme| theme.status().method_get,
        |theme| theme.colors().background,
    ),
    (
        "status.method_post on colors.background",
        |theme| theme.status().method_post,
        |theme| theme.colors().background,
    ),
    (
        "status.method_put on colors.background",
        |theme| theme.status().method_put,
        |theme| theme.colors().background,
    ),
    (
        "status.method_patch on colors.background",
        |theme| theme.status().method_patch,
        |theme| theme.colors().background,
    ),
    (
        "status.method_delete on colors.background",
        |theme| theme.status().method_delete,
        |theme| theme.colors().background,
    ),
    (
        "status.method_head on colors.background",
        |theme| theme.status().method_head,
        |theme| theme.colors().background,
    ),
    (
        "status.method_options on colors.background",
        |theme| theme.status().method_options,
        |theme| theme.colors().background,
    ),
    (
        "colors.syntax_key on colors.editor_background",
        |theme| theme.colors().syntax_key,
        |theme| theme.colors().editor_background,
    ),
    (
        "colors.syntax_string on colors.editor_background",
        |theme| theme.colors().syntax_string,
        |theme| theme.colors().editor_background,
    ),
    (
        "colors.syntax_number on colors.editor_background",
        |theme| theme.colors().syntax_number,
        |theme| theme.colors().editor_background,
    ),
    (
        "colors.syntax_boolean on colors.editor_background",
        |theme| theme.colors().syntax_boolean,
        |theme| theme.colors().editor_background,
    ),
    (
        "colors.syntax_null on colors.editor_background",
        |theme| theme.colors().syntax_null,
        |theme| theme.colors().editor_background,
    ),
    (
        "colors.syntax_punctuation on colors.editor_background",
        |theme| theme.colors().syntax_punctuation,
        |theme| theme.colors().editor_background,
    ),
    (
        "colors.variable_resolved on colors.editor_background",
        |theme| theme.colors().variable_resolved,
        |theme| theme.colors().editor_background,
    ),
    (
        "colors.variable_unresolved on colors.editor_background",
        |theme| theme.colors().variable_unresolved,
        |theme| theme.colors().editor_background,
    ),
    (
        "colors.variable_resolved on colors.element_background",
        |theme| theme.colors().variable_resolved,
        |theme| theme.colors().element_background,
    ),
    (
        "colors.variable_unresolved on colors.element_background",
        |theme| theme.colors().variable_unresolved,
        |theme| theme.colors().element_background,
    ),
];

#[test]
fn text_contrast_at_least_4_5_to_1() {
    // Both bundled themes hold up the accessibility contract (REQ-NFR-06) for every
    // text/background pair the app will draw — computed here from the WCAG formula, so a
    // value that drifts in `assets/themes/` fails this before it reaches a screen.
    let registry = ThemeRegistry::bundled().expect("the bundled themes parse");
    for theme in registry.themes() {
        let theme: &Theme = theme;
        for (pair, foreground, background) in CONTRAST_PAIRS {
            let ratio = contrast_ratio(foreground(theme), background(theme));
            assert!(
                ratio >= 4.5,
                "{}: {} is {:.2}:1 against the 4.5:1 text minimum",
                theme.name(),
                pair,
                ratio
            );
        }
    }
}
