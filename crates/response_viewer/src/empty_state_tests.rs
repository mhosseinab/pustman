//! The response pane's empty state must read in both bundled themes (#266).
//!
//! The "No response yet" heading took no colour of its own, so it inherited GPUI's
//! default text colour — near-black, and near-invisible on the dark theme
//! (`ds-dark-3-url-focus.png`). It is pinned here to the theme's body-text token
//! (`text/primary` in the redesign pack's theme roles) and held to the contrast the
//! pack's accessibility contract asks of normal text.

use gpui::TestAppContext;
use theme::{ActiveTheme as _, Appearance, ThemeStore, contrast_ratio};
use ui::test_support::init_test;

use crate::EMPTY_STATE_HEADING_COLOR;

/// WCAG 2.2 AA for normal text — `01_specs/design_system.md`'s accessibility contract.
const MIN_TEXT_CONTRAST: f64 = 4.5;

#[gpui::test]
fn test_empty_state_heading_meets_contrast_in_both_themes(cx: &mut TestAppContext) {
    cx.update(init_test);
    for appearance in [Appearance::Light, Appearance::Dark] {
        cx.update(|cx| {
            ThemeStore::set_system_appearance(cx, appearance);
            assert_eq!(
                EMPTY_STATE_HEADING_COLOR.color(cx),
                cx.theme().colors().text,
                "{appearance:?}: the heading is drawn in the theme's body-text token, \
                 not an inherited default"
            );
            // The workbench the request editor draws in, and the dock it can be pinned
            // to: the empty state reads on either.
            for background in [
                cx.theme().colors().background,
                cx.theme().colors().panel_background,
            ] {
                let ratio = contrast_ratio(EMPTY_STATE_HEADING_COLOR.color(cx), background);
                assert!(
                    ratio >= MIN_TEXT_CONTRAST,
                    "{appearance:?}: the empty-state heading is {ratio:.2}:1 on the pane \
                     background, under the 4.5:1 text minimum"
                );
            }
        });
    }
}
