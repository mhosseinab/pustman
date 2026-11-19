use std::cell::Cell;
use std::rc::Rc;

use gpui::TestAppContext;

use crate::{Setting, Settings, SettingsError, SettingsStore, Theme, load_defaults};

#[test]
fn defaults_load() {
    // What a user who changes nothing gets: the documented values of
    // assets/settings/default.json (REQ-UI-08, REQ-HTTP-09, REQ-HIST-04, REQ-CON-02),
    // read through the embedded copy so a drift in either place fails here.
    let settings = load_defaults();

    assert_eq!(settings.theme, Theme::System);
    assert_eq!(settings.ui_font_size, 16);
    assert_eq!(settings.buffer_font_size, 14);
    assert_eq!(settings.request.timeout_ms, 30_000);
    assert!(settings.request.follow_redirects);
    assert_eq!(settings.request.max_redirects, 10);
    assert!(settings.request.verify_tls);
    assert_eq!(settings.request.response_pretty_limit_mb, 10);
    assert_eq!(settings.history_retention_days, 30);
    assert_eq!(settings.console_max_entries, 5000);
}

#[gpui::test]
fn user_value_overrides_default(cx: &mut TestAppContext) {
    // One key the user sets wins over the bundled default, every key they leave out keeps
    // it (REQ-UI-08 and friends), and the value is read back through the settings store.
    let user_json = r#"{"ui_font_size": 20}"#;

    cx.update(|cx| {
        let init = SettingsStore::init(cx, Some(user_json));
        assert_eq!(init, Ok(()), "a valid user blob merges over the defaults");

        Settings::register(cx);

        let settings = Settings::get_global(cx);
        assert_eq!(settings.ui_font_size, 20, "the user's value wins");
        assert_eq!(
            *settings,
            Settings {
                ui_font_size: 20,
                ..Settings::default()
            },
            "every other field keeps its default"
        );
    });
}

#[gpui::test]
fn invalid_user_file_keeps_last_good_values_and_reports_error(cx: &mut TestAppContext) {
    // A malformed user file must never destroy the settings the store was built on
    // (REQ-UI-08): the last good values stand, and the parse error is reported to the
    // caller and recorded for the UI to show.
    cx.update(|cx| {
        let init = SettingsStore::init(cx, Some(r#"{"ui_font_size": 20}"#));
        assert_eq!(init, Ok(()), "the good user blob merges over the defaults");
        Settings::register(cx);
    });

    cx.update(|cx| {
        let error = SettingsStore::reload(cx, Some("{not json"));
        assert!(
            matches!(error, Err(SettingsError::Parse(_))),
            "a malformed blob is a parse error the caller can show"
        );
        assert_eq!(
            *Settings::get_global(cx),
            Settings {
                ui_font_size: 20,
                ..Settings::default()
            },
            "the last good values stand"
        );
        assert!(
            matches!(SettingsStore::last_error(cx), Some(SettingsError::Parse(_))),
            "the error is recorded for the UI to show too"
        );
    });
}

#[gpui::test]
fn observer_fires_on_change(cx: &mut TestAppContext) {
    // A subscriber of the store is notified when a reload changes a value, and the
    // notification sees the new value (settings apply live, REQ-UI-08).
    let notifications = Rc::new(Cell::new(0));

    cx.update(|cx| {
        let init = SettingsStore::init(cx, Some(r#"{"ui_font_size": 16}"#));
        assert_eq!(
            init,
            Ok(()),
            "the starting user blob merges over the defaults"
        );
        Settings::register(cx);
    });

    let _subscription = cx.update(|cx| {
        let notifications = Rc::clone(&notifications);
        cx.observe_global::<SettingsStore>(move |cx| {
            notifications.set(notifications.get() + 1);
            assert_eq!(
                Settings::get_global(cx).ui_font_size,
                20,
                "the notification sees the new value"
            );
        })
    });
    // `observe_global` activates on a deferred effect: let it land before the change.
    cx.run_until_parked();
    assert_eq!(notifications.get(), 0, "no change yet, so no notification");

    cx.update(|cx| {
        let reload = SettingsStore::reload(cx, Some(r#"{"ui_font_size": 20}"#));
        assert_eq!(reload, Ok(()), "the changed blob merges over the defaults");
    });
    cx.run_until_parked();

    assert_eq!(
        notifications.get(),
        1,
        "the subscriber is notified when the value changes"
    );
    cx.update(|cx| assert_eq!(Settings::get_global(cx).ui_font_size, 20));
}
