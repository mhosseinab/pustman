//! A live `console_max_entries` change trims the ring buffer to the new cap (#109):
//! [`ConsoleStore::set_max_entries`] is reached only through the settings subscription
//! `crate::init` installs, so the change here is made the way a user makes one — a new
//! settings blob — and not by calling the store's internals.

use std::time::Duration;

use gpui::TestAppContext;

use crate::{ConsoleEntry, ConsoleLevel, ConsoleStore};

/// One ordinary entry about `url`, everything else empty or unremarkable.
fn entry(url: &str) -> ConsoleEntry {
    ConsoleEntry {
        level: ConsoleLevel::Info,
        protocol: "http".to_string(),
        method: Some(api_model::Method::Get),
        url: url.to_string(),
        status: Some(200),
        time: Some(Duration::from_millis(12)),
        size: Some(34),
        raw_request: String::new(),
        raw_response: String::new(),
    }
}

/// The URLs the store holds, oldest first — what the Console list would show.
fn stored_urls(cx: &gpui::App) -> Vec<String> {
    cx.global::<ConsoleStore>()
        .entries()
        .iter()
        .map(|entry| entry.url.clone())
        .collect()
}

#[gpui::test]
fn settings_change_trims_to_new_cap(cx: &mut TestAppContext) {
    // Shrinking `console_max_entries` while the app runs drops the oldest entries right
    // away — the memory comes back now, not at the next push (REQ-CON-02).
    cx.update(|cx| {
        let init = settings::SettingsStore::init(cx, Some(r#"{"console_max_entries": 3}"#));
        assert_eq!(init, Ok(()), "the cap is a setting the user can change");
        crate::init(cx);

        ConsoleStore::push(cx, entry("https://example.test/one"));
        ConsoleStore::push(cx, entry("https://example.test/two"));
        ConsoleStore::push(cx, entry("https://example.test/three"));
    });

    // The change arrives the way `SettingsStore::watch` delivers one: a fresh merge over
    // the defaults, notifying the store's settings subscription.
    cx.update(|cx| {
        let reload = settings::SettingsStore::reload(cx, Some(r#"{"console_max_entries": 1}"#));
        assert_eq!(
            reload,
            Ok(()),
            "the new cap is a settings blob the store accepts"
        );
    });
    cx.run_until_parked();

    cx.update(|cx| {
        assert_eq!(
            stored_urls(cx),
            ["https://example.test/three"],
            "the smaller cap trims at once, dropping the oldest entries first"
        );
    });
}
