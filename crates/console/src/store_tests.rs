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
fn cap_drops_oldest_entries(cx: &mut TestAppContext) {
    // The buffer keeps the newest entries up to `console_max_entries` and drops the
    // oldest first (REQ-CON-02), with the cap read from the settings like any other
    // user-set value.
    cx.update(|cx| {
        let init = settings::SettingsStore::init(cx, Some(r#"{"console_max_entries": 2}"#));
        assert_eq!(init, Ok(()), "the cap is a setting the user can change");
        crate::init(cx);

        ConsoleStore::push(cx, entry("https://example.test/one"));
        ConsoleStore::push(cx, entry("https://example.test/two"));
        ConsoleStore::push(cx, entry("https://example.test/three"));

        assert_eq!(
            stored_urls(cx),
            ["https://example.test/two", "https://example.test/three"],
            "the third push drops the oldest entry, not the newest ones"
        );
    });
}

#[gpui::test]
fn pause_stops_new_entries(cx: &mut TestAppContext) {
    // Paused, the store drops what arrives instead of recording it, and resuming takes
    // entries again (REQ-CON-02).
    cx.update(|cx| {
        let init = settings::SettingsStore::init(cx, None);
        assert_eq!(init, Ok(()), "the store starts from the bundled defaults");
        crate::init(cx);

        ConsoleStore::push(cx, entry("https://example.test/one"));
        ConsoleStore::set_paused(cx, true);
        ConsoleStore::push(cx, entry("https://example.test/two"));

        assert_eq!(
            stored_urls(cx),
            ["https://example.test/one"],
            "a push while paused lands nowhere"
        );

        ConsoleStore::set_paused(cx, false);
        ConsoleStore::push(cx, entry("https://example.test/three"));

        assert_eq!(
            stored_urls(cx),
            ["https://example.test/one", "https://example.test/three"],
            "resuming takes entries again, without the one dropped while paused"
        );
    });
}
