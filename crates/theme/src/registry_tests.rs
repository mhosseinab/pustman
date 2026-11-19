use std::sync::Arc;

use crate::{Appearance, ThemeRegistry, relative_luminance};

#[test]
fn both_bundled_themes_parse() {
    // Both files in `assets/themes/` are themes this crate can read, each one carrying
    // every token — a key that is not a token, or a token that is missing, fails to
    // parse and lands here — and each declaring the appearance its name promises
    // (REQ-UI-06).
    let registry = ThemeRegistry::bundled().expect("the bundled themes parse");

    let light = registry.for_appearance(Appearance::Light);
    assert_eq!(light.name(), "Pustman Light");
    assert_eq!(light.appearance(), Appearance::Light);

    let dark = registry.for_appearance(Appearance::Dark);
    assert_eq!(dark.name(), "Pustman Dark");
    assert_eq!(dark.appearance(), Appearance::Dark);

    assert!(
        registry
            .get("Pustman Light")
            .is_some_and(|found| Arc::ptr_eq(found, light)),
        "a name finds the theme behind it"
    );
    assert!(
        registry
            .get("Pustman Dark")
            .is_some_and(|found| Arc::ptr_eq(found, dark)),
        "a name finds the theme behind it"
    );
    assert!(
        registry.get("Zed One Dark").is_none(),
        "an unknown name finds nothing"
    );

    // Parsing is not enough: the light theme has to be the lighter one, so two files
    // that swapped places cannot pass as "the light and dark themes are bundled".
    assert!(
        relative_luminance(light.colors().background)
            > relative_luminance(dark.colors().background),
        "the light theme's background is the lighter of the two"
    );
}
