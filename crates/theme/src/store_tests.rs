use gpui::TestAppContext;
use settings::SettingsStore;

use crate::{ActiveTheme, Appearance, ThemeStore, init};

#[gpui::test]
fn system_setting_follows_appearance(cx: &mut TestAppContext) {
    // `system` — what a user who changes nothing gets — draws the theme for whatever the
    // window reports, and a `theme` of `light` or `dark` overrides that (REQ-UI-06).
    cx.update(|cx| {
        SettingsStore::init(cx, None).expect("the bundled settings parse");
        init(cx).expect("the bundled themes parse");
        assert_eq!(
            cx.theme().appearance(),
            Appearance::Light,
            "the window this test runs in reports light"
        );
    });

    cx.update(|cx| ThemeStore::set_system_appearance(cx, Appearance::Dark));
    cx.update(|cx| assert_eq!(cx.theme().appearance(), Appearance::Dark));

    cx.update(|cx| ThemeStore::set_system_appearance(cx, Appearance::Light));
    cx.update(|cx| assert_eq!(cx.theme().appearance(), Appearance::Light));

    // A forced selection wins over the window ...
    cx.update(|cx| {
        SettingsStore::reload(cx, Some(r#"{"theme":"dark"}"#)).expect("the user blob parses");
    });
    cx.run_until_parked();
    cx.update(|cx| ThemeStore::set_system_appearance(cx, Appearance::Light));
    cx.update(|cx| {
        assert_eq!(
            cx.theme().appearance(),
            Appearance::Dark,
            "the forced selection wins over the window appearance"
        );
    });

    // ... and `system` hands the choice back to the window.
    cx.update(|cx| {
        SettingsStore::reload(cx, Some(r#"{"theme":"system"}"#)).expect("the user blob parses");
    });
    cx.run_until_parked();
    cx.update(|cx| ThemeStore::set_system_appearance(cx, Appearance::Dark));
    cx.update(|cx| assert_eq!(cx.theme().appearance(), Appearance::Dark));
    cx.update(|cx| ThemeStore::set_system_appearance(cx, Appearance::Light));
    cx.update(|cx| assert_eq!(cx.theme().appearance(), Appearance::Light));
}

#[gpui::test]
fn forced_light_overrides_a_dark_window(cx: &mut TestAppContext) {
    // The mirror of the forced-`dark` case above: `theme: light` must draw the light theme
    // while the window reports dark (REQ-UI-06). Without this the `Light` arm of the
    // selection match could map to the wrong appearance and nothing would notice.
    cx.update(|cx| {
        SettingsStore::init(cx, None).expect("the bundled settings parse");
        init(cx).expect("the bundled themes parse");
    });
    cx.update(|cx| ThemeStore::set_system_appearance(cx, Appearance::Dark));
    cx.update(|cx| {
        SettingsStore::reload(cx, Some(r#"{"theme":"light"}"#)).expect("the user blob parses");
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            cx.theme().appearance(),
            Appearance::Light,
            "the forced light selection wins over the window appearance"
        );
    });
}
