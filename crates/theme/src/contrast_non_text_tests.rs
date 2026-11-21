//! The 3:1 half of the contrast contract: what the user has to see but does not read.
//! The pack's `05_validation/token_contrast.csv` pins `focus` and `border/control` on
//! `surface` at 3:1, and design-system.md §7 holds icons to the same minimum. The 4.5:1
//! text pairs live in `contrast_tests.rs`.

use gpui::Hsla;

use crate::{Theme, ThemeRegistry, contrast_ratio};

/// A named token reader.
type NamedToken = (&'static str, fn(&Theme) -> Hsla);

type ContrastPair = (&'static str, fn(&Theme) -> Hsla, fn(&Theme) -> Hsla);

/// The layers a control, a focus ring or an icon is drawn on.
const LAYERS: &[NamedToken] = &[
    ("colors.background", |theme| theme.colors().background),
    ("colors.surface_background", |theme| {
        theme.colors().surface_background
    }),
    ("colors.panel_background", |theme| {
        theme.colors().panel_background
    }),
    ("colors.elevated_surface_background", |theme| {
        theme.colors().elevated_surface_background
    }),
    ("colors.editor_background", |theme| {
        theme.colors().editor_background
    }),
    ("colors.tab_bar_background", |theme| {
        theme.colors().tab_bar_background
    }),
];

/// The non-text tokens that carry meaning. `border_variant` is missing on purpose: the
/// pack calls subtle borders decorative, so they are exempt (WCAG 1.4.11 covers only
/// what identifies a control or its state). `icon_disabled` is exempt like
/// `text_disabled`.
const FOREGROUNDS: &[NamedToken] = &[
    ("colors.border", |theme| theme.colors().border),
    ("colors.border_focused", |theme| {
        theme.colors().border_focused
    }),
    ("colors.icon", |theme| theme.colors().icon),
    ("colors.icon_muted", |theme| theme.colors().icon_muted),
    ("colors.icon_accent", |theme| theme.colors().icon_accent),
];

/// The filled primary button's edge against the surface it sits on: a control boundary.
const EXTRA_PAIRS: &[ContrastPair] = &[(
    "colors.accent_background on colors.surface_background",
    |theme| theme.colors().accent_background,
    |theme| theme.colors().surface_background,
)];

#[test]
fn test_controls_focus_rings_and_icons_reach_3_to_1_on_every_layer() {
    let registry = ThemeRegistry::bundled().expect("the bundled themes parse");
    for theme in registry.themes() {
        let theme: &Theme = theme;
        let mut failures = Vec::new();
        for (foreground_name, foreground) in FOREGROUNDS {
            for (layer_name, layer) in LAYERS {
                let ratio = contrast_ratio(foreground(theme), layer(theme));
                if ratio < 3.0 {
                    failures.push(format!("{foreground_name} on {layer_name}: {ratio:.2}:1"));
                }
            }
        }
        for (pair, foreground, background) in EXTRA_PAIRS {
            let ratio = contrast_ratio(foreground(theme), background(theme));
            if ratio < 3.0 {
                failures.push(format!("{pair}: {ratio:.2}:1"));
            }
        }
        assert!(
            failures.is_empty(),
            "{}: below the 3:1 non-text minimum: {failures:#?}",
            theme.name()
        );
    }
}
